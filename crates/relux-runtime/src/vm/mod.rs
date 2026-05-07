pub mod bifs;
pub mod buffer;
pub mod context;
mod pty;

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use regex::Regex;
use regex::RegexBuilder;
use tokio_util::sync::CancellationToken;

use crate::RuntimeContext;
use crate::observe::structured::SpanId;
use crate::observe::structured::SpanKind;
use crate::observe::structured::StructuredLogBuilder;
use crate::report::result::Failure;
use crate::vm::buffer::FailPatternHit;
use crate::vm::buffer::MatchContext;
use crate::vm::buffer::regex_error_summary;
use crate::vm::context::Captures;
use crate::vm::context::ExecutionContext;
use crate::vm::context::FailPattern;
use crate::vm::pty::PtyShell;
use relux_core::diagnostics::IrSpan;
use relux_ir::IrCallExpr;
use relux_ir::IrExpr;
use relux_ir::IrFn;
use relux_ir::IrInterpolation;
use relux_ir::IrPureFn;
use relux_ir::IrShellStmt;
use relux_ir::IrStringPart;
use relux_ir::Tables;

// ─── Interpolation helpers ──────────────────────────────────────

fn has_interpolation(expr: &IrInterpolation) -> bool {
    expr.parts().iter().any(|p| {
        matches!(
            p,
            IrStringPart::Var { .. }
                | IrStringPart::QualifiedVar { .. }
                | IrStringPart::CaptureRef { .. }
        )
    })
}

fn interpolation_template(expr: &IrInterpolation) -> String {
    expr.parts()
        .iter()
        .map(|p| match p {
            IrStringPart::Literal { value, .. } => value.clone(),
            IrStringPart::Var { name, .. } => format!("${{{name}}}"),
            IrStringPart::QualifiedVar {
                qualifier, name, ..
            } => format!("${{{qualifier}.{name}}}"),
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
            IrStringPart::QualifiedVar {
                qualifier, name, ..
            } => {
                let qualified = format!("{qualifier}.{name}");
                if let Some(v) = ctx.lookup(&qualified).await {
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
    pub log: StructuredLogBuilder,
    shell_prompt: String,
    pub(crate) cancel: CancellationToken,
    flaky_timeout_multiplier: f64,
    exit_span: Option<IrSpan>,
}

impl Vm {
    pub async fn new(
        shell_name: String,
        ctx: ExecutionContext,
        rt_ctx: &RuntimeContext,
    ) -> Result<Self, Failure> {
        let shell_command = rt_ctx.shell.command.to_string();
        let shell_prompt = rt_ctx.shell.prompt.to_string();

        let log = rt_ctx.log.clone();
        let pty = PtyShell::spawn(
            &shell_command,
            ctx.process_env(),
            log.clone(),
            shell_name.clone(),
        )
        .map_err(|e| Failure::Runtime {
            message: format!("failed to spawn shell: {e}"),
            span: None,
            shell: Some(shell_name.clone()),
        })?;

        let cancel = rt_ctx.cancel.clone();
        let span = ctx.current_span();

        let mut vm = Self {
            pty,
            ctx,
            tables: rt_ctx.tables.clone(),
            log: log.clone(),
            shell_prompt,
            cancel,
            flaky_timeout_multiplier: rt_ctx.flaky_timeout_multiplier,
            exit_span: None,
        };

        log.emit_shell_spawn(span, &shell_name, &shell_command);

        vm.pty
            .init_prompt(
                &vm.shell_prompt,
                vm.ctx
                    .timeout()
                    .adjusted_duration_with_flaky(vm.flaky_timeout_multiplier),
            )
            .await
            .map_err(|_| Failure::Runtime {
                message: "shell did not produce prompt during init".to_string(),
                span: None,
                shell: Some(shell_name),
            })?;

        let ready_shell = vm.ctx.current_name();
        vm.log.emit_shell_ready(span, &ready_shell);

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

    pub fn shell_prompt(&self) -> &str {
        &self.shell_prompt
    }

    pub fn set_exit_span(&mut self, span: IrSpan) {
        self.exit_span = Some(span);
    }

    /// Re-parent all subsequent VM emissions onto the given block span.
    /// Called when a shell is reused across shell blocks.
    pub fn set_block_span(&mut self, span: SpanId) {
        self.ctx.set_block_span(span);
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
            // self.events.emit_recv(&self.ctx.current_name(), &data);
        }
    }

    fn current_span(&self) -> SpanId {
        self.ctx.current_span()
    }

    async fn emit_interpolation(
        &mut self,
        expr: &IrInterpolation,
        result: &str,
        _span: Option<&IrSpan>,
    ) {
        if has_interpolation(expr) {
            let mut bindings = Vec::new();
            for part in expr.parts() {
                match part {
                    IrStringPart::Var { name, .. } => {
                        let value = self.ctx.lookup(name).await.unwrap_or_default();
                        bindings.push((name.clone(), value));
                    }
                    IrStringPart::QualifiedVar {
                        qualifier, name, ..
                    } => {
                        let qualified = format!("{qualifier}.{name}");
                        let value = self.ctx.lookup(&qualified).await.unwrap_or_default();
                        bindings.push((qualified, value));
                    }
                    IrStringPart::CaptureRef { index, .. } => {
                        let name = index.to_string();
                        let value = self.ctx.capture(*index).unwrap_or_default();
                        bindings.push((name, value));
                    }
                    _ => {}
                }
            }
            let shell = self.ctx.current_name();
            self.log.emit_interpolation(
                self.current_span(),
                &shell,
                &interpolation_template(expr),
                result,
                &bindings,
            );
        }
    }

    pub async fn exec_stmt(&mut self, stmt: &IrShellStmt) -> Result<String, Failure> {
        use relux_ir::IrNode;
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
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log
                    .emit_fail_pattern_set(self.current_span(), &shell, &pat);
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
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log
                    .emit_fail_pattern_set(self.current_span(), &shell, &pat);
                let fp = Some(FailPattern::Literal(pat));
                self.ctx.set_fail_pattern(fp);
                self.check_fail(span).await?;
                Ok(String::new())
            }
            IrShellStmt::ClearFailPattern { .. } => {
                let shell = self.ctx.current_name();
                self.log
                    .emit_fail_pattern_cleared(self.current_span(), &shell);
                self.ctx.set_fail_pattern(None);
                Ok(String::new())
            }
            IrShellStmt::Timeout { timeout, .. } => {
                let previous = format!("{:?}", self.ctx.timeout());
                self.ctx.set_timeout(timeout.clone());
                let new_timeout = format!("{:?}", self.ctx.timeout());
                let shell = self.ctx.current_name();
                self.log
                    .emit_timeout_set(self.current_span(), &shell, &new_timeout, &previous);
                Ok(String::new())
            }
            IrShellStmt::Let { stmt: let_stmt, .. } => {
                let value = if let Some(expr) = let_stmt.value() {
                    self.eval_expr(expr).await?
                } else {
                    String::new()
                };
                let shell = self.ctx.current_name();
                self.log
                    .emit_var_let(self.current_span(), &shell, let_stmt.name().name(), &value);
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
                let shell = self.ctx.current_name();
                self.log
                    .emit_var_assign(self.current_span(), &shell, assign.name().name(), &value);
                Ok(value)
            }
            IrShellStmt::Expr { expr, .. } => self.eval_expr(expr).await,
            IrShellStmt::Send { payload, .. } => {
                let data = interpolate_ir(payload, &self.ctx).await;
                self.emit_interpolation(payload, &data, Some(&span)).await;
                self.send_bytes(format!("{data}\n").as_bytes(), span.clone())
                    .await?;
                let shell = self.ctx.current_name();
                self.log.emit_send(self.current_span(), &shell, &data);
                Ok(data)
            }
            IrShellStmt::SendRaw { payload, .. } => {
                let data = interpolate_ir(payload, &self.ctx).await;
                self.emit_interpolation(payload, &data, Some(&span)).await;
                self.send_bytes(data.as_bytes(), span.clone()).await?;
                let shell = self.ctx.current_name();
                self.log.emit_send(self.current_span(), &shell, &data);
                Ok(data)
            }
            IrShellStmt::MatchLiteral { pattern, .. } => {
                let timeout = self
                    .ctx
                    .timeout()
                    .adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log
                    .emit_match_start(self.current_span(), &shell, &pat, false);
                let match_start = Instant::now();
                let (mat, (before, _, after)) = self
                    .wait_consume_literal(&pat, timeout, span.clone())
                    .await?;
                let shell = self.ctx.current_name();
                self.log.emit_match_done(
                    self.current_span(),
                    &shell,
                    &mat.value.0,
                    match_start.elapsed(),
                    None,
                    &before,
                    &after,
                );
                Ok(pat)
            }
            IrShellStmt::MatchRegex { pattern, .. } => {
                let timeout = self
                    .ctx
                    .timeout()
                    .adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let re = RegexBuilder::new(&pat)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(|e| Failure::Runtime {
                        message: format!("invalid regex: {}", regex_error_summary(&e)),
                        span: Some(pattern.span().clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    })?;
                let shell = self.ctx.current_name();
                self.log
                    .emit_match_start(self.current_span(), &shell, &pat, true);
                let match_start = Instant::now();
                let (mat, (before, _, after)) = self
                    .wait_consume_regex(&pat, &re, timeout, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                let shell = self.ctx.current_name();
                self.log.emit_match_done(
                    self.current_span(),
                    &shell,
                    &full,
                    match_start.elapsed(),
                    Some(captures.clone()),
                    &before,
                    &after,
                );
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::TimedMatchLiteral {
                timeout, pattern, ..
            } => {
                let dur = timeout.adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log
                    .emit_match_start(self.current_span(), &shell, &pat, false);
                let match_start = Instant::now();
                let (mat, (before, _, after)) =
                    self.wait_consume_literal(&pat, dur, span.clone()).await?;
                let shell = self.ctx.current_name();
                self.log.emit_match_done(
                    self.current_span(),
                    &shell,
                    &mat.value.0,
                    match_start.elapsed(),
                    None,
                    &before,
                    &after,
                );
                Ok(pat)
            }
            IrShellStmt::TimedMatchRegex {
                timeout, pattern, ..
            } => {
                let dur = timeout.adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
                let pat = interpolate_ir(pattern, &self.ctx).await;
                self.emit_interpolation(pattern, &pat, Some(&span)).await;
                let re = RegexBuilder::new(&pat)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(|e| Failure::Runtime {
                        message: format!("invalid regex: {}", regex_error_summary(&e)),
                        span: Some(pattern.span().clone()),
                        shell: Some(self.ctx.current_name().to_string()),
                    })?;
                let shell = self.ctx.current_name();
                self.log
                    .emit_match_start(self.current_span(), &shell, &pat, true);
                let match_start = Instant::now();
                let (mat, (before, _, after)) = self
                    .wait_consume_regex(&pat, &re, dur, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                let shell = self.ctx.current_name();
                self.log.emit_match_done(
                    self.current_span(),
                    &shell,
                    &full,
                    match_start.elapsed(),
                    Some(captures.clone()),
                    &before,
                    &after,
                );
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::BufferReset { .. } => {
                let shell = self.ctx.current_name();
                let discarded = self.pty.output_buf.clear().await;
                self.log.emit_buffer_reset(&shell, &discarded);
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
        use relux_ir::IrNode;
        let span = expr.span().clone();
        self.check_fail(span.clone()).await?;
        match expr {
            IrExpr::String { value, .. } => {
                let result = interpolate_ir(value, &self.ctx).await;
                self.emit_interpolation(value, &result, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log
                    .emit_string_eval(self.current_span(), &shell, &result);
                Ok(result)
            }
            IrExpr::Var { name, .. } => Ok(self.ctx.lookup(name).await.unwrap_or_default()),
            IrExpr::QualifiedVar {
                qualifier, name, ..
            } => {
                let qualified = format!("{qualifier}.{name}");
                Ok(self.ctx.lookup(&qualified).await.unwrap_or_default())
            }
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
                    let parent_span = self.current_span();
                    let fn_span = self.log.open_span(
                        SpanKind::FnCall {
                            name: fn_name.clone(),
                            args: named_args.clone(),
                        },
                        Some(parent_span),
                        Some(span),
                    );
                    self.ctx.push_span(fn_span);
                    self.ctx
                        .push_call(fn_name.clone(), named_args.into_iter().collect());
                    let mut last = String::new();
                    for stmt in &body {
                        match self.exec_stmt(stmt).await {
                            Ok(v) => last = v,
                            Err(e) => {
                                self.ctx.pop_call();
                                self.ctx.pop_span();
                                self.log.close_span(fn_span);
                                return Err(e);
                            }
                        }
                    }
                    self.ctx.pop_call();
                    self.ctx.pop_span();
                    self.log.close_span(fn_span);
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
                        let parent_span = self.current_span();
                        let fn_span = self.log.open_span(
                            SpanKind::FnCall {
                                name: fn_name.clone(),
                                args: positional_args,
                            },
                            Some(parent_span),
                            Some(span),
                        );
                        self.ctx.push_span(fn_span);
                        let result = bif.call(self, evaluated_args, span).await;
                        self.ctx.pop_span();
                        self.log.close_span(fn_span);
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
            let parent_span = self.current_span();
            let fn_span = self.log.open_span(
                SpanKind::FnCall {
                    name: fn_name.clone(),
                    args: named_args,
                },
                Some(parent_span),
                Some(span),
            );
            self.ctx.push_span(fn_span);
            let return_value = relux_ir::evaluator::eval_pure_fn(
                ir_fn,
                evaluated_args,
                &self.ctx.env,
                &self.tables.pure_fns,
            );
            self.ctx.pop_span();
            self.log.close_span(fn_span);
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

    // ─── Public methods for BIFs ────────────────────────────────

    pub async fn match_literal(&mut self, pattern: &str, span: &IrSpan) -> Result<String, Failure> {
        let shell = self.ctx.current_name();
        self.log
            .emit_match_start(self.current_span(), &shell, pattern, false);
        let match_start = Instant::now();
        let timeout = self
            .ctx
            .timeout()
            .adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
        let (mat, (before, _, after)) = self
            .wait_consume_literal(pattern, timeout, span.clone())
            .await?;
        let shell = self.ctx.current_name();
        self.log.emit_match_done(
            self.current_span(),
            &shell,
            &mat.value.0,
            match_start.elapsed(),
            None,
            &before,
            &after,
        );
        Ok(pattern.to_string())
    }

    pub async fn send_line(&mut self, line: &str, span: &IrSpan) -> Result<(), Failure> {
        self.send_bytes(format!("{line}\n").as_bytes(), span.clone())
            .await?;
        let shell = self.ctx.current_name();
        self.log.emit_send(self.current_span(), &shell, line);
        Ok(())
    }

    pub async fn send_raw(&mut self, data: &[u8], span: &IrSpan) -> Result<(), Failure> {
        self.send_bytes(data, span.clone()).await?;
        let display = data
            .iter()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>();
        let shell = self.ctx.current_name();
        self.log.emit_send(self.current_span(), &shell, &display);
        Ok(())
    }

    // ─── Wait + consume/peek helpers ────────────────────────────

    async fn wait_consume_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: IrSpan,
    ) -> Result<(buffer::Match<buffer::LiteralMatch>, MatchContext), Failure> {
        let fut = async {
            loop {
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
                        return Ok::<(buffer::Match<buffer::LiteralMatch>, MatchContext), Failure>(
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
                let shell = self.ctx.current_name();
                self.log.emit_timeout(self.current_span(), &shell, pattern);
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
    ) -> Result<(buffer::Match<buffer::RegexMatch>, MatchContext), Failure> {
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
                        return Ok::<(buffer::Match<buffer::RegexMatch>, MatchContext), Failure>(
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
                let shell = self.ctx.current_name();
                self.log.emit_timeout(self.current_span(), &shell, pattern);
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
        let shell = self.ctx.current_name();
        self.log.emit_fail_pattern_triggered(
            self.current_span(),
            &shell,
            &hit.pattern,
            &hit.matched_text,
        );
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
        let shell = self.ctx.current_name();
        self.log.emit_shell_terminate(self.current_span(), &shell);
        self.pty.shutdown().await;
    }
}
