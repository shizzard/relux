use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use relux_core::diagnostics::IrSpan;
use relux_core::table::SourceTable;
use relux_ir::IrTimeout;

use super::ArtifactEntry;
use super::EnvInfo;
use super::SourceLocation;
use super::StructuredLog;
use super::TestInfo;
use super::TestOutcome;
use super::buffer::BufferEvent;
use super::buffer::BufferEventKind;
use super::event::Event;
use super::event::EventKind;
use super::event::EventSeq;
use super::event::TimeoutValue;
use super::failure::FailureRecord;
use super::failure::StackFrame;
use super::shell::ShellRecord;
use super::span::Span;
use super::span::SpanId;
use super::span::SpanKind;
use crate::observe::progress::ProgressEvent;
use crate::observe::progress::ProgressTx;

/// Concurrent accumulator for `StructuredLog`. Cheap to `Clone` (the storage
/// is `Arc`-shared); the runtime threads it through `RuntimeContext` and
/// every emission site forwards through it.
#[derive(Clone)]
pub struct StructuredLogBuilder {
    inner: Arc<Mutex<BuilderInner>>,
    test_start: Instant,
    sources: SourceTable,
    project_root: Arc<Path>,
    progress_tx: ProgressTx,
}

/// RAII handle for a span. `Drop` calls `close_span_inner` on the underlying
/// builder, so `?` early-returns are safe — the span always gets an `end_ts`.
/// Use `id()` to obtain the `SpanId` for emissions and as a parent of
/// child spans. Use `close()` to close explicitly (gives a tighter `end_ts`
/// than waiting for drop, useful right before `build()`).
pub struct SpanGuard {
    id: Option<SpanId>,
    log: StructuredLogBuilder,
}

impl SpanGuard {
    pub fn id(&self) -> SpanId {
        self.id.expect("span guard already closed")
    }

    pub fn close(mut self) {
        if let Some(id) = self.id.take() {
            self.log.close_span_inner(id);
        }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.log.close_span_inner(id);
        }
    }
}

struct BuilderInner {
    next_seq: EventSeq,
    next_span_id: SpanId,
    spans: HashMap<SpanId, Span>,
    events: Vec<Event>,
    buffer_events: Vec<BufferEvent>,
    shells: HashMap<String, ShellRecord>,
}

impl StructuredLogBuilder {
    pub fn new(
        progress_tx: ProgressTx,
        test_start: Instant,
        sources: SourceTable,
        project_root: Arc<Path>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BuilderInner {
                next_seq: 0,
                next_span_id: 0,
                spans: HashMap::new(),
                events: Vec::new(),
                buffer_events: Vec::new(),
                shells: HashMap::new(),
            })),
            test_start,
            sources,
            project_root,
            progress_tx,
        }
    }

    fn now(&self) -> Duration {
        self.test_start.elapsed()
    }

    fn resolve_location(&self, span: &IrSpan) -> Option<SourceLocation> {
        let file_id = span.file();
        let source_file = self.sources.get(file_id)?;
        let line = source_file.line_at(span.span().start());
        let rel_path = source_file
            .path
            .strip_prefix(&*self.project_root)
            .unwrap_or(&source_file.path);
        Some(SourceLocation {
            file: rel_path.display().to_string(),
            line,
            start: span.span().start(),
            end: span.span().end(),
        })
    }

    fn timeout_value(&self, t: &IrTimeout) -> TimeoutValue {
        match t {
            IrTimeout::Tolerance {
                duration,
                multiplier,
                span,
            } => TimeoutValue::Tolerance {
                duration: humantime::format_duration(*duration).to_string(),
                multiplier: format_multiplier(*multiplier),
                total_duration: humantime::format_duration(t.adjusted_duration()).to_string(),
                source: self.resolve_location(span),
            },
            IrTimeout::Assertion { duration, span } => TimeoutValue::Assertion {
                duration: humantime::format_duration(*duration).to_string(),
                source: self.resolve_location(span),
            },
        }
    }

    fn push_progress(&self, event: ProgressEvent) {
        let _ = self.progress_tx.send(event);
    }

    // ─── Span lifecycle ───────────────────────────────────────────

    /// Open a span and return a guard that closes it on drop. The caller
    /// must keep the guard alive for the span's lifetime; passing the id
    /// (`guard.id()`) to children is fine. Drop on `?` propagation closes
    /// cleanly; for a tighter `end_ts`, use `SpanGuard::close()` explicitly.
    pub fn open_span(
        &self,
        kind: SpanKind,
        parent: Option<SpanId>,
        location: Option<&IrSpan>,
    ) -> SpanGuard {
        let location = location.and_then(|s| self.resolve_location(s));
        let start_ts = self.now();
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.next_span_id;
            inner.next_span_id += 1;
            inner.spans.insert(
                id,
                Span {
                    id,
                    kind,
                    parent,
                    start_ts,
                    end_ts: None,
                    location,
                },
            );
            id
        };
        SpanGuard {
            id: Some(id),
            log: self.clone(),
        }
    }

    fn close_span_inner(&self, id: SpanId) {
        let end_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(span) = inner.spans.get_mut(&id) {
            span.end_ts = Some(end_ts);
        }
    }

    /// Attach a return value to an in-flight `FnCall` span. Called from
    /// `exec_call` on the success path before the span closes; failed calls
    /// leave `result` as `None` so the row title falls back to `name/arity`.
    pub fn set_fn_call_result(&self, id: SpanId, result: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(span) = inner.spans.get_mut(&id)
            && let SpanKind::FnCall { result: slot, .. } = &mut span.kind
        {
            *slot = Some(result.to_string());
        }
    }

    /// Walk parent pointers from `leaf` back to a root span and return the
    /// frames in root-to-leaf order. Used at failure-construction time to
    /// snapshot the active call chain.
    pub fn resolve_stack(&self, leaf: SpanId) -> Vec<StackFrame> {
        let inner = self.inner.lock().unwrap();
        let mut chain: Vec<StackFrame> = Vec::new();
        let mut next = Some(leaf);
        while let Some(id) = next {
            let Some(span) = inner.spans.get(&id) else {
                break;
            };
            let (name, args) = span.kind.frame_data();
            chain.push(StackFrame {
                span: id,
                kind: span.kind.kind_str().to_string(),
                name,
                args,
                alias: span.kind.frame_alias(),
                location: span.location.clone(),
            });
            next = span.parent;
        }
        chain.reverse();
        chain
    }

    /// Latest emitted seq, or `0` if no event has fired yet. Failures use
    /// this to point the structured-log artifact at the most recent event
    /// (typically a `Timeout` or `FailPatternTriggered`).
    pub fn current_seq(&self) -> EventSeq {
        let inner = self.inner.lock().unwrap();
        inner.next_seq.saturating_sub(1)
    }

    // ─── Raw event/buffer-event push ──────────────────────────────

    /// Test-only: a snapshot of the accumulated buffer events, in the
    /// order they were pushed.
    #[cfg(test)]
    pub(crate) fn buffer_events_for_tests(&self) -> Vec<BufferEvent> {
        self.inner.lock().unwrap().buffer_events.clone()
    }

    #[cfg(test)]
    pub(crate) fn sources_for_tests(&self) -> &relux_core::table::SourceTable {
        &self.sources
    }

    #[cfg(test)]
    pub(crate) fn resolve_location_for_tests(&self, span: &IrSpan) -> Option<SourceLocation> {
        self.resolve_location(span)
    }

    pub fn push_event(
        &self,
        span: SpanId,
        shell: Option<&str>,
        shell_marker: Option<&str>,
        location: Option<&IrSpan>,
        kind: EventKind,
    ) -> EventSeq {
        let source = location.and_then(|s| self.resolve_location(s));
        let ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.events.push(Event {
            seq,
            ts,
            span,
            shell: shell.map(String::from),
            shell_marker: shell_marker.map(String::from),
            source,
            kind,
        });
        seq
    }

    pub fn push_buffer_event(
        &self,
        shell: &str,
        shell_marker: &str,
        kind: BufferEventKind,
    ) -> EventSeq {
        let ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.buffer_events.push(BufferEvent {
            seq,
            ts,
            shell: shell.to_string(),
            shell_marker: shell_marker.to_string(),
            kind,
        });
        seq
    }

    // ─── Shells glossary ──────────────────────────────────────────

    pub fn record_shell_spawn(&self, marker: &str, name: &str, command: &str) {
        let spawn_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        inner.shells.insert(
            marker.to_string(),
            ShellRecord {
                marker: marker.to_string(),
                name: name.to_string(),
                spawn_ts,
                terminate_ts: None,
                command: command.to_string(),
            },
        );
    }

    pub fn record_shell_terminate(&self, marker: &str) {
        let terminate_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(rec) = inner.shells.get_mut(marker) {
            rec.terminate_ts = Some(terminate_ts);
        }
    }

    // ─── Convenience emitters (mirror EventSink shape) ────────────

    // Shell lifecycle ---------------------------------------------------

    pub fn emit_shell_spawn(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        command: &str,
        location: Option<&IrSpan>,
    ) {
        self.record_shell_spawn(marker, shell, command);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellSpawn {
                name: shell.to_string(),
                command: command.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSpawn);
    }

    pub fn emit_shell_ready(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellReady {
                name: shell.to_string(),
            },
        );
    }

    pub fn emit_shell_switch(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellSwitch {
                name: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSwitch(shell.to_string()));
    }

    pub fn emit_shell_terminate(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.record_shell_terminate(marker);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellTerminate {
                name: shell.to_string(),
            },
        );
    }

    // Effect exposes ----------------------------------------------------

    pub fn emit_effect_expose_shell(
        &self,
        span: SpanId,
        name: &str,
        target: &str,
        qualifier: Option<&str>,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::EffectExposeShell {
                name: name.to_string(),
                target: target.to_string(),
                qualifier: qualifier.map(String::from),
            },
        );
    }

    pub fn emit_effect_expose_var(
        &self,
        span: SpanId,
        name: &str,
        target: &str,
        qualifier: Option<&str>,
        value: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::EffectExposeVar {
                name: name.to_string(),
                target: target.to_string(),
                qualifier: qualifier.map(String::from),
                value: value.to_string(),
            },
        );
    }

    // I/O ---------------------------------------------------------------

    pub fn emit_send(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        data: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Send {
                data: data.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Send);
    }

    pub fn emit_recv(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        data: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Recv {
                data: data.to_string(),
            },
        );
    }

    // Matching ----------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn emit_match_start(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        effective: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let effective = self.timeout_value(effective);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MatchStart {
                pattern: pattern.to_string(),
                is_regex,
                effective,
            },
        );
        self.push_progress(ProgressEvent::MatchStart);
    }

    /// Record a structured `MatchDone` event referencing a buffer event
    /// that was pushed (atomically with the consume operation) by
    /// `OutputBuffer::consume_*`. The buffer event push is the consumer's
    /// responsibility — this method only emits the structured event +
    /// progress notification.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_match_done_record(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        matched: &str,
        elapsed: Duration,
        captures: Option<HashMap<String, String>>,
        buffer_seq: EventSeq,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MatchDone {
                matched: matched.to_string(),
                elapsed,
                captures,
                buffer_seq,
            },
        );
        self.push_progress(ProgressEvent::MatchDone);
    }

    pub fn emit_timeout(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        effective: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let effective = self.timeout_value(effective);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Timeout {
                pattern: pattern.to_string(),
                buffer_seq: None,
                effective,
            },
        );
        self.push_progress(ProgressEvent::Timeout);
    }

    // Fail patterns -----------------------------------------------------

    pub fn emit_fail_pattern_set(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::FailPatternSet {
                pattern: pattern.to_string(),
                is_regex,
            },
        );
    }

    pub fn emit_fail_pattern_cleared(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::FailPatternCleared,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_fail_pattern_triggered(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        matched_line: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::FailPatternTriggered {
                pattern: pattern.to_string(),
                is_regex,
                matched_line: matched_line.to_string(),
                buffer_seq: None,
            },
        );
        self.push_progress(ProgressEvent::FailPattern);
    }

    // Control flow ------------------------------------------------------

    pub fn emit_sleep_start(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        duration: Duration,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::SleepStart { duration },
        );
        self.push_progress(ProgressEvent::SleepStart);
    }

    pub fn emit_sleep_done(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::SleepDone,
        );
        self.push_progress(ProgressEvent::SleepDone);
    }

    pub fn emit_timeout_set(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        timeout: &IrTimeout,
        previous: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let timeout = self.timeout_value(timeout);
        let previous = self.timeout_value(previous);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::TimeoutSet { timeout, previous },
        );
    }

    // Values ------------------------------------------------------------

    pub fn emit_var_let(
        &self,
        span: SpanId,
        shell: Option<&str>,
        marker: Option<&str>,
        name: &str,
        value: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            shell,
            marker,
            location,
            EventKind::VarLet {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_var_assign(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        name: &str,
        value: &str,
        previous: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::VarAssign {
                name: name.to_string(),
                value: value.to_string(),
                previous: previous.to_string(),
            },
        );
    }

    pub fn emit_string_eval(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        result: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::StringEval {
                result: result.to_string(),
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_interpolation(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Interpolation {
                template: template.to_string(),
                result: result.to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    /// Interpolation event emitted from a pure-eval context (no shell,
    /// no shell marker). Used by `LogSink` for marker replay and for
    /// test/effect-level lets.
    pub fn emit_pure_interpolation(
        &self,
        span: SpanId,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::Interpolation {
                template: template.to_string(),
                result: result.to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    /// Pure variable-read event. Used by `LogSink` to surface bare
    /// `${X}`-style reads that resolve against scope/env. The result
    /// is the resolved string (`""` when the var is undefined).
    pub fn emit_var_read(&self, span: SpanId, name: &str, value: &str, location: Option<&IrSpan>) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::VarRead {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    /// Pure string-match event. Used by `LogSink` for marker `?`
    /// regex conditions and the future runtime string-match syntax.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_pure_match(
        &self,
        span: SpanId,
        match_kind: super::span::MatchKind,
        value: &str,
        pattern: &str,
        result: &str,
        captures: &HashMap<String, String>,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::PureMatch {
                match_kind,
                value: value.to_string(),
                pattern: pattern.to_string(),
                result: result.to_string(),
                captures: captures.clone(),
            },
        );
    }

    /// Open the synthetic `markers` root span. Always opened (per
    /// design); viewer filters out empty markers roots.
    pub fn open_markers_span(&self, location: Option<&IrSpan>) -> SpanGuard {
        self.open_span(SpanKind::Markers, None, location)
    }

    /// Open a `marker-eval` span as a child of a `markers` root.
    pub fn open_marker_eval_span(
        &self,
        parent: SpanId,
        marker_kind: super::span::MarkerEvalKind,
        modifier: super::span::MarkerEvalModifier,
        decision: super::span::MarkerEvalDecision,
        location: Option<&IrSpan>,
    ) -> SpanGuard {
        self.open_span(
            SpanKind::MarkerEval {
                marker_kind,
                modifier,
                decision,
            },
            Some(parent),
            location,
        )
    }

    /// Emit the final truthy/falsy outcome event inside a marker-eval
    /// span. Mirrors the shape stored on `MarkerRecording.evaluation`.
    /// Returns the emitted event's `EventSeq` so callers (e.g. `replay_markers`)
    /// can use it as a focus pointer.
    pub fn emit_bool_check(
        &self,
        span: SpanId,
        evaluation: super::span::MarkerEvalDetail,
        location: Option<&IrSpan>,
    ) -> EventSeq {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::BoolCheck { evaluation },
        )
    }

    // Diagnostics -------------------------------------------------------

    pub fn emit_annotate(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        text: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Annotate {
                text: text.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Annotation(text.to_string()));
    }

    pub fn emit_log(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Log {
                message: message.to_string(),
            },
        );
    }

    pub fn emit_warning(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Warning {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Warning(message.to_string()));
    }

    pub fn emit_error(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Error {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Error(message.to_string()));
    }

    /// Emit a `cancelled` event on the span the VM was in when it observed
    /// the cancel token flipping. Carries the reason recorded by whoever
    /// called `cancel_with(...)`. Pushes a `C` sigil into the per-test
    /// progress sliding window so live TUI viewers see the cancel land
    /// in the same place errors and timeouts do.
    pub fn emit_cancelled(
        &self,
        span: SpanId,
        shell: Option<&str>,
        shell_marker: Option<&str>,
        reason: &crate::cancel::CancelReason,
    ) {
        self.push_event(
            span,
            shell,
            shell_marker,
            None,
            EventKind::Cancelled {
                reason: super::event::CancelReasonRecord::from(reason),
            },
        );
        self.push_progress(ProgressEvent::Cancellation);
    }

    /// Push a `Failure` progress notification only. The structured failure
    /// information is carried in the `FailureRecord` passed to `build()`.
    pub fn emit_failure_progress(&self) {
        self.push_progress(ProgressEvent::Failure);
    }

    // ─── Failure record translation ───────────────────────────────

    /// Translate a runtime `Failure` into a `FailureRecord`, lifting the
    /// `FailureContext` captured at failure-construction time into the
    /// structured log artifact. Sites that don't have a VM (effect-resolution
    /// errors, pre-VM init) supply `FailureContext::default()`, which lands as
    /// `span: 0` / `event_seq: 0` / empty stack — the artifact is still
    /// well-formed but lacks call-stack detail for those cases.
    pub fn failure_record(&self, failure: &crate::report::result::Failure) -> FailureRecord {
        use crate::report::result::Failure;
        match failure {
            Failure::MatchTimeout {
                pattern,
                shell,
                effective,
                context,
                ..
            } => FailureRecord::MatchTimeout {
                span: context.span.unwrap_or(0),
                event_seq: context.event_seq.unwrap_or(0),
                shell: shell.clone(),
                pattern: pattern.clone(),
                effective: self.timeout_value(effective),
                call_stack: context.call_stack.clone(),
                buffer_tail: context.buffer_tail.clone(),
                vars_in_scope: context.vars_in_scope.clone(),
            },
            Failure::FailPatternMatched {
                pattern,
                matched_line,
                shell,
                context,
                ..
            } => FailureRecord::FailPatternMatched {
                span: context.span.unwrap_or(0),
                event_seq: context.event_seq.unwrap_or(0),
                shell: shell.clone(),
                pattern: pattern.clone(),
                matched_line: matched_line.clone(),
                call_stack: context.call_stack.clone(),
                buffer_tail: context.buffer_tail.clone(),
                vars_in_scope: context.vars_in_scope.clone(),
            },
            Failure::ShellExited {
                shell,
                exit_code,
                context,
                ..
            } => FailureRecord::ShellExited {
                span: context.span.unwrap_or(0),
                event_seq: context.event_seq.unwrap_or(0),
                shell: shell.clone(),
                exit_code: *exit_code,
                call_stack: context.call_stack.clone(),
                buffer_tail: context.buffer_tail.clone(),
                vars_in_scope: context.vars_in_scope.clone(),
            },
            Failure::Runtime {
                message,
                shell,
                context,
                ..
            } => FailureRecord::Runtime {
                span: context.span,
                event_seq: context.event_seq,
                shell: shell.clone(),
                message: message.clone(),
                call_stack: context.call_stack.clone(),
                vars_in_scope: context.vars_in_scope.clone(),
            },
        }
    }

    /// Translate a runtime `Cancellation` into a `CancellationRecord`.
    pub fn cancellation_record(
        &self,
        c: &crate::report::result::Cancellation,
    ) -> super::CancellationRecord {
        let ctx = &c.context;
        super::CancellationRecord {
            reason: super::event::CancelReasonRecord::from(&c.reason),
            span: ctx.span,
            event_seq: ctx.event_seq,
            shell: None,
            call_stack: ctx.call_stack.clone(),
        }
    }

    // ─── Final assembly ───────────────────────────────────────────

    pub fn build(
        self,
        info: TestInfo,
        env: EnvInfo,
        outcome: TestOutcome,
        artifacts: Vec<ArtifactEntry>,
    ) -> StructuredLog {
        let inner = match Arc::try_unwrap(self.inner) {
            Ok(mutex) => mutex.into_inner().unwrap(),
            Err(arc) => {
                let mut guard = arc.lock().unwrap();
                BuilderInner {
                    next_seq: guard.next_seq,
                    next_span_id: guard.next_span_id,
                    spans: std::mem::take(&mut guard.spans),
                    events: std::mem::take(&mut guard.events),
                    buffer_events: std::mem::take(&mut guard.buffer_events),
                    shells: std::mem::take(&mut guard.shells),
                }
            }
        };

        let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
        for span in inner.spans.values() {
            if let Some(loc) = &span.location {
                referenced.insert(loc.file.clone());
            }
        }
        for ev in &inner.events {
            if let Some(loc) = &ev.source {
                referenced.insert(loc.file.clone());
            }
        }

        let mut sources: HashMap<String, String> = HashMap::new();
        for (_, source_file) in self.sources.as_vec() {
            let rel = source_file
                .path
                .strip_prefix(&*self.project_root)
                .unwrap_or(&source_file.path)
                .display()
                .to_string();
            if referenced.contains(&rel) {
                sources.insert(rel, source_file.source.clone());
            }
        }

        StructuredLog {
            schema_version: super::SCHEMA_VERSION,
            info,
            outcome,
            env,
            shells: inner.shells,
            spans: inner.spans,
            events: inner.events,
            buffer_events: inner.buffer_events,
            sources,
            artifacts,
        }
    }
}

/// Format a tolerance multiplier as a stable string. Whole numbers keep one
/// decimal place (`1.0`, `2.0`), fractional values use default float
/// formatting (`1.5`, `1.25`).
fn format_multiplier(m: f64) -> String {
    if m.fract() == 0.0 {
        format!("{m:.1}")
    } else {
        format!("{m}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::span::FnCallKind;
    use super::*;
    use crate::observe::progress;

    fn make_builder() -> (
        StructuredLogBuilder,
        tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>,
    ) {
        let (tx, rx) = progress::channel();
        let sources = relux_core::table::SharedTable::new();
        let builder = StructuredLogBuilder::new(
            tx,
            Instant::now(),
            sources,
            Arc::from(PathBuf::from("/project").as_path()),
        );
        (builder, rx)
    }

    #[test]
    fn seq_is_monotonic_across_event_and_buffer_pushes() {
        let (b, _rx) = make_builder();
        let test_span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let id = test_span.id();
        let s1 = b.push_event(
            id,
            Some("sh"),
            Some("m"),
            None,
            EventKind::Send { data: "a".into() },
        );
        let s2 = b.push_buffer_event("sh", "m", BufferEventKind::Grew { data: "b".into() });
        let s3 = b.push_event(
            id,
            Some("sh"),
            Some("m"),
            None,
            EventKind::Recv { data: "c".into() },
        );
        assert_eq!(s1, 0);
        assert_eq!(s2, 1);
        assert_eq!(s3, 2);
    }

    #[test]
    fn open_close_span_round_trips() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let id = span.id();
        span.close();
        let inner = b.inner.lock().unwrap();
        let stored = inner.spans.get(&id).unwrap();
        assert!(stored.end_ts.is_some());
        assert!(stored.parent.is_none());
    }

    #[test]
    fn span_guard_closes_on_drop() {
        let (b, _rx) = make_builder();
        let id = {
            let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
            span.id()
            // span drops at end of this block
        };
        let inner = b.inner.lock().unwrap();
        assert!(inner.spans.get(&id).unwrap().end_ts.is_some());
    }

    #[test]
    fn span_guard_explicit_close_then_drop_is_noop() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let id = span.id();
        span.close();
        let end_after_close = b.inner.lock().unwrap().spans.get(&id).unwrap().end_ts;
        assert!(end_after_close.is_some());
        // Drop happened inside `close()` (Option taken). A subsequent peek
        // should show the same end_ts — the guard's drop didn't re-touch it
        // because there's no guard left.
        let end_later = b.inner.lock().unwrap().spans.get(&id).unwrap().end_ts;
        assert_eq!(end_after_close, end_later);
    }

    #[test]
    fn span_ids_are_unique_and_parent_preserved() {
        let (b, _rx) = make_builder();
        let parent = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let parent_id = parent.id();
        let child = b.open_span(
            SpanKind::ShellBlock { shell: "sh".into() },
            Some(parent_id),
            None,
        );
        let child_id = child.id();
        assert_ne!(parent_id, child_id);
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.spans.get(&child_id).unwrap().parent, Some(parent_id));
    }

    #[test]
    fn shell_glossary_records_spawn_and_terminate() {
        let (b, _rx) = make_builder();
        b.record_shell_spawn("test-marker-0000", "default", "/bin/bash");
        b.record_shell_terminate("test-marker-0000");
        let inner = b.inner.lock().unwrap();
        let rec = inner.shells.get("test-marker-0000").unwrap();
        assert_eq!(rec.name, "default");
        assert_eq!(rec.command, "/bin/bash");
        assert!(rec.terminate_ts.is_some());
    }

    #[test]
    fn clone_shares_storage() {
        let (b, _rx) = make_builder();
        let b2 = b.clone();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        b2.push_event(
            span.id(),
            Some("sh"),
            Some("m"),
            None,
            EventKind::Send { data: "x".into() },
        );
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.events.len(), 1);
    }

    #[test]
    fn build_consumes_builder_and_yields_log() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let id = span.id();
        b.push_event(
            id,
            Some("sh"),
            Some("m"),
            None,
            EventKind::Send { data: "x".into() },
        );
        b.push_buffer_event("sh", "m", BufferEventKind::Grew { data: "y".into() });
        span.close();
        let log = b.build(
            TestInfo {
                name: "t".into(),
                path: "t.relux".into(),
                duration_ms: 1,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        );
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.buffer_events.len(), 1);
        assert_eq!(log.spans.len(), 1);
        assert!(matches!(log.outcome, TestOutcome::Pass));
    }

    #[test]
    fn emit_send_pushes_event_and_progress() {
        let (b, mut rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        b.emit_send(span.id(), "sh", "m", "hello", None);
        let inner = b.inner.lock().unwrap();
        assert!(matches!(
            &inner.events.last().unwrap().kind,
            EventKind::Send { data } if data == "hello"
        ));
        drop(inner);
        assert!(matches!(rx.try_recv(), Ok(ProgressEvent::Send)));
    }

    #[test]
    fn emit_match_done_record_pushes_event_with_supplied_buffer_seq() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        // Simulate the buffer event that `OutputBuffer::consume_*` would
        // have pushed atomically with the consume operation.
        let buffer_seq = b.push_buffer_event(
            "sh",
            "m",
            BufferEventKind::Matched {
                before: "before".into(),
                matched: "ok".into(),
                after: "after".into(),
            },
        );
        b.emit_match_done_record(
            span.id(),
            "sh",
            "m",
            "ok",
            Duration::from_millis(5),
            None,
            buffer_seq,
            None,
        );
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.buffer_events.len(), 1);
        let last = inner.events.last().unwrap();
        match &last.kind {
            EventKind::MatchDone {
                buffer_seq: ev_seq, ..
            } => assert_eq!(*ev_seq, buffer_seq),
            _ => panic!("expected MatchDone"),
        }
    }

    #[test]
    fn resolve_stack_walks_parent_chain_root_to_leaf() {
        let (b, _rx) = make_builder();
        let test_span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let test_id = test_span.id();
        let block_span = b.open_span(
            SpanKind::ShellBlock { shell: "sh".into() },
            Some(test_id),
            None,
        );
        let block_id = block_span.id();
        let fn_span = b.open_span(
            SpanKind::FnCall {
                name: "do_thing".into(),
                args: vec![("x".into(), "1".into())],
                result: None,
                callee_kind: FnCallKind::User,
                is_pure: false,
            },
            Some(block_id),
            None,
        );
        let fn_id = fn_span.id();

        let frames = b.resolve_stack(fn_id);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].span, test_id);
        assert_eq!(frames[0].kind, "test");
        assert_eq!(frames[0].name.as_deref(), Some("t"));
        assert_eq!(frames[1].span, block_id);
        assert_eq!(frames[1].kind, "shell-block");
        assert_eq!(frames[1].name.as_deref(), Some("sh"));
        assert_eq!(frames[2].span, fn_id);
        assert_eq!(frames[2].kind, "fn-call");
        assert_eq!(frames[2].name.as_deref(), Some("do_thing"));
        assert_eq!(frames[2].args, vec![("x".into(), "1".into())]);
    }

    #[test]
    fn fn_call_round_trips_callee_kind_and_is_pure() {
        let (b, _rx) = make_builder();
        let test_span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let test_id = test_span.id();
        let fn_span = b.open_span(
            SpanKind::FnCall {
                name: "trim".into(),
                args: vec![("$0".into(), "hi".into())],
                result: None,
                callee_kind: FnCallKind::Bif,
                is_pure: true,
            },
            Some(test_id),
            None,
        );
        let fn_id = fn_span.id();
        fn_span.close();
        test_span.close();

        let inner = b.inner.lock().unwrap();
        let stored = inner.spans.get(&fn_id).unwrap();
        match &stored.kind {
            SpanKind::FnCall {
                callee_kind,
                is_pure,
                ..
            } => {
                assert_eq!(*callee_kind, FnCallKind::Bif);
                assert!(*is_pure);
            }
            _ => panic!("expected FnCall"),
        }
    }

    #[test]
    fn current_seq_reflects_latest_emission() {
        let (b, _rx) = make_builder();
        assert_eq!(b.current_seq(), 0);
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        b.push_event(
            span.id(),
            Some("sh"),
            Some("m"),
            None,
            EventKind::Send { data: "a".into() },
        );
        assert_eq!(b.current_seq(), 0);
        b.push_buffer_event("sh", "m", BufferEventKind::Grew { data: "b".into() });
        assert_eq!(b.current_seq(), 1);
    }

    #[test]
    fn round_trip_serde_json() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let buffer_seq = b.push_buffer_event(
            "sh",
            "m",
            BufferEventKind::Matched {
                before: "".into(),
                matched: "ok".into(),
                after: "".into(),
            },
        );
        b.emit_match_done_record(
            span.id(),
            "sh",
            "m",
            "ok",
            Duration::from_millis(1),
            None,
            buffer_seq,
            None,
        );
        span.close();
        let log = b.build(
            TestInfo {
                name: "t".into(),
                path: "t.relux".into(),
                duration_ms: 1,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        );
        let json = serde_json::to_string(&log).unwrap();
        let back: StructuredLog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.events.len(), log.events.len());
        assert_eq!(back.buffer_events.len(), log.buffer_events.len());
        assert_eq!(back.spans.len(), log.spans.len());
    }

    #[test]
    fn push_event_records_source_when_irspan_provided() {
        use relux_core::Span as CoreSpan;
        use relux_core::diagnostics::IrSpan;
        use relux_core::table::FileId;

        let (builder, _rx) = make_builder();
        let path = PathBuf::from("/project/t.relux");
        let file_id = FileId::new(path.clone());
        let src = relux_core::table::SourceFile::new(path, "abcdef\n".into());
        builder.sources_for_tests().insert(file_id.clone(), src);

        let span_guard = builder.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let ir = IrSpan::new(file_id, CoreSpan::new(1, 4));
        builder.push_event(
            span_guard.id(),
            None,
            None,
            Some(&ir),
            EventKind::Annotate { text: "hi".into() },
        );
        drop(span_guard);

        let log = builder.build(
            TestInfo {
                name: "t".into(),
                path: "t".into(),
                duration_ms: 0,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        );
        let ev = log
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Annotate { .. }))
            .unwrap();
        let src = ev.source.as_ref().expect("source present");
        assert_eq!(src.start, 1);
        assert_eq!(src.end, 4);
    }

    #[test]
    fn build_populates_sources_for_referenced_files_only() {
        use relux_core::Span as CoreSpan;
        use relux_core::diagnostics::IrSpan;
        use relux_core::table::FileId;

        let (builder, _rx) = make_builder();
        let path_used = PathBuf::from("/project/used.relux");
        let path_unused = PathBuf::from("/project/unused.relux");
        let fid_used = FileId::new(path_used.clone());
        let fid_unused = FileId::new(path_unused.clone());
        builder.sources_for_tests().insert(
            fid_used.clone(),
            relux_core::table::SourceFile::new(path_used, "u-content\n".into()),
        );
        builder.sources_for_tests().insert(
            fid_unused,
            relux_core::table::SourceFile::new(path_unused, "x-content\n".into()),
        );

        let test_span = builder.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let ir = IrSpan::new(fid_used, CoreSpan::new(0, 1));
        builder.emit_annotate(test_span.id(), "sh", "m", "a", Some(&ir));
        drop(test_span);

        let log = builder.build(
            TestInfo {
                name: "t".into(),
                path: "t".into(),
                duration_ms: 0,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        );

        assert_eq!(log.sources.len(), 1, "only referenced files in sources");
        assert_eq!(
            log.sources.get("used.relux"),
            Some(&"u-content\n".to_string())
        );
        assert!(!log.sources.contains_key("unused.relux"));
    }

    #[test]
    fn emit_send_records_source_from_irspan() {
        use relux_core::Span as CoreSpan;
        use relux_core::diagnostics::IrSpan;
        use relux_core::table::FileId;

        let (builder, _rx) = make_builder();
        let path = PathBuf::from("/project/t.relux");
        let file_id = FileId::new(path.clone());
        let src = relux_core::table::SourceFile::new(path, "send hello\n".into());
        builder.sources_for_tests().insert(file_id.clone(), src);

        let test_span = builder.open_span(SpanKind::Test { name: "t".into() }, None, None);
        let ir = IrSpan::new(file_id, CoreSpan::new(0, 4));
        builder.emit_send(test_span.id(), "sh", "m", "hello", Some(&ir));
        drop(test_span);

        let log = builder.build(
            TestInfo {
                name: "t".into(),
                path: "t".into(),
                duration_ms: 0,
            },
            EnvInfo::default(),
            TestOutcome::Pass,
            Vec::new(),
        );
        let ev = log
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Send { .. }))
            .unwrap();
        let s = ev.source.as_ref().expect("source set");
        assert_eq!(s.start, 0);
        assert_eq!(s.end, 4);
    }

    #[test]
    fn source_location_carries_byte_range() {
        use relux_core::Span as CoreSpan;
        use relux_core::diagnostics::IrSpan;
        use relux_core::table::FileId;

        let (builder, _rx) = make_builder();
        let path = PathBuf::from("/project/lib/x.relux");
        let file_id = FileId::new(path.clone());
        let src = relux_core::table::SourceFile::new(path, "line 1\nline 2\nline 3\n".into());
        builder.sources_for_tests().insert(file_id.clone(), src);

        let ir = IrSpan::new(file_id, CoreSpan::new(7, 13));
        let loc = builder.resolve_location_for_tests(&ir).expect("resolve");
        assert_eq!(loc.file, "lib/x.relux");
        assert_eq!(loc.line, 2);
        assert_eq!(loc.start, 7);
        assert_eq!(loc.end, 13);
    }

    #[test]
    fn test_outcome_skip_serde_round_trip() {
        use super::super::skip::SkipRecord;
        use super::super::span::MarkerEvalDetail;
        use super::super::span::MarkerEvalKind;

        let original = TestOutcome::Skip(SkipRecord {
            span: 42u64,
            event_seq: 7u64,
            marker_kind: MarkerEvalKind::Skip,
            evaluation: MarkerEvalDetail::Unconditional,
        });
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("\"kind\":\"skip\""),
            "expected `\"kind\":\"skip\"` in JSON, got: {json}"
        );
        let parsed: TestOutcome = serde_json::from_str(&json).unwrap();
        match parsed {
            TestOutcome::Skip(rec) => {
                assert_eq!(rec.span, 42u64);
                assert_eq!(rec.event_seq, 7u64);
            }
            other => panic!("expected TestOutcome::Skip, got {other:?}"),
        }
    }
}
