pub mod bifs;
pub mod buffer;
pub mod context;
mod pty;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::diagnostics::IrSpan;
use crate::dsl::resolver::ir::{IrCallExpr, IrExpr, IrInterpolation, IrShellStmt, IrStringPart};
use crate::dsl::resolver::ir::{IrFn, IrPureFn, Tables};
use crate::runtime::observe::event_log::{BufferSnapshot, EventCollector, LogEventKind};
use crate::runtime::observe::progress::{ProgressEvent, ProgressTx};
use crate::runtime::observe::shell_log::ShellLogger;
use crate::runtime::report::result::Failure;
use crate::runtime::vm::bifs::VmContext;
use crate::runtime::vm::buffer::{BUFFER_TAIL_LEN, FailPatternHit, regex_error_summary};
use crate::runtime::vm::context::ExecutionContext;
use crate::runtime::vm::context::{Captures, FailPattern};
use crate::runtime::vm::pty::PtyShell;

// ─── Interpolation helpers ──────────────────────────────────────

fn has_interpolation(expr: &IrInterpolation) -> bool {
    expr.parts().iter().any(|p| {
        matches!(
            p,
            IrStringPart::Var { .. } | IrStringPart::CaptureRef { .. }
        )
    })
}

fn interpolation_template(expr: &IrInterpolation) -> String {
    expr.parts()
        .iter()
        .map(|p| match p {
            IrStringPart::Literal { value, .. } => value.clone(),
            IrStringPart::Var { name, .. } => format!("${{{name}}}"),
            IrStringPart::EscapedDollar { .. } => "$".to_string(),
            IrStringPart::CaptureRef { index, .. } => format!("${{{index}}}"),
        })
        .collect()
}

async fn interpolate_ir(expr: &IrInterpolation, ctx: &ExecutionContext) -> String {
    let mut out = String::new();
    for part in expr.parts() {
        match part {
            IrStringPart::Literal { value, .. } => out.push_str(value),
            IrStringPart::Var { name, .. } => {
                if let Some(v) = ctx.lookup(name).await {
                    out.push_str(&v);
                }
            }
            IrStringPart::EscapedDollar { .. } => out.push('$'),
            IrStringPart::CaptureRef { index, .. } => {
                if let Some(v) = ctx.capture(*index) {
                    out.push_str(&v);
                }
            }
        }
    }
    out
}

// ─── Vm ─────────────────────────────────────────────────────────

pub struct Vm {
    pty: PtyShell,
    ctx: ExecutionContext,
    tables: Tables,
    shell_prompt: String,
    progress_tx: Option<ProgressTx>,
    event_collector: Option<EventCollector>,
    cancel: CancellationToken,
}

impl Vm {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        shell_name: String,
        shell_prompt: String,
        shell_command: String,
        ctx: ExecutionContext,
        tables: Tables,
        progress_tx: Option<ProgressTx>,
        log_dir: &Path,
        test_start: Instant,
        event_collector: Option<EventCollector>,
        cancel: CancellationToken,
    ) -> Result<Self, Failure> {
        let shell_log = ShellLogger::create(log_dir, &shell_name, test_start).map_err(|e| {
            Failure::Runtime {
                message: format!("failed to create shell log: {e}"),
                span: None,
                shell: Some(shell_name.clone()),
            }
        })?;
        let shell_log = Arc::new(Mutex::new(shell_log));

        let pty = PtyShell::spawn(&shell_command, ctx.process_env(), shell_log).map_err(|e| {
            Failure::Runtime {
                message: format!("failed to spawn shell: {e}"),
                span: None,
                shell: Some(shell_name.clone()),
            }
        })?;

        let mut vm = Self {
            pty,
            ctx,
            tables,
            shell_prompt,
            progress_tx,
            event_collector,
            cancel,
        };

        vm.emit_event(LogEventKind::ShellSpawn {
            name: shell_name.clone(),
            command: shell_command,
        })
        .await;

        vm.pty
            .init_prompt(&vm.shell_prompt, vm.ctx.timeout().adjusted_duration())
            .await
            .map_err(|_| Failure::Runtime {
                message: "shell did not produce prompt during init".to_string(),
                span: None,
                shell: Some(shell_name),
            })?;

        vm.emit_event(LogEventKind::ShellReady {
            name: vm.ctx.current_name().to_string(),
        })
        .await;

        Ok(vm)
    }

    /// Current display name for logging (resolves effect chain + alias).
    pub fn current_name(&self) -> String {
        self.ctx.current_name()
    }

    /// Reset the execution context for shell export (effect → test/parent effect).
    pub fn reset_for_export(&mut self, new_scope: context::Scope) {
        self.ctx.reset_for_export(new_scope);
    }

    pub async fn exec_stmts(&mut self, stmts: &[IrShellStmt]) -> Result<String, Failure> {
        let mut last = String::new();
        for stmt in stmts {
            if self.cancel.is_cancelled() {
                return Err(Failure::Cancelled {
                    span: None,
                    shell: Some(self.ctx.current_name().to_string()),
                });
            }
            last = self.exec_stmt(stmt).await?;
        }
        self.drain_recv_event().await;
        Ok(last)
    }

    async fn drain_recv_event(&mut self) {
        if let Some(_data) = self.pty.output_buf.drain_recv().await {
            // self.emit_event(LogEventKind::Recv { data }).await;
        }
    }

    async fn emit_interpolation(&mut self, expr: &IrInterpolation, result: &str) {
        if has_interpolation(expr) {
            let mut bindings = Vec::new();
            for part in expr.parts() {
                match part {
                    IrStringPart::Var { name, .. } => {
                        let value = self.ctx.lookup(name).await.unwrap_or_default();
                        bindings.push((name.clone(), value));
                    }
                    IrStringPart::CaptureRef { index, .. } => {
                        let name = index.to_string();
                        let value = self.ctx.capture(*index).unwrap_or_default();
                        bindings.push((name, value));
                    }
                    _ => {}
                }
            }
            self.emit_event(LogEventKind::Interpolation {
                template: interpolation_template(expr),
                result: result.to_string(),
                bindings,
            })
            .await;
        }
    }

    pub async fn exec_stmt(&mut self, stmt: &IrShellStmt) -> Result<String, Failure> {
        use crate::dsl::resolver::ir::IrNode;
        let span = stmt.span().clone();
        self.drain_recv_event().await;
        self.check_fail(span.clone()).await?;
        match stmt {
            IrShellStmt::Comment { .. } => Ok(String::new()),
            IrShellStmt::FailRegex {
                pattern,
                span: ir_span,
            } => {
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                self.emit_event(LogEventKind::FailPatternSet {
                    pattern: pat.clone(),
                })
                .await;
                let re = RegexBuilder::new(&pat)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(|e| Failure::Runtime {
                        message: format!("invalid fail regex: {}", regex_error_summary(&e)),
                        span: Some(ir_span.clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    })?;
                let fp = Some(FailPattern::Regex(re));
                self.ctx.set_fail_pattern(fp);
                self.check_fail(span).await?;
                Ok(String::new())
            }
            IrShellStmt::FailLiteral { pattern, .. } => {
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                self.emit_event(LogEventKind::FailPatternSet {
                    pattern: pat.clone(),
                })
                .await;
                let fp = Some(FailPattern::Literal(pat));
                self.ctx.set_fail_pattern(fp);
                self.check_fail(span).await?;
                Ok(String::new())
            }
            IrShellStmt::ClearFailPattern { .. } => {
                self.emit_event(LogEventKind::FailPatternCleared).await;
                self.ctx.set_fail_pattern(None);
                Ok(String::new())
            }
            IrShellStmt::Timeout { timeout, .. } => {
                let previous = format!("{:?}", self.ctx.timeout());
                self.ctx.set_timeout(timeout.clone());
                let new_timeout = format!("{:?}", self.ctx.timeout());
                self.emit_event(LogEventKind::TimeoutSet {
                    timeout: new_timeout,
                    previous,
                })
                .await;
                Ok(String::new())
            }
            IrShellStmt::Let { stmt: let_stmt, .. } => {
                let value = if let Some(expr) = let_stmt.value() {
                    self.eval_expr(expr).await?
                } else {
                    String::new()
                };
                self.emit_event(LogEventKind::VarLet {
                    name: let_stmt.name().name().to_string(),
                    value: value.clone(),
                })
                .await;
                self.ctx
                    .let_insert(let_stmt.name().name().to_string(), value.clone());
                Ok(value)
            }
            IrShellStmt::Assign { stmt: assign, .. } => {
                let value = self.eval_expr(assign.value()).await?;
                let found = self.ctx.assign(assign.name().name(), value.clone()).await;
                if !found {
                    return Err(Failure::Runtime {
                        message: format!(
                            "assignment to undeclared variable `{}`",
                            assign.name().name()
                        ),
                        span: Some(assign.name().span().clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    });
                }
                self.emit_event(LogEventKind::VarAssign {
                    name: assign.name().name().to_string(),
                    value: value.clone(),
                })
                .await;
                Ok(value)
            }
            IrShellStmt::Expr { expr, .. } => self.eval_expr(expr).await,
            IrShellStmt::Send { payload, .. } => {
                let data = interpolate_ir(payload, &self.ctx).await;
                self.emit_interpolation(payload, &data).await;
                self.send_bytes(format!("{data}\n").as_bytes(), span.clone())
                    .await?;
                self.emit_event(LogEventKind::Send { data: data.clone() })
                    .await;
                self.emit_progress(ProgressEvent::Send);
                Ok(data)
            }
            IrShellStmt::SendRaw { payload, .. } => {
                let data = interpolate_ir(payload, &self.ctx).await;
                self.emit_interpolation(payload, &data).await;
                self.send_bytes(data.as_bytes(), span.clone()).await?;
                self.emit_event(LogEventKind::Send { data: data.clone() })
                    .await;
                self.emit_progress(ProgressEvent::Send);
                Ok(data)
            }
            IrShellStmt::MatchLiteral { pattern, .. } => {
                let timeout = self.ctx.timeout().adjusted_duration();
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                self.emit_event(LogEventKind::MatchStart {
                    pattern: pat.clone(),
                    is_regex: false,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (mat, snapshot) = self.wait_consume_literal(&pat, timeout, span).await?;
                self.emit_event(LogEventKind::MatchDone {
                    matched: mat.value.0.clone(),
                    elapsed: match_start.elapsed(),
                    buffer: snapshot,
                    captures: None,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(pat)
            }
            IrShellStmt::MatchRegex { pattern, .. } => {
                let timeout = self.ctx.timeout().adjusted_duration();
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                let re = RegexBuilder::new(&pat)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(|e| Failure::Runtime {
                        message: format!("invalid regex: {}", regex_error_summary(&e)),
                        span: Some(pattern.span().clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    })?;
                self.emit_event(LogEventKind::MatchStart {
                    pattern: pat.clone(),
                    is_regex: true,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (mat, snapshot) = self
                    .wait_consume_regex(&pat, &re, timeout, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                self.emit_event(LogEventKind::MatchDone {
                    matched: full.clone(),
                    elapsed: match_start.elapsed(),
                    buffer: snapshot,
                    captures: Some(captures.clone()),
                })
                .await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::TimedMatchLiteral {
                timeout, pattern, ..
            } => {
                let dur = timeout.adjusted_duration();
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                self.emit_event(LogEventKind::MatchStart {
                    pattern: pat.clone(),
                    is_regex: false,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (mat, snapshot) = self.wait_consume_literal(&pat, dur, span).await?;
                self.emit_event(LogEventKind::MatchDone {
                    matched: mat.value.0.clone(),
                    elapsed: match_start.elapsed(),
                    buffer: snapshot,
                    captures: None,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(pat)
            }
            IrShellStmt::TimedMatchRegex {
                timeout, pattern, ..
            } => {
                let dur = timeout.adjusted_duration();
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat).await;
                let re = RegexBuilder::new(&pat)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(|e| Failure::Runtime {
                        message: format!("invalid regex: {}", regex_error_summary(&e)),
                        span: Some(pattern.span().clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    })?;
                self.emit_event(LogEventKind::MatchStart {
                    pattern: pat.clone(),
                    is_regex: true,
                })
                .await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (mat, snapshot) = self
                    .wait_consume_regex(&pat, &re, dur, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                self.emit_event(LogEventKind::MatchDone {
                    matched: full.clone(),
                    elapsed: match_start.elapsed(),
                    buffer: snapshot,
                    captures: Some(captures.clone()),
                })
                .await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::BufferReset { .. } => {
                let snapshot = self.pty.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::BufferReset { buffer: snapshot })
                    .await;
                self.pty.output_buf.clear().await;
                Ok(String::new())
            }
        }
    }

    fn set_captures_from_map(&mut self, map: HashMap<String, String>) {
        let mut caps = Captures::new();
        for (k, v) in map {
            caps.set(k, v);
        }
        self.ctx.set_captures(caps);
    }

    #[async_recursion::async_recursion]
    async fn eval_expr(&mut self, expr: &IrExpr) -> Result<String, Failure> {
        use crate::dsl::resolver::ir::IrNode;
        let span = expr.span().clone();
        self.check_fail(span.clone()).await?;
        match expr {
            IrExpr::String { value, .. } => {
                let result = interpolate_ir(value, &self.ctx).await;
                self.emit_interpolation(value, &result).await;
                self.emit_event(LogEventKind::StringEval {
                    result: result.clone(),
                })
                .await;
                Ok(result)
            }
            IrExpr::Var { name, .. } => Ok(self.ctx.lookup(name).await.unwrap_or_default()),
            IrExpr::CaptureRef { index, .. } => Ok(self.ctx.capture(*index).unwrap_or_default()),
            IrExpr::Call { call, .. } => self.eval_call(call, &span).await,
        }
    }

    async fn eval_call(&mut self, call: &IrCallExpr, span: &IrSpan) -> Result<String, Failure> {
        let fn_id = call.resolved().clone();
        let fn_name = call.name().name().to_string();

        // Evaluate args first
        let mut evaluated_args = Vec::with_capacity(call.args().len());
        for arg in call.args() {
            evaluated_args.push(self.eval_expr(arg).await?);
        }

        // Try user-defined function
        if let Some(result) = self.tables.fns.get(&fn_id) {
            let ir_fn = result.as_ref().map_err(|e| Failure::Runtime {
                message: format!("function resolution failed: {e:?}"),
                span: Some(span.clone()),
                shell: Some(self.ctx.current_name().to_string()),
            })?;
            match ir_fn {
                IrFn::UserDefined { params, body, .. } => {
                    let params = params.clone();
                    let body = body.clone();
                    let named_args: Vec<(String, String)> = params
                        .iter()
                        .zip(evaluated_args.iter())
                        .map(|(p, v)| (p.name().to_string(), v.clone()))
                        .collect();
                    self.emit_event(LogEventKind::FnEnter {
                        name: fn_name.clone(),
                        args: named_args.clone(),
                    })
                    .await;
                    self.emit_progress(ProgressEvent::FnEnter(fn_name.clone()));
                    let pre_timeout = format!("{:?}", self.ctx.timeout());
                    let pre_fail = self.ctx.fail_pattern().map(|p| format!("{p:?}"));
                    self.ctx
                        .push_call(fn_name.clone(), named_args.into_iter().collect());
                    let mut last = String::new();
                    for stmt in &body {
                        match self.exec_stmt(stmt).await {
                            Ok(v) => last = v,
                            Err(e) => {
                                self.ctx.pop_call();
                                return Err(e);
                            }
                        }
                    }
                    self.ctx.pop_call();
                    let post_timeout = format!("{:?}", self.ctx.timeout());
                    let post_fail = self.ctx.fail_pattern().map(|p| format!("{p:?}"));
                    self.emit_event(LogEventKind::FnExit {
                        name: fn_name,
                        return_value: last.clone(),
                        restored_timeout: if post_timeout != pre_timeout {
                            Some(post_timeout)
                        } else {
                            None
                        },
                        restored_fail_pattern: if post_fail != pre_fail {
                            post_fail
                        } else {
                            None
                        },
                    })
                    .await;
                    self.emit_progress(ProgressEvent::FnExit);
                    return Ok(last);
                }
                IrFn::Builtin { name, arity } => {
                    // Impure builtin
                    if let Some(bif) = bifs::lookup_impure(name, *arity) {
                        let positional_args: Vec<(String, String)> = evaluated_args
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (format!("${i}"), v.clone()))
                            .collect();
                        self.emit_event(LogEventKind::FnEnter {
                            name: fn_name.clone(),
                            args: positional_args,
                        })
                        .await;
                        let result = bif.call(self, evaluated_args, span).await;
                        let return_value = result.clone().unwrap_or_default();
                        self.emit_event(LogEventKind::FnExit {
                            name: fn_name,
                            return_value,
                            restored_timeout: None,
                            restored_fail_pattern: None,
                        })
                        .await;
                        return result;
                    }
                }
            }
        }

        // Try pure function
        if let Some(result) = self.tables.pure_fns.get(&fn_id) {
            let ir_fn = result.as_ref().map_err(|e| Failure::Runtime {
                message: format!("pure function resolution failed: {e:?}"),
                span: Some(span.clone()),
                shell: Some(self.ctx.current_name().to_string()),
            })?;
            let named_args: Vec<(String, String)> = match ir_fn {
                IrPureFn::UserDefined { params, .. } => params
                    .iter()
                    .zip(evaluated_args.iter())
                    .map(|(p, v)| (p.name().to_string(), v.clone()))
                    .collect(),
                IrPureFn::Builtin { .. } => evaluated_args
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("${i}"), v.clone()))
                    .collect(),
            };
            self.emit_event(LogEventKind::FnEnter {
                name: fn_name.clone(),
                args: named_args,
            })
            .await;
            self.emit_progress(ProgressEvent::FnEnter(fn_name.clone()));
            let return_value = crate::pure::evaluator::eval_pure_fn(
                ir_fn,
                evaluated_args,
                &self.ctx.env,
                &self.tables.pure_fns,
            );
            self.emit_event(LogEventKind::FnExit {
                name: fn_name,
                return_value: return_value.clone(),
                restored_timeout: None,
                restored_fail_pattern: None,
            })
            .await;
            self.emit_progress(ProgressEvent::FnExit);
            return Ok(return_value);
        }

        Err(Failure::Runtime {
            message: format!(
                "undefined function `{}` with arity {}",
                fn_name,
                call.args().len()
            ),
            span: Some(span.clone()),
            shell: Some(self.ctx.current_name().to_string()),
        })
    }

    // ─── Wait + consume/peek helpers ────────────────────────────

    async fn wait_consume_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: IrSpan,
    ) -> Result<(buffer::Match<buffer::LiteralMatch>, BufferSnapshot), Failure> {
        let fut = async {
            loop {
                // Register the notified future BEFORE checking the buffer to
                // avoid missed wakeups: if data arrives between the check and
                // the await, the notification is already captured.
                let notified = self.pty.output_buf.notify.notified();
                let fail_pat = self.ctx.fail_pattern();
                match self
                    .pty
                    .output_buf
                    .fail_check_consume_literal(pattern, fail_pat)
                    .await
                {
                    Err(hit) => {
                        return Err(self.make_fail_pattern_error(hit, span.clone()).await);
                    }
                    Ok(Some(result)) => {
                        return Ok::<(buffer::Match<buffer::LiteralMatch>, BufferSnapshot), Failure>(
                            result,
                        );
                    }
                    Ok(None) => {}
                }
                tokio::select! {
                    _ = notified => {}
                    _ = self.cancel.cancelled() => {
                        return Err(Failure::Cancelled {
                            span: Some(span.clone()),
                            shell: Some(self.ctx.current_name().to_string()),
                        });
                    }
                }
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.pty.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::Timeout {
                    pattern: pattern.to_string(),
                    buffer,
                })
                .await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.ctx.current_name().to_string(),
                })
            }
        }
    }

    async fn wait_consume_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: Duration,
        span: IrSpan,
    ) -> Result<(buffer::Match<buffer::RegexMatch>, BufferSnapshot), Failure> {
        let fut = async {
            loop {
                let notified = self.pty.output_buf.notify.notified();
                let fail_pat = self.ctx.fail_pattern();
                match self
                    .pty
                    .output_buf
                    .fail_check_consume_regex(re, fail_pat)
                    .await
                {
                    Err(hit) => {
                        return Err(self.make_fail_pattern_error(hit, span.clone()).await);
                    }
                    Ok(Some(result)) => {
                        return Ok::<(buffer::Match<buffer::RegexMatch>, BufferSnapshot), Failure>(
                            result,
                        );
                    }
                    Ok(None) => {}
                }
                tokio::select! {
                    _ = notified => {}
                    _ = self.cancel.cancelled() => {
                        return Err(Failure::Cancelled {
                            span: Some(span.clone()),
                            shell: Some(self.ctx.current_name().to_string()),
                        });
                    }
                }
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.pty.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::Timeout {
                    pattern: pattern.to_string(),
                    buffer,
                })
                .await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.ctx.current_name().to_string(),
                })
            }
        }
    }

    async fn check_fail(&self, span: IrSpan) -> Result<(), Failure> {
        let fail_pat = self.ctx.fail_pattern();
        if let Some(hit) = self.pty.output_buf.check_fail_pattern(fail_pat).await {
            return Err(self.make_fail_pattern_error(hit, span).await);
        }
        Ok(())
    }

    async fn make_fail_pattern_error(&self, hit: FailPatternHit, span: IrSpan) -> Failure {
        self.emit_progress(ProgressEvent::FailPattern);
        let buffer = self.pty.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
        self.emit_event(LogEventKind::FailPatternTriggered {
            pattern: hit.pattern.clone(),
            matched_line: hit.matched_text.clone(),
            buffer,
        })
        .await;
        Failure::FailPatternMatched {
            pattern: hit.pattern,
            matched_line: hit.matched_text,
            span,
            shell: self.ctx.current_name().to_string(),
        }
    }

    async fn send_bytes(&mut self, data: &[u8], span: IrSpan) -> Result<(), Failure> {
        self.pty
            .send_bytes(data)
            .await
            .map_err(|e| Failure::ShellExited {
                shell: self.ctx.current_name().to_string(),
                exit_code: e.raw_os_error(),
                span,
            })
    }

    pub async fn shutdown(&mut self) {
        self.emit_event(LogEventKind::ShellTerminate {
            name: self.ctx.current_name().to_string(),
        })
        .await;
        self.pty.shutdown().await;
    }
}

impl Vm {
    async fn emit_event(&self, kind: LogEventKind) {
        if let Some(ec) = &self.event_collector {
            ec.push(&self.ctx.current_name(), kind).await;
        }
    }
}

#[async_trait::async_trait]
impl VmContext for Vm {
    fn emit_progress(&self, event: ProgressEvent) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(event);
        }
    }

    async fn emit_log(&mut self, message: String) {
        self.emit_event(LogEventKind::Log { message }).await;
    }

    async fn emit_sleep(&mut self, duration: std::time::Duration) {
        self.emit_event(LogEventKind::Sleep { duration }).await;
    }

    async fn emit_annotate(&mut self, text: String) {
        self.emit_event(LogEventKind::Annotate { text }).await;
    }

    async fn match_literal(&mut self, pattern: &str, span: &IrSpan) -> Result<String, Failure> {
        self.emit_event(LogEventKind::MatchStart {
            pattern: pattern.to_string(),
            is_regex: false,
        })
        .await;
        self.emit_progress(ProgressEvent::MatchStart);
        let match_start = Instant::now();
        let timeout = self.ctx.timeout().adjusted_duration();
        let (mat, snapshot) = self
            .wait_consume_literal(pattern, timeout, span.clone())
            .await?;
        self.emit_event(LogEventKind::MatchDone {
            matched: mat.value.0.clone(),
            elapsed: match_start.elapsed(),
            buffer: snapshot,
            captures: None,
        })
        .await;
        self.emit_progress(ProgressEvent::MatchDone);
        Ok(pattern.to_string())
    }

    async fn send_line(&mut self, line: &str, span: &IrSpan) -> Result<(), Failure> {
        self.send_bytes(format!("{line}\n").as_bytes(), span.clone())
            .await?;
        self.emit_event(LogEventKind::Send {
            data: line.to_string(),
        })
        .await;
        self.emit_progress(ProgressEvent::Send);
        Ok(())
    }

    async fn send_raw(&mut self, data: &[u8], span: &IrSpan) -> Result<(), Failure> {
        self.send_bytes(data, span.clone()).await?;
        let display = data
            .iter()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>();
        self.emit_event(LogEventKind::Send { data: display }).await;
        self.emit_progress(ProgressEvent::Send);
        Ok(())
    }

    fn shell_prompt(&self) -> &str {
        &self.shell_prompt
    }
}
