//! Bridge between the IR's `PureEvalSink` trait and the runtime's
//! `StructuredLogBuilder`. Used at two sites:
//!
//!   1. Test-/effect-level let / overlay evaluation — the sink opens
//!      pure FnCall spans and emits Interpolation events under the
//!      enclosing test or setup span.
//!   2. Marker replay — the sink lays down the buffered
//!      `MarkerRecording::ops` under a per-marker `marker-eval` span.
//!
//! The sink owns a stack of `SpanGuard`s so nested pure-fn calls
//! parent correctly; the root parent is supplied at construction.

use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;
use relux_ir::pure_sink::MatchKind as IrMatchKind;
use relux_ir::pure_sink::PureEvalSink;
use relux_ir::pure_sink::SinkOp;

use super::builder::SpanGuard;
use super::builder::StructuredLogBuilder;
use super::span::FnCallKind;
use super::span::MatchKind;
use super::span::SpanId;
use super::span::SpanKind;

pub struct LogSink<'a> {
    log: &'a StructuredLogBuilder,
    root_parent: SpanId,
    stack: Vec<SpanGuard>,
}

impl<'a> LogSink<'a> {
    pub fn new(log: &'a StructuredLogBuilder, root_parent: SpanId) -> Self {
        Self {
            log,
            root_parent,
            stack: Vec::new(),
        }
    }

    fn current_parent(&self) -> SpanId {
        self.stack
            .last()
            .map(SpanGuard::id)
            .unwrap_or(self.root_parent)
    }

    /// Apply a buffered sequence of sink ops, re-emitting them onto
    /// the structured log. Used to replay marker recordings.
    pub fn replay(&mut self, ops: &[SinkOp]) {
        for op in ops {
            match op {
                SinkOp::EnterPureFn {
                    name,
                    args,
                    is_builtin,
                    span,
                } => {
                    self.enter_pure_fn(name, args, *is_builtin, span);
                }
                SinkOp::LeavePureFn { result } => self.leave_pure_fn(result),
                SinkOp::RecordInterpolation {
                    template,
                    result,
                    bindings,
                    span,
                } => {
                    self.record_interpolation(template, result, bindings, span);
                }
                SinkOp::Match {
                    kind,
                    value,
                    pattern,
                    result,
                    captures,
                    span,
                } => {
                    self.record_match(*kind, value, pattern, result, captures, span);
                }
                SinkOp::VarRead { name, value, span } => {
                    self.record_var_read(name, value, span);
                }
            }
        }
    }
}

fn to_runtime_match_kind(k: IrMatchKind) -> MatchKind {
    match k {
        IrMatchKind::Regex => MatchKind::Regex,
        IrMatchKind::Literal => MatchKind::Literal,
    }
}

impl<'a> PureEvalSink for LogSink<'a> {
    fn enter_pure_fn(
        &mut self,
        name: &str,
        args: &[(String, String)],
        is_builtin: bool,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        let kind = SpanKind::FnCall {
            name: name.to_string(),
            args: args.to_vec(),
            result: None,
            callee_kind: if is_builtin {
                FnCallKind::Bif
            } else {
                FnCallKind::User
            },
            is_pure: true,
        };
        let guard = self.log.open_span(kind, Some(parent), Some(span));
        self.stack.push(guard);
    }

    fn leave_pure_fn(&mut self, result: &str) {
        if let Some(guard) = self.stack.pop() {
            self.log.set_fn_call_result(guard.id(), result);
            // guard drops here, closing the span
        }
    }

    fn record_interpolation(
        &mut self,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        self.log
            .emit_pure_interpolation(parent, template, result, bindings, Some(span));
    }

    fn record_match(
        &mut self,
        kind: IrMatchKind,
        value: &str,
        pattern: &str,
        result: &str,
        captures: &HashMap<String, String>,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        self.log.emit_pure_match(
            parent,
            to_runtime_match_kind(kind),
            value,
            pattern,
            result,
            captures,
            Some(span),
        );
    }

    fn record_var_read(&mut self, name: &str, value: &str, span: &IrSpan) {
        let parent = self.current_parent();
        self.log.emit_var_read(parent, name, value, Some(span));
    }
}
