pub mod bifs;
pub mod buffer;
pub mod context;
mod pty;

use std::collections::HashMap;
use std::time::Instant;

use crate::cancel::CancelToken;
use regex::Regex;
use regex::RegexBuilder;

use crate::RuntimeContext;
use crate::observe::structured::EventSeq;
use crate::observe::structured::FnCallKind;
use crate::observe::structured::MatchContext;
use crate::observe::structured::SpanId;
use crate::observe::structured::SpanKind;
use crate::observe::structured::StructuredLogBuilder;
use crate::report::result::Cancellation;
use crate::report::result::ExecError;
use crate::report::result::Failure;
use crate::report::result::FailureContext;

/// Size of the per-shell buffer tail snapshotted into a failure record.
/// Tuned for "fits on one console screen, enough to see what arrived
/// instead of the expected pattern".
const BUFFER_TAIL_BYTES: usize = 4096;
use crate::vm::buffer::FailPatternHit;
use crate::vm::buffer::MultiMatchHit;
use crate::vm::buffer::PatternSlot;
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
use relux_ir::IrMultiMatchPattern;
use relux_ir::IrPureFn;
use relux_ir::IrShellStmt;
use relux_ir::IrTimeout;
use relux_ir::Tables;

/// Per-pattern state while a multimatch block is running.
#[derive(Debug)]
struct MultiSlot {
    slot: PatternSlot,
    /// Pattern source string (preserved verbatim from the IR for log
    /// emission and the failure record).
    source: String,
    is_regex: bool,
    /// `Some` once the pattern has matched in the current block.
    hit: Option<MultiMatchHit>,
    /// Set once `MultiMatchPatternDone` is emitted, so we never double-emit.
    done: bool,
    /// Set when the per-pattern `Matched` buffer event has been pushed -
    /// referenced by `MultiMatchPatternDone.buffer_seq`.
    buffer_seq: Option<EventSeq>,
}

// --- Vm --------------------------------------------------

pub struct Vm {
    pty: PtyShell,
    ctx: ExecutionContext,
    tables: Tables,
    pub log: StructuredLogBuilder,
    shell_prompt: String,
    pub(crate) cancel: CancelToken,
    flaky_timeout_multiplier: f64,
    terminated: bool,
    /// Stable identity for this shell. Same value for the entire VM
    /// lifetime, including after `reset_for_export`. Threaded into
    /// every structured/buffer event this VM emits.
    shell_marker: String,
}

impl Vm {
    pub async fn new(
        shell_name: String,
        shell_marker: String,
        ctx: ExecutionContext,
        rt_ctx: &RuntimeContext,
        block_span: IrSpan,
    ) -> Result<Self, ExecError> {
        let shell_command = rt_ctx.shell.command.to_string();
        let shell_prompt = rt_ctx.shell.prompt.to_string();

        let log = rt_ctx.log.clone();
        let pty = PtyShell::spawn(
            &shell_command,
            ctx.process_env(),
            log.clone(),
            shell_name.clone(),
            shell_marker.clone(),
        )
        .map_err(|e| Failure::Runtime {
            message: format!("failed to spawn shell: {e}"),
            span: block_span.clone(),
            shell: Some(shell_name.clone()),
            context: FailureContext::pre_vm_with_span(ctx.current_span()),
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
            terminated: false,
            shell_marker: shell_marker.clone(),
        };

        log.emit_shell_spawn(span, &shell_name, &shell_marker, &shell_command, None);

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
                span: block_span.clone(),
                shell: Some(shell_name),
                context: FailureContext::pre_vm_with_span(vm.ctx.current_span()),
            })?;

        let ready_shell = vm.ctx.current_name();
        vm.log
            .emit_shell_ready(span, &ready_shell, &shell_marker, None);

        Ok(vm)
    }

    /// Current display name for logging (resolves effect chain + alias).
    pub fn current_name(&self) -> String {
        self.ctx.current_name()
    }

    /// Stable identity for this shell (same across `reset_for_export`).
    pub fn shell_marker(&self) -> &str {
        &self.shell_marker
    }

    /// Reset the execution context for shell export (effect -> test/parent effect).
    pub fn reset_for_export(
        &mut self,
        new_scope: context::Scope,
        parent_alias: Option<String>,
        parent_effect_name: Option<String>,
        shell_local_name: String,
    ) {
        self.ctx.reset_for_export(
            new_scope,
            parent_alias,
            parent_effect_name,
            shell_local_name,
        );
    }

    pub fn shell_prompt(&self) -> &str {
        &self.shell_prompt
    }

    /// Re-parent all subsequent VM emissions onto the given block span.
    /// Called when a shell is reused across shell blocks.
    pub fn set_block_span(&mut self, span: SpanId) {
        self.ctx.set_block_span(span);
    }

    pub async fn exec_stmts(&mut self, stmts: &[IrShellStmt]) -> Result<String, ExecError> {
        let mut last = String::new();
        for stmt in stmts {
            if self.cancel.is_cancelled() {
                return Err(self.observed_cancel(None).await);
            }
            last = self.exec_stmt(stmt).await?;
        }
        Ok(last)
    }

    /// Build an `ExecError::Cancelled` from the current cancel-token state.
    /// Emits the `cancelled` event on the current span before returning so
    /// that the event ordering matches "VM observed cancel, then unwound".
    pub(crate) async fn observed_cancel(&self, span: Option<IrSpan>) -> ExecError {
        let context = self.capture_failure_context().await;
        let reason = self
            .cancel
            .reason()
            .expect("production cancels always carry a reason");
        let shell = self.ctx.current_name();
        self.log.emit_cancelled(
            self.current_span(),
            Some(&shell),
            Some(&self.shell_marker),
            &reason,
        );
        let _ = span;
        ExecError::Cancelled(Cancellation { reason, context })
    }

    fn current_span(&self) -> SpanId {
        self.ctx.current_span()
    }

    /// Snapshot the diagnostic context for a `Failure` produced by this VM.
    /// Captures the active span, the latest event seq, the resolved call
    /// stack, the failing shell's buffer tail, and user-visible vars.
    /// Must be called *at* the failure construction site - once the VM is
    /// dropped the buffer is gone.
    pub(crate) async fn capture_failure_context(&self) -> FailureContext {
        let call_stack = self.log.resolve_stack(self.ctx.current_span());
        self.capture_failure_context_with_stack(call_stack).await
    }

    /// Like `capture_failure_context`, but with a pre-resolved call stack.
    /// The pure-fn-call error path resolves the stack from the innermost
    /// still-open pure-fn span (see `LogSink::deepest_open_span`) so the
    /// nested pure-fn frames appear; those frames are descendants of the
    /// enclosing `FnCall` span and would be missed by resolving from
    /// `current_span`.
    pub(crate) async fn capture_failure_context_with_stack(
        &self,
        call_stack: Vec<crate::observe::structured::failure::StackFrame>,
    ) -> FailureContext {
        FailureContext::Vm {
            span: self.ctx.current_span(),
            event_seq: self.log.current_seq(),
            call_stack,
            buffer_tail: self.pty.output_buf.snapshot_tail(BUFFER_TAIL_BYTES).await,
            vars_in_scope: self.ctx.snapshot_user_vars().await,
        }
    }

    /// Resolve an interpolation and emit its event in one walk. Locks the
    /// (uncontended) scope vars, hands the shared renderer this shell's live
    /// resolution chain, emits the Interpolation event when a value-bearing
    /// part was present, and returns the substituted string.
    async fn render_interp(&mut self, expr: &IrInterpolation, location: Option<&IrSpan>) -> String {
        let guard = self.ctx.scope.vars().lock().await;
        let (scopes, env) = self.ctx.interp_chain(&guard);
        let captures = self.ctx.current_captures_map();
        let rendered = relux_ir::evaluator::render_interpolation(expr, &scopes, env, captures);
        drop(guard);
        if rendered.emitted {
            let shell = self.ctx.current_name();
            self.log.emit_interpolation(
                self.current_span(),
                Some(&shell),
                Some(&self.shell_marker),
                &rendered.template,
                &rendered.result,
                &rendered.bindings,
                location,
            );
        }
        rendered.result
    }

    pub async fn exec_stmt(&mut self, stmt: &IrShellStmt) -> Result<String, ExecError> {
        use relux_ir::IrNode;
        let span = stmt.span().clone();
        self.check_fail(span.clone()).await?;
        match stmt {
            IrShellStmt::Comment { .. } => Ok(String::new()),
            IrShellStmt::FailRegex {
                pattern,
                span: ir_span,
            } => {
                let pat = self.render_interp(pattern, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_fail_pattern_set(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    true,
                    Some(&span),
                );
                let re = match RegexBuilder::new(&pat).multi_line(true).crlf(true).build() {
                    Ok(re) => re,
                    Err(e) => {
                        let context = self.capture_failure_context().await;
                        return Err(Failure::Runtime {
                            message: format!("invalid fail regex: {}", regex_error_summary(&e)),
                            span: ir_span.clone(),
                            shell: Some(self.ctx.current_name().to_string()),
                            context,
                        }
                        .into());
                    }
                };
                let fp = Some(FailPattern::Regex(re));
                self.ctx.set_fail_pattern(fp);
                self.check_fail(span).await?;
                Ok(String::new())
            }
            IrShellStmt::FailLiteral { pattern, .. } => {
                let pat = self.render_interp(pattern, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_fail_pattern_set(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    false,
                    Some(&span),
                );
                let fp = Some(FailPattern::Literal(pat));
                self.ctx.set_fail_pattern(fp);
                self.check_fail(span).await?;
                Ok(String::new())
            }
            IrShellStmt::ClearFailPattern { .. } => {
                let shell = self.ctx.current_name();
                self.log.emit_fail_pattern_cleared(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    Some(&span),
                );
                self.ctx.set_fail_pattern(None);
                Ok(String::new())
            }
            IrShellStmt::Timeout { timeout, .. } => {
                let previous = self.ctx.timeout().clone();
                self.ctx.set_timeout(timeout.clone());
                let shell = self.ctx.current_name();
                self.log.emit_timeout_set(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    self.ctx.timeout(),
                    &previous,
                    Some(&span),
                );
                Ok(String::new())
            }
            IrShellStmt::Let { stmt: let_stmt, .. } => {
                let value = if let Some(expr) = let_stmt.value() {
                    self.eval_expr(expr).await?
                } else {
                    String::new()
                };
                let shell = self.ctx.current_name();
                self.log.emit_var_let(
                    self.current_span(),
                    Some(&shell),
                    Some(&self.shell_marker),
                    let_stmt.name().name(),
                    &value,
                    Some(&span),
                );
                self.ctx
                    .let_insert(let_stmt.name().name().to_string(), value.clone());
                Ok(value)
            }
            IrShellStmt::Assign { stmt: assign, .. } => {
                let value = self.eval_expr(assign.value()).await?;
                let Some(previous) = self.ctx.assign(assign.name().name(), value.clone()).await
                else {
                    let context = self.capture_failure_context().await;
                    return Err(Failure::Runtime {
                        message: format!(
                            "assignment to undeclared variable `{}`",
                            assign.name().name()
                        ),
                        span: assign.name().span().clone(),
                        shell: Some(self.ctx.current_name().to_string()),
                        context,
                    }
                    .into());
                };
                let shell = self.ctx.current_name();
                self.log.emit_var_assign(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    assign.name().name(),
                    &value,
                    &previous,
                    Some(&span),
                );
                Ok(value)
            }
            IrShellStmt::Expr { expr, .. } => self.eval_expr(expr).await,
            IrShellStmt::Send { payload, .. } => {
                let data = self.render_interp(payload, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_send(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &data,
                    Some(&span),
                );
                self.send_bytes(format!("{data}\n").as_bytes(), span.clone())
                    .await?;
                Ok(data)
            }
            IrShellStmt::SendRaw { payload, .. } => {
                let data = self.render_interp(payload, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_send(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &data,
                    Some(&span),
                );
                self.send_bytes(data.as_bytes(), span.clone()).await?;
                Ok(data)
            }
            IrShellStmt::MatchLiteral { pattern, .. } => {
                let timeout = self.ctx.timeout().clone();
                let pat = self.render_interp(pattern, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_match_start(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    false,
                    &timeout,
                    Some(&span),
                );
                let match_start = Instant::now();
                let (mat, buffer_seq) = self
                    .wait_consume_literal(&pat, &timeout, span.clone())
                    .await?;
                let shell = self.ctx.current_name();
                self.log.emit_match_done_record(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &mat.value.0,
                    match_start.elapsed(),
                    None,
                    buffer_seq,
                    Some(&span),
                );
                Ok(pat)
            }
            IrShellStmt::MatchRegex { pattern, .. } => {
                let timeout = self.ctx.timeout().clone();
                let pat = self.render_interp(pattern, Some(&span)).await;
                let re = match RegexBuilder::new(&pat).multi_line(true).crlf(true).build() {
                    Ok(re) => re,
                    Err(e) => {
                        let context = self.capture_failure_context().await;
                        return Err(Failure::Runtime {
                            message: format!("invalid regex: {}", regex_error_summary(&e)),
                            span: pattern.span().clone(),
                            shell: Some(self.ctx.current_name().to_string()),
                            context,
                        }
                        .into());
                    }
                };
                let shell = self.ctx.current_name();
                self.log.emit_match_start(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    true,
                    &timeout,
                    Some(&span),
                );
                let match_start = Instant::now();
                let (mat, buffer_seq) = self
                    .wait_consume_regex(&pat, &re, &timeout, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                let shell = self.ctx.current_name();
                self.log.emit_match_done_record(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &full,
                    match_start.elapsed(),
                    Some(captures.clone()),
                    buffer_seq,
                    Some(&span),
                );
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::PureMatch {
                lhs,
                pattern,
                is_regex,
                ..
            } => {
                let value = self.eval_expr(lhs).await?;
                let pat = self.render_interp(pattern, Some(&span)).await;
                let outcome = {
                    let mut sink = crate::observe::structured::log_sink::LogSink::new_in_shell(
                        &self.log,
                        self.current_span(),
                        self.ctx.current_name(),
                        self.shell_marker.clone(),
                    );
                    relux_ir::eval_pure_match(&mut sink, &value, &pat, *is_regex, &span)
                };
                match outcome {
                    Ok(Some(hit)) => {
                        let matched = hit.matched_text;
                        if *is_regex {
                            self.set_captures_from_map(hit.captures);
                        }
                        Ok(matched)
                    }
                    Ok(None) => {
                        let context = self.capture_failure_context().await;
                        let match_context = match self.ctx.current_fn_name() {
                            Some(name) => MatchContext::Fn {
                                name: name.to_string(),
                            },
                            None => MatchContext::Shell {
                                name: self.ctx.current_name(),
                            },
                        };
                        Err(Failure::PureMatch {
                            value,
                            pattern: pat,
                            is_regex: *is_regex,
                            span: span.clone(),
                            match_context,
                            context,
                        }
                        .into())
                    }
                    Err(e) => {
                        let context = self.capture_failure_context().await;
                        Err(Failure::Runtime {
                            message: crate::report::result::invalid_regex_message(&e.reason),
                            span: span.clone(),
                            shell: Some(self.ctx.current_name().to_string()),
                            context,
                        }
                        .into())
                    }
                }
            }
            IrShellStmt::TimedMatchLiteral {
                timeout, pattern, ..
            } => {
                let pat = self.render_interp(pattern, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_match_start(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    false,
                    timeout,
                    Some(&span),
                );
                let match_start = Instant::now();
                let (mat, buffer_seq) = self
                    .wait_consume_literal(&pat, timeout, span.clone())
                    .await?;
                let shell = self.ctx.current_name();
                self.log.emit_match_done_record(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &mat.value.0,
                    match_start.elapsed(),
                    None,
                    buffer_seq,
                    Some(&span),
                );
                Ok(pat)
            }
            IrShellStmt::TimedMatchRegex {
                timeout, pattern, ..
            } => {
                let pat = self.render_interp(pattern, Some(&span)).await;
                let re = match RegexBuilder::new(&pat).multi_line(true).crlf(true).build() {
                    Ok(re) => re,
                    Err(e) => {
                        let context = self.capture_failure_context().await;
                        return Err(Failure::Runtime {
                            message: format!("invalid regex: {}", regex_error_summary(&e)),
                            span: pattern.span().clone(),
                            shell: Some(self.ctx.current_name().to_string()),
                            context,
                        }
                        .into());
                    }
                };
                let shell = self.ctx.current_name();
                self.log.emit_match_start(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &pat,
                    true,
                    timeout,
                    Some(&span),
                );
                let match_start = Instant::now();
                let (mat, buffer_seq) = self
                    .wait_consume_regex(&pat, &re, timeout, span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                let captures = mat.value.0.clone();
                let shell = self.ctx.current_name();
                self.log.emit_match_done_record(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &full,
                    match_start.elapsed(),
                    Some(captures.clone()),
                    buffer_seq,
                    Some(&span),
                );
                self.set_captures_from_map(captures);
                Ok(full)
            }
            IrShellStmt::MultiMatch {
                timeout, patterns, ..
            } => self.exec_multimatch(patterns, timeout.as_ref(), span).await,
            IrShellStmt::BufferReset { .. } => {
                // `clear` emits the `Reset` buffer event internally under
                // the output_buf lock, so no separate emit is needed here.
                let _consumed = self.pty.output_buf.clear().await;
                Ok(String::new())
            }
        }
    }

    async fn exec_multimatch(
        &mut self,
        patterns: &[IrMultiMatchPattern],
        timeout: Option<&IrTimeout>,
        span: IrSpan,
    ) -> Result<String, ExecError> {
        use relux_ir::IrNode;
        let effective = timeout
            .cloned()
            .unwrap_or_else(|| self.ctx.timeout().clone());

        // 1. Interpolate patterns + emit per-pattern interpolation events.
        let mut compiled: Vec<MultiSlot> = Vec::with_capacity(patterns.len());
        for ir_pat in patterns {
            let resolved = self.render_interp(ir_pat.pattern(), Some(&span)).await;
            let slot = if ir_pat.is_regex() {
                let re = match RegexBuilder::new(&resolved)
                    .multi_line(true)
                    .crlf(true)
                    .build()
                {
                    Ok(re) => re,
                    Err(e) => {
                        let context = self.capture_failure_context().await;
                        return Err(Failure::Runtime {
                            message: format!("invalid regex: {}", regex_error_summary(&e)),
                            span: ir_pat.pattern().span().clone(),
                            shell: Some(self.ctx.current_name().to_string()),
                            context,
                        }
                        .into());
                    }
                };
                PatternSlot::regex(resolved.clone(), re)
            } else {
                PatternSlot::literal(resolved.clone())
            };
            compiled.push(MultiSlot {
                slot,
                source: resolved.clone(),
                is_regex: ir_pat.is_regex(),
                hit: None,
                done: false,
                buffer_seq: None,
            });
        }

        // 2. Open the multi-match span.
        let shell = self.ctx.current_name();
        let parent_span = self.current_span();
        let mm_guard = self
            .log
            .open_multimatch_span(parent_span, &shell, Some(&span));
        let mm_span_id = mm_guard.id();
        self.ctx.push_span(mm_span_id);

        // 3. Emit MultiMatchStart with pattern metadata.
        let pattern_meta: Vec<crate::observe::structured::MultiMatchPattern> = compiled
            .iter()
            .map(|m| crate::observe::structured::MultiMatchPattern {
                pattern: m.source.clone(),
                is_regex: m.is_regex,
            })
            .collect();
        self.log.emit_multimatch_start(
            mm_span_id,
            &shell,
            &self.shell_marker,
            &pattern_meta,
            &effective,
            Some(&span),
        );

        // 4. Snapshot block-entry offset and start timer.
        let block_entry = self.pty.output_buf.base_offset().await;
        let block_start = Instant::now();

        // 5. Run the scan loop. Terminal lifecycle events emit while the
        //    span is still open.
        let outcome = self
            .wait_multimatch(
                &mut compiled,
                block_entry,
                block_start,
                &effective,
                &span,
                mm_span_id,
            )
            .await;

        // 6. Close the multi-match span.
        self.ctx.pop_span();
        drop(mm_guard);

        outcome.map(|()| String::new())
    }

    async fn wait_multimatch(
        &self,
        slots: &mut [MultiSlot],
        block_entry: usize,
        block_start: Instant,
        timeout: &IrTimeout,
        span: &IrSpan,
        mm_span_id: SpanId,
    ) -> Result<(), ExecError> {
        let dur = timeout.adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
        let shell = self.ctx.current_name();

        let fut = async {
            loop {
                let notified = self.pty.output_buf.notify.notified();

                // 1. Fail-pattern check (peek-only, no drain).
                let fail_pat = self.ctx.fail_pattern();
                if let Some(hit) = self.pty.output_buf.check_fail_pattern(fail_pat).await {
                    return Err(self.make_fail_pattern_error(hit, span.clone()).await);
                }

                // 2. Build a temporary slice of pattern slots that are still
                //    unmatched, then scan in one pass.
                let mut active_idx: Vec<usize> = Vec::with_capacity(slots.len());
                let mut active_slots: Vec<PatternSlot> = Vec::with_capacity(slots.len());
                for (i, s) in slots.iter().enumerate() {
                    if s.hit.is_none() {
                        active_idx.push(i);
                        active_slots.push(s.slot.clone());
                    }
                }
                let hits = self
                    .pty
                    .output_buf
                    .multimatch_scan(&mut active_slots, block_entry)
                    .await;

                // 3. For each newly-matched slot, emit the per-pattern
                //    `Matched` buffer event then `MultiMatchPatternDone`.
                let mut max_end: Option<(usize, EventSeq)> = None;
                for (k, hit_opt) in hits.into_iter().enumerate() {
                    let Some(hit) = hit_opt else { continue };
                    let i = active_idx[k];
                    let buffer_seq = self.pty.output_buf.push_multimatch_matched_event(
                        hit.before.clone(),
                        hit.matched_text.clone(),
                        hit.after.clone(),
                    );
                    let end_abs = hit.end_abs;
                    slots[i].hit = Some(hit);
                    slots[i].buffer_seq = Some(buffer_seq);
                    slots[i].done = true;
                    let elapsed = block_start.elapsed();
                    self.log.emit_multimatch_pattern_done(
                        mm_span_id,
                        &shell,
                        &self.shell_marker,
                        i,
                        elapsed,
                        buffer_seq,
                        Some(span),
                    );
                    match max_end {
                        Some((cur, _)) if end_abs <= cur => {}
                        _ => max_end = Some((end_abs, buffer_seq)),
                    }
                }
                // Recompute across all matched slots so the chosen advance
                // is the farthest hit overall, not just within this round.
                for s in slots.iter() {
                    if let (Some(h), Some(seq)) = (s.hit.as_ref(), s.buffer_seq) {
                        match max_end {
                            Some((cur, _)) if h.end_abs <= cur => {}
                            _ => max_end = Some((h.end_abs, seq)),
                        }
                    }
                }

                // 4. All slots done?
                if slots.iter().all(|s| s.hit.is_some()) {
                    let (final_end, advance_seq) =
                        max_end.expect("all slots matched -> max_end set");
                    self.pty.output_buf.drain_to(final_end).await;
                    self.log.emit_multimatch_done(
                        mm_span_id,
                        &shell,
                        &self.shell_marker,
                        advance_seq,
                        Some(span),
                    );
                    return Ok::<(), ExecError>(());
                }

                // 5. Wait for the buffer to grow or cancellation.
                tokio::select! {
                    _ = notified => {}
                    _ = self.cancel.cancelled() => {
                        return Err(self.observed_cancel(Some(span.clone())).await);
                    }
                }
            }
        };

        match tokio::time::timeout(dur, fut).await {
            Ok(result) => result,
            Err(_) => {
                let unmatched: Vec<usize> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.hit.is_none())
                    .map(|(i, _)| i)
                    .collect();
                let matched: Vec<usize> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.hit.is_some())
                    .map(|(i, _)| i)
                    .collect();
                self.log.emit_multimatch_timeout(
                    mm_span_id,
                    &shell,
                    &self.shell_marker,
                    &unmatched,
                    Some(span),
                );
                let pattern_meta: Vec<crate::observe::structured::MultiMatchPattern> = slots
                    .iter()
                    .map(|m| crate::observe::structured::MultiMatchPattern {
                        pattern: m.source.clone(),
                        is_regex: m.is_regex,
                    })
                    .collect();
                let context = self.capture_failure_context().await;
                Err(Failure::MultiMatch {
                    shell: self.ctx.current_name().to_string(),
                    patterns: pattern_meta,
                    matched,
                    span: span.clone(),
                    effective: Box::new(timeout.clone()),
                    context,
                }
                .into())
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
    async fn eval_expr(&mut self, expr: &IrExpr) -> Result<String, ExecError> {
        use relux_ir::IrNode;
        let span = expr.span().clone();
        self.check_fail(span.clone()).await?;
        match expr {
            IrExpr::String { value, .. } => {
                let result = self.render_interp(value, Some(&span)).await;
                let shell = self.ctx.current_name();
                self.log.emit_string_eval(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    &result,
                    Some(&span),
                );
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

    async fn eval_call(&mut self, call: &IrCallExpr, span: &IrSpan) -> Result<String, ExecError> {
        let fn_id = call.resolved().clone();
        let fn_name = call.name().name().to_string();

        // Evaluate args first
        let mut evaluated_args = Vec::with_capacity(call.args().len());
        for arg in call.args() {
            evaluated_args.push(self.eval_expr(arg).await?);
        }

        // Try user-defined function
        if let Some(result) = self.tables.fns.get(&fn_id) {
            let ir_fn = match result.as_ref() {
                Ok(f) => f,
                Err(e) => {
                    let context = self.capture_failure_context().await;
                    return Err(Failure::Runtime {
                        message: format!("function resolution failed: {e:?}"),
                        span: span.clone(),
                        shell: Some(self.ctx.current_name().to_string()),
                        context,
                    }
                    .into());
                }
            };
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
                    let fn_guard = self.log.open_span(
                        SpanKind::FnCall {
                            name: fn_name.clone(),
                            args: named_args.clone(),
                            result: None,
                            callee_kind: FnCallKind::User,
                            is_pure: false,
                        },
                        Some(parent_span),
                        Some(span),
                    );
                    self.ctx.push_span(fn_guard.id());
                    self.ctx
                        .push_call(fn_name.clone(), named_args.into_iter().collect());
                    self.log.push_fn_enter(&fn_name);
                    let mut last = String::new();
                    for stmt in &body {
                        match self.exec_stmt(stmt).await {
                            Ok(v) => last = v,
                            Err(e) => {
                                self.ctx.pop_call();
                                self.ctx.pop_span();
                                self.log.push_fn_exit();
                                return Err(e);
                            }
                        }
                    }
                    self.ctx.pop_call();
                    self.ctx.pop_span();
                    self.log.set_fn_call_result(fn_guard.id(), &last);
                    self.log.push_fn_exit();
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
                        let fn_guard = self.log.open_span(
                            SpanKind::FnCall {
                                name: fn_name.clone(),
                                args: positional_args,
                                result: None,
                                callee_kind: FnCallKind::Bif,
                                is_pure: false,
                            },
                            Some(parent_span),
                            Some(span),
                        );
                        self.ctx.push_span(fn_guard.id());
                        self.log.push_fn_enter(&fn_name);
                        let result = bif.call(self, evaluated_args, span).await;
                        self.ctx.pop_span();
                        if let Ok(ref v) = result {
                            self.log.set_fn_call_result(fn_guard.id(), v);
                        }
                        self.log.push_fn_exit();
                        return result;
                    }
                }
            }
        }

        // Try pure function
        if let Some(result) = self.tables.pure_fns.get(&fn_id) {
            let ir_fn = match result.as_ref() {
                Ok(f) => f,
                Err(e) => {
                    let context = self.capture_failure_context().await;
                    return Err(Failure::Runtime {
                        message: format!("pure function resolution failed: {e:?}"),
                        span: span.clone(),
                        shell: Some(self.ctx.current_name().to_string()),
                        context,
                    }
                    .into());
                }
            };
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
            let callee_kind = match ir_fn {
                IrPureFn::UserDefined { .. } => FnCallKind::User,
                IrPureFn::Builtin { .. } => FnCallKind::Bif,
            };
            let parent_span = self.current_span();
            let fn_guard = self.log.open_span(
                SpanKind::FnCall {
                    name: fn_name.clone(),
                    args: named_args,
                    result: None,
                    callee_kind,
                    is_pure: true,
                },
                Some(parent_span),
                Some(span),
            );
            self.ctx.push_span(fn_guard.id());
            self.log.push_fn_enter(&fn_name);
            let mut sink = crate::observe::structured::log_sink::LogSink::new_in_shell(
                &self.log,
                fn_guard.id(),
                self.ctx.current_name(),
                self.shell_marker.clone(),
            );
            let return_value = match relux_ir::evaluator::eval_pure_fn(
                ir_fn,
                evaluated_args,
                &self.ctx.env,
                &self.tables.pure_fns,
                &mut sink,
            ) {
                Ok(v) => v,
                Err(err) => {
                    // Resolve the call chain from the innermost still-open
                    // pure-fn span so nested pure-fn frames (descendants of
                    // this shell-fn span) render in the failure. When no pure
                    // fn is open - a direct match in this fn's own body -
                    // resolve from the enclosing `FnCall` span instead, so the
                    // frame is not lost. Branching on the span source directly
                    // avoids re-deriving `None` from an empty stack.
                    let call_stack = match sink.deepest_open_span() {
                        Some(leaf) => self.log.resolve_stack(leaf),
                        None => self.log.resolve_stack(self.current_span()),
                    };
                    // Build the match context from the innermost frame before
                    // `capture_failure_context_with_stack` consumes `call_stack`.
                    let match_context = MatchContext::Fn {
                        name: call_stack
                            .last()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_else(|| self.ctx.current_name()),
                    };
                    // Dropping the sink closes those spans (tighter `end_ts`)
                    // before the context snapshots the current seq; the spans
                    // stay in the map, so the resolution above still holds.
                    drop(sink);
                    let context = self.capture_failure_context_with_stack(call_stack).await;
                    self.ctx.pop_span();
                    self.log.push_fn_exit();
                    return Err(Failure::from_pure_eval(err, match_context, context).into());
                }
            };
            self.ctx.pop_span();
            self.log.set_fn_call_result(fn_guard.id(), &return_value);
            self.log.push_fn_exit();
            return Ok(return_value);
        }

        let context = self.capture_failure_context().await;
        Err(Failure::Runtime {
            message: format!(
                "undefined function `{}` with arity {}",
                fn_name,
                call.args().len()
            ),
            span: span.clone(),
            shell: Some(self.ctx.current_name().to_string()),
            context,
        }
        .into())
    }

    // --- Public methods for BIFs -------------------------

    pub async fn match_literal(
        &mut self,
        pattern: &str,
        span: &IrSpan,
    ) -> Result<String, ExecError> {
        let shell = self.ctx.current_name();
        let timeout = self.ctx.timeout().clone();
        self.log.emit_match_start(
            self.current_span(),
            &shell,
            &self.shell_marker,
            pattern,
            false,
            &timeout,
            Some(span),
        );
        let match_start = Instant::now();
        let (mat, buffer_seq) = self
            .wait_consume_literal(pattern, &timeout, span.clone())
            .await?;
        let shell = self.ctx.current_name();
        self.log.emit_match_done_record(
            self.current_span(),
            &shell,
            &self.shell_marker,
            &mat.value.0,
            match_start.elapsed(),
            None,
            buffer_seq,
            Some(span),
        );
        Ok(pattern.to_string())
    }

    pub async fn send_line(&mut self, line: &str, span: &IrSpan) -> Result<(), ExecError> {
        let shell = self.ctx.current_name();
        self.log.emit_send(
            self.current_span(),
            &shell,
            &self.shell_marker,
            line,
            Some(span),
        );
        self.send_bytes(format!("{line}\n").as_bytes(), span.clone())
            .await?;
        Ok(())
    }

    pub async fn send_raw(&mut self, data: &[u8], span: &IrSpan) -> Result<(), ExecError> {
        let display = data
            .iter()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>();
        let shell = self.ctx.current_name();
        self.log.emit_send(
            self.current_span(),
            &shell,
            &self.shell_marker,
            &display,
            Some(span),
        );
        self.send_bytes(data, span.clone()).await?;
        Ok(())
    }

    // --- Wait + consume/peek helpers ---------------------

    async fn wait_consume_literal(
        &self,
        pattern: &str,
        timeout: &IrTimeout,
        span: IrSpan,
    ) -> Result<(buffer::Match<buffer::LiteralMatch>, EventSeq), ExecError> {
        let dur = timeout.adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
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
                        return Ok::<(buffer::Match<buffer::LiteralMatch>, EventSeq), ExecError>(
                            result,
                        );
                    }
                    Ok(None) => {}
                }
                tokio::select! {
                    _ = notified => {}
                    _ = self.cancel.cancelled() => {
                        return Err(self.observed_cancel(Some(span.clone())).await);
                    }
                }
            }
        };

        match tokio::time::timeout(dur, fut).await {
            Ok(result) => result,
            Err(_) => {
                let shell = self.ctx.current_name();
                self.log.emit_timeout(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    pattern,
                    timeout,
                    Some(&span),
                );
                let context = self.capture_failure_context().await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.ctx.current_name().to_string(),
                    effective: Box::new(timeout.clone()),
                    context,
                }
                .into())
            }
        }
    }

    async fn wait_consume_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: &IrTimeout,
        span: IrSpan,
    ) -> Result<(buffer::Match<buffer::RegexMatch>, EventSeq), ExecError> {
        let dur = timeout.adjusted_duration_with_flaky(self.flaky_timeout_multiplier);
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
                        return Ok::<(buffer::Match<buffer::RegexMatch>, EventSeq), ExecError>(
                            result,
                        );
                    }
                    Ok(None) => {}
                }
                tokio::select! {
                    _ = notified => {}
                    _ = self.cancel.cancelled() => {
                        return Err(self.observed_cancel(Some(span.clone())).await);
                    }
                }
            }
        };

        match tokio::time::timeout(dur, fut).await {
            Ok(result) => result,
            Err(_) => {
                let shell = self.ctx.current_name();
                self.log.emit_timeout(
                    self.current_span(),
                    &shell,
                    &self.shell_marker,
                    pattern,
                    timeout,
                    Some(&span),
                );
                let context = self.capture_failure_context().await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.ctx.current_name().to_string(),
                    effective: Box::new(timeout.clone()),
                    context,
                }
                .into())
            }
        }
    }

    async fn check_fail(&self, span: IrSpan) -> Result<(), ExecError> {
        let fail_pat = self.ctx.fail_pattern();
        if let Some(hit) = self.pty.output_buf.check_fail_pattern(fail_pat).await {
            return Err(self.make_fail_pattern_error(hit, span).await);
        }
        Ok(())
    }

    async fn make_fail_pattern_error(&self, hit: FailPatternHit, span: IrSpan) -> ExecError {
        let shell = self.ctx.current_name();
        self.log.emit_fail_pattern_triggered(
            self.current_span(),
            &shell,
            &self.shell_marker,
            &hit.pattern,
            hit.is_regex,
            &hit.matched_text,
            Some(&span),
        );
        let context = self.capture_failure_context().await;
        Failure::FailPatternMatched {
            pattern: hit.pattern,
            matched_line: hit.matched_text,
            span,
            shell: self.ctx.current_name().to_string(),
            context,
        }
        .into()
    }

    async fn send_bytes(&mut self, data: &[u8], span: IrSpan) -> Result<(), ExecError> {
        match self.pty.send_bytes(data).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let context = self.capture_failure_context().await;
                Err(Failure::ShellExited {
                    shell: self.ctx.current_name().to_string(),
                    exit_code: e.raw_os_error(),
                    span,
                    context,
                }
                .into())
            }
        }
    }

    pub async fn shutdown(&mut self) {
        // Idempotent: the same VM Arc is reachable from multiple cleanup
        // paths (test-level shells map, owning effect's handle, and
        // recursively from the dependency effect's handle), and each path
        // dedups by Arc pointer only within itself. Without this guard the
        // same shell would emit shell-terminate up to N times.
        if self.terminated {
            return;
        }
        self.terminated = true;
        let shell = self.ctx.current_name();
        self.log
            .emit_shell_terminate(self.current_span(), &shell, &self.shell_marker, None);
        self.pty.shutdown().await;
    }
}
