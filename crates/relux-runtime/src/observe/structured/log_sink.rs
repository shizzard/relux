//! Bridge between the IR's `PureEvalSink` trait and the runtime's
//! `StructuredLogBuilder`. Used at two sites:
//!
//!   1. Test-/effect-level let / overlay evaluation - the sink opens
//!      pure FnCall spans and emits Interpolation events under the
//!      enclosing test or setup span.
//!   2. Marker replay - the sink lays down the buffered
//!      `MarkerRecording::ops` under a per-marker `marker-eval` span.
//!
//! The sink owns a stack of `SpanGuard`s so nested pure-fn calls
//! parent correctly; the root parent is supplied at construction.

use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;
use relux_ir::pure_sink::PureEvalSink;
use relux_ir::pure_sink::SinkOp;

use super::builder::SpanGuard;
use super::builder::StructuredLogBuilder;
use super::span::FnCallKind;
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

    /// The innermost still-open pure-fn span, if any pure-fn call is on the
    /// stack. On the error path (`leave_pure_fn` skipped by `?`
    /// propagation) the nested pure-fn spans stay on the stack, so the
    /// failure boundary resolves the call chain from this leaf - the
    /// nested frames are descendants of the root parent and would not
    /// otherwise appear when resolving from the enclosing span.
    pub fn deepest_open_span(&self) -> Option<SpanId> {
        self.stack.last().map(SpanGuard::id)
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
                SinkOp::PureMatchStart {
                    value,
                    pattern,
                    is_regex,
                    span,
                } => {
                    self.record_pure_match_start(value, pattern, *is_regex, span);
                }
                SinkOp::PureMatchDone {
                    matched,
                    captures,
                    span,
                } => {
                    self.record_pure_match_done(matched, captures, span);
                }
                SinkOp::PureMatchFailed { span } => {
                    self.record_pure_match_failed(span);
                }
                SinkOp::VarRead { name, value, span } => {
                    self.record_var_read(name, value, span);
                }
            }
        }
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

    fn record_pure_match_start(
        &mut self,
        value: &str,
        pattern: &str,
        is_regex: bool,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        self.log
            .emit_pure_match_start(parent, value, pattern, is_regex, Some(span));
    }

    fn record_pure_match_done(
        &mut self,
        matched: &str,
        captures: &HashMap<String, String>,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        self.log
            .emit_pure_match_done(parent, matched, captures, Some(span));
    }

    fn record_pure_match_failed(&mut self, span: &IrSpan) {
        let parent = self.current_parent();
        self.log.emit_pure_match_failed(parent, Some(span));
    }

    fn record_var_read(&mut self, name: &str, value: &str, span: &IrSpan) {
        let parent = self.current_parent();
        self.log.emit_var_read(parent, name, value, Some(span));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::progress;
    use crate::observe::structured::span::SpanKind;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    fn make_builder() -> StructuredLogBuilder {
        let (tx, _rx) = progress::channel();
        let sources = relux_core::table::SharedTable::new();
        StructuredLogBuilder::new(
            tx,
            Instant::now(),
            sources,
            Arc::from(PathBuf::from("/project").as_path()),
        )
    }

    #[test]
    fn deepest_open_span_tracks_the_innermost_unbalanced_pure_fn() {
        let log = make_builder();
        let root = log.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let mut sink = LogSink::new(&log, root.id());
        assert_eq!(sink.deepest_open_span(), None);

        // enter outer, then inner; on the error path `leave_pure_fn` is
        // skipped, so the innermost frame stays and is the resolve leaf.
        sink.enter_pure_fn("outer", &[], false, &IrSpan::synthetic());
        let outer = sink.deepest_open_span().expect("outer open");
        sink.enter_pure_fn("inner", &[], false, &IrSpan::synthetic());
        let inner = sink.deepest_open_span().expect("inner open");
        assert_ne!(outer, inner);

        // Resolving from the innermost span surfaces both pure-fn frames
        // under the test root, top-down.
        let frames = log.resolve_stack(inner);
        let rendered: Vec<(&str, Option<&str>)> = frames
            .iter()
            .map(|f| (f.kind.as_str(), f.name.as_deref()))
            .collect();
        assert_eq!(
            rendered,
            vec![
                ("test", Some("t")),
                ("pure-fn-call", Some("outer")),
                ("pure-fn-call", Some("inner")),
            ]
        );

        // Balanced leave pops back to the outer frame, then to none.
        sink.leave_pure_fn("");
        assert_eq!(sink.deepest_open_span(), Some(outer));
        sink.leave_pure_fn("");
        assert_eq!(sink.deepest_open_span(), None);
    }
}
