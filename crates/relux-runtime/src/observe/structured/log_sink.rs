//! Bridge between the IR's `PureEvalSink` trait and the runtime's
//! `StructuredLogBuilder`. Used at three sites:
//!
//!   1. Test-/effect-level let / overlay evaluation - the sink opens
//!      pure FnCall spans and emits Interpolation events under the
//!      enclosing test or setup span.
//!   2. Marker replay - the sink lays down the buffered
//!      `MarkerRecording::ops` under a per-marker `marker-eval` span.
//!   3. Pure-match statements running inside a `shell` block (and pure-fn
//!      calls made from one) - constructed via [`LogSink::new_in_shell`] so
//!      the pure-match events carry the enclosing shell name / marker,
//!      satisfying the schema invariant that `shell` / `shell_marker` are
//!      present iff a shell was in scope at the emit site.
//!
//! Sites 1 and 2 are shell-less and use [`LogSink::new`]. The sink owns a
//! stack of `SpanGuard`s so nested pure-fn calls parent correctly; the root
//! parent is supplied at construction.

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
    /// Enclosing shell `(name, marker)` when the sink runs inside a `shell`
    /// block; `None` for the shell-less sites (preambles, marker replay).
    /// Only the pure-match events carry it - the other pure emitters
    /// (interpolation, var-read) stay shell-agnostic as before.
    shell: Option<(String, String)>,
    stack: Vec<SpanGuard>,
}

impl<'a> LogSink<'a> {
    pub fn new(log: &'a StructuredLogBuilder, root_parent: SpanId) -> Self {
        Self {
            log,
            root_parent,
            shell: None,
            stack: Vec::new(),
        }
    }

    /// Like [`Self::new`], but tags shell-bound pure-match events with the
    /// enclosing shell name / marker. Use when a pure-match statement (or a
    /// pure-fn call) runs inside a `shell` block, so its events satisfy the
    /// events.json invariant that `shell` / `shell_marker` are present iff a
    /// shell was in scope at the emit site.
    pub fn new_in_shell(
        log: &'a StructuredLogBuilder,
        root_parent: SpanId,
        shell: String,
        marker: String,
    ) -> Self {
        Self {
            log,
            root_parent,
            shell: Some((shell, marker)),
            stack: Vec::new(),
        }
    }

    fn shell_ctx(&self) -> (Option<&str>, Option<&str>) {
        match &self.shell {
            Some((shell, marker)) => (Some(shell.as_str()), Some(marker.as_str())),
            None => (None, None),
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
            .emit_interpolation(parent, None, None, template, result, bindings, Some(span));
    }

    fn record_pure_match_start(
        &mut self,
        value: &str,
        pattern: &str,
        is_regex: bool,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        let (shell, marker) = self.shell_ctx();
        self.log
            .emit_pure_match_start(parent, shell, marker, value, pattern, is_regex, Some(span));
    }

    fn record_pure_match_done(
        &mut self,
        matched: &str,
        captures: &HashMap<String, String>,
        span: &IrSpan,
    ) {
        let parent = self.current_parent();
        let (shell, marker) = self.shell_ctx();
        self.log
            .emit_pure_match_done(parent, shell, marker, matched, captures, Some(span));
    }

    fn record_pure_match_failed(&mut self, span: &IrSpan) {
        let parent = self.current_parent();
        let (shell, marker) = self.shell_ctx();
        self.log
            .emit_pure_match_failed(parent, shell, marker, Some(span));
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

    fn build_events(log: StructuredLogBuilder) -> Vec<crate::observe::structured::event::Event> {
        use crate::observe::structured::EnvInfo;
        use crate::observe::structured::TestInfo;
        use crate::observe::structured::TestOutcome;
        log.build(
            TestInfo {
                name: "t".into(),
                path: "t".into(),
                duration_ms: 0,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        )
        .events
    }

    // Regression: a pure-match statement inside a `shell` block must tag its
    // events with the enclosing shell, per the events.json invariant that
    // `shell`/`shell_marker` are present iff a shell was in scope.
    #[test]
    fn shell_bound_pure_match_events_carry_the_shell() {
        use crate::observe::structured::event::EventKind;

        let log = make_builder();
        let root = log.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let mut sink = LogSink::new_in_shell(&log, root.id(), "sh".into(), "sh#1".into());
        sink.record_pure_match_start("abc", "abc", false, &IrSpan::synthetic());
        sink.record_pure_match_done("abc", &HashMap::new(), &IrSpan::synthetic());
        sink.record_pure_match_failed(&IrSpan::synthetic());
        drop(root);

        let matched = build_events(log)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::PureMatchStart { .. }
                        | EventKind::PureMatchDone { .. }
                        | EventKind::PureMatchFailed { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(matched.len(), 3);
        for ev in matched {
            assert_eq!(ev.shell.as_deref(), Some("sh"));
            assert_eq!(ev.shell_marker.as_deref(), Some("sh#1"));
        }
    }

    // The shell-less sites (marker replay, preambles) still emit pure-match
    // events with no shell, as before.
    #[test]
    fn shell_less_pure_match_events_omit_the_shell() {
        use crate::observe::structured::event::EventKind;

        let log = make_builder();
        let root = log.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let mut sink = LogSink::new(&log, root.id());
        sink.record_pure_match_start("abc", "abc", false, &IrSpan::synthetic());
        sink.record_pure_match_failed(&IrSpan::synthetic());
        drop(root);

        let matched = build_events(log)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::PureMatchStart { .. } | EventKind::PureMatchFailed { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(matched.len(), 2);
        for ev in matched {
            assert_eq!(ev.shell, None);
            assert_eq!(ev.shell_marker, None);
        }
    }
}
