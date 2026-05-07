use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use relux_core::diagnostics::IrSpan;
use relux_core::table::SourceTable;

use super::EnvInfo;
use super::SourceLocation;
use super::StructuredLog;
use super::TestInfo;
use super::buffer::BufferEvent;
use super::buffer::BufferEventKind;
use super::event::Event;
use super::event::EventKind;
use super::event::EventSeq;
use super::failure::FailureRecord;
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
        })
    }

    fn push_progress(&self, event: ProgressEvent) {
        let _ = self.progress_tx.send(event);
    }

    // ─── Span lifecycle ───────────────────────────────────────────

    pub fn open_span(
        &self,
        kind: SpanKind,
        parent: Option<SpanId>,
        location: Option<&IrSpan>,
    ) -> SpanId {
        let location = location.and_then(|s| self.resolve_location(s));
        let start_ts = self.now();
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
    }

    pub fn close_span(&self, id: SpanId) {
        let end_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(span) = inner.spans.get_mut(&id) {
            span.end_ts = Some(end_ts);
        }
    }

    // ─── Raw event/buffer-event push ──────────────────────────────

    pub fn push_event(&self, span: SpanId, shell: Option<&str>, kind: EventKind) -> EventSeq {
        let ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.events.push(Event {
            seq,
            ts,
            span,
            shell: shell.map(String::from),
            kind,
        });
        seq
    }

    pub fn push_buffer_event(&self, shell: &str, kind: BufferEventKind) -> EventSeq {
        let ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.buffer_events.push(BufferEvent {
            seq,
            ts,
            shell: shell.to_string(),
            kind,
        });
        seq
    }

    // ─── Shells glossary ──────────────────────────────────────────

    pub fn record_shell_spawn(&self, name: &str, command: &str) {
        let spawn_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        inner.shells.insert(
            name.to_string(),
            ShellRecord {
                spawn_ts,
                terminate_ts: None,
                command: command.to_string(),
                alias_of: None,
            },
        );
    }

    pub fn record_shell_terminate(&self, name: &str) {
        let terminate_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(rec) = inner.shells.get_mut(name) {
            rec.terminate_ts = Some(terminate_ts);
        }
    }

    pub fn record_shell_alias(&self, alias: &str, source: &str) {
        let spawn_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        inner.shells.insert(
            alias.to_string(),
            ShellRecord {
                spawn_ts,
                terminate_ts: None,
                command: String::new(),
                alias_of: Some(source.to_string()),
            },
        );
    }

    // ─── Convenience emitters (mirror EventSink shape) ────────────

    // Shell lifecycle ---------------------------------------------------

    pub fn emit_shell_spawn(&self, span: SpanId, shell: &str, command: &str) {
        self.record_shell_spawn(shell, command);
        self.push_event(
            span,
            Some(shell),
            EventKind::ShellSpawn {
                name: shell.to_string(),
                command: command.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSpawn);
    }

    pub fn emit_shell_ready(&self, span: SpanId, shell: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::ShellReady {
                name: shell.to_string(),
            },
        );
    }

    pub fn emit_shell_switch(&self, span: SpanId, shell: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::ShellSwitch {
                name: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSwitch(shell.to_string()));
    }

    pub fn emit_shell_terminate(&self, span: SpanId, shell: &str) {
        self.record_shell_terminate(shell);
        self.push_event(
            span,
            Some(shell),
            EventKind::ShellTerminate {
                name: shell.to_string(),
            },
        );
    }

    pub fn emit_shell_alias(&self, span: SpanId, alias: &str, source: &str) {
        self.record_shell_alias(alias, source);
        self.push_event(
            span,
            Some(alias),
            EventKind::ShellAlias {
                name: alias.to_string(),
                source: source.to_string(),
            },
        );
    }

    // I/O ---------------------------------------------------------------

    pub fn emit_send(&self, span: SpanId, shell: &str, data: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Send {
                data: data.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Send);
    }

    pub fn emit_recv(&self, span: SpanId, shell: &str, data: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Recv {
                data: data.to_string(),
            },
        );
    }

    // Matching ----------------------------------------------------------

    pub fn emit_match_start(&self, span: SpanId, shell: &str, pattern: &str, is_regex: bool) {
        self.push_event(
            span,
            Some(shell),
            EventKind::MatchStart {
                pattern: pattern.to_string(),
                is_regex,
            },
        );
        self.push_progress(ProgressEvent::MatchStart);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_match_done(
        &self,
        span: SpanId,
        shell: &str,
        matched: &str,
        elapsed: Duration,
        captures: Option<HashMap<String, String>>,
        before: &str,
        after: &str,
    ) {
        let buffer_seq = self.push_buffer_event(
            shell,
            BufferEventKind::Matched {
                before: before.to_string(),
                matched: matched.to_string(),
                after: after.to_string(),
            },
        );
        self.push_event(
            span,
            Some(shell),
            EventKind::MatchDone {
                matched: matched.to_string(),
                elapsed,
                captures,
                buffer_seq,
            },
        );
        self.push_progress(ProgressEvent::MatchDone);
    }

    pub fn emit_timeout(&self, span: SpanId, shell: &str, pattern: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Timeout {
                pattern: pattern.to_string(),
                buffer_seq: None,
            },
        );
        self.push_progress(ProgressEvent::Timeout);
    }

    pub fn emit_buffer_reset(&self, shell: &str, discarded: &str) {
        self.push_buffer_event(
            shell,
            BufferEventKind::Reset {
                discarded: discarded.to_string(),
            },
        );
    }

    // Fail patterns -----------------------------------------------------

    pub fn emit_fail_pattern_set(&self, span: SpanId, shell: &str, pattern: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::FailPatternSet {
                pattern: pattern.to_string(),
            },
        );
    }

    pub fn emit_fail_pattern_cleared(&self, span: SpanId, shell: &str) {
        self.push_event(span, Some(shell), EventKind::FailPatternCleared);
    }

    pub fn emit_fail_pattern_triggered(
        &self,
        span: SpanId,
        shell: &str,
        pattern: &str,
        matched_line: &str,
    ) {
        self.push_event(
            span,
            Some(shell),
            EventKind::FailPatternTriggered {
                pattern: pattern.to_string(),
                matched_line: matched_line.to_string(),
                buffer_seq: None,
            },
        );
        self.push_progress(ProgressEvent::FailPattern);
    }

    // Control flow ------------------------------------------------------

    pub fn emit_sleep_start(&self, span: SpanId, shell: &str, duration: Duration) {
        self.push_event(span, Some(shell), EventKind::SleepStart { duration });
        self.push_progress(ProgressEvent::SleepStart);
    }

    pub fn emit_sleep_done(&self, span: SpanId, shell: &str) {
        self.push_event(span, Some(shell), EventKind::SleepDone);
        self.push_progress(ProgressEvent::SleepDone);
    }

    pub fn emit_timeout_set(&self, span: SpanId, shell: &str, timeout: &str, previous: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::TimeoutSet {
                timeout: timeout.to_string(),
                previous: previous.to_string(),
            },
        );
    }

    // Values ------------------------------------------------------------

    pub fn emit_var_let(&self, span: SpanId, shell: &str, name: &str, value: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::VarLet {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    pub fn emit_var_assign(&self, span: SpanId, shell: &str, name: &str, value: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::VarAssign {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    pub fn emit_string_eval(&self, span: SpanId, shell: &str, result: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::StringEval {
                result: result.to_string(),
            },
        );
    }

    pub fn emit_interpolation(
        &self,
        span: SpanId,
        shell: &str,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
    ) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Interpolation {
                template: template.to_string(),
                result: result.to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    // Diagnostics -------------------------------------------------------

    pub fn emit_annotate(&self, span: SpanId, shell: &str, text: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Annotate {
                text: text.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Annotation(text.to_string()));
    }

    pub fn emit_log(&self, span: SpanId, shell: &str, message: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Log {
                message: message.to_string(),
            },
        );
    }

    pub fn emit_warning(&self, span: SpanId, shell: &str, message: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Warning {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Warning(message.to_string()));
    }

    pub fn emit_error(&self, span: SpanId, shell: &str, message: &str) {
        self.push_event(
            span,
            Some(shell),
            EventKind::Error {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Error(message.to_string()));
    }

    /// Push a `Failure` progress notification only. The structured failure
    /// information is carried in the `FailureRecord` passed to `build()`.
    pub fn emit_failure_progress(&self) {
        self.push_progress(ProgressEvent::Failure);
    }

    // ─── Final assembly ───────────────────────────────────────────

    pub fn build(
        self,
        test: TestInfo,
        env: EnvInfo,
        failure: Option<FailureRecord>,
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
        StructuredLog {
            test,
            env,
            shells: inner.shells,
            spans: inner.spans,
            events: inner.events,
            buffer_events: inner.buffer_events,
            failure,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
        let test_span = b.open_span(SpanKind::Test, None, None);
        let s1 = b.push_event(test_span, Some("sh"), EventKind::Send { data: "a".into() });
        let s2 = b.push_buffer_event("sh", BufferEventKind::Grew { data: "b".into() });
        let s3 = b.push_event(test_span, Some("sh"), EventKind::Recv { data: "c".into() });
        assert_eq!(s1, 0);
        assert_eq!(s2, 1);
        assert_eq!(s3, 2);
    }

    #[test]
    fn open_close_span_round_trips() {
        let (b, _rx) = make_builder();
        let id = b.open_span(SpanKind::Test, None, None);
        b.close_span(id);
        let inner = b.inner.lock().unwrap();
        let span = inner.spans.get(&id).unwrap();
        assert!(span.end_ts.is_some());
        assert!(span.parent.is_none());
    }

    #[test]
    fn span_ids_are_unique_and_parent_preserved() {
        let (b, _rx) = make_builder();
        let parent = b.open_span(SpanKind::Test, None, None);
        let child = b.open_span(
            SpanKind::ShellBlock { shell: "sh".into() },
            Some(parent),
            None,
        );
        assert_ne!(parent, child);
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.spans.get(&child).unwrap().parent, Some(parent));
    }

    #[test]
    fn shell_glossary_records_spawn_and_terminate() {
        let (b, _rx) = make_builder();
        b.record_shell_spawn("default", "/bin/bash");
        b.record_shell_terminate("default");
        let inner = b.inner.lock().unwrap();
        let rec = inner.shells.get("default").unwrap();
        assert_eq!(rec.command, "/bin/bash");
        assert!(rec.terminate_ts.is_some());
    }

    #[test]
    fn clone_shares_storage() {
        let (b, _rx) = make_builder();
        let b2 = b.clone();
        let span = b.open_span(SpanKind::Test, None, None);
        b2.push_event(span, Some("sh"), EventKind::Send { data: "x".into() });
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.events.len(), 1);
    }

    #[test]
    fn build_consumes_builder_and_yields_log() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test, None, None);
        b.push_event(span, Some("sh"), EventKind::Send { data: "x".into() });
        b.push_buffer_event("sh", BufferEventKind::Grew { data: "y".into() });
        b.close_span(span);
        let log = b.build(
            TestInfo {
                name: "t".into(),
                path: "t.relux".into(),
                outcome: "pass".into(),
                duration_ms: 1,
            },
            EnvInfo::default(),
            None,
        );
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.buffer_events.len(), 1);
        assert_eq!(log.spans.len(), 1);
        assert!(log.failure.is_none());
    }

    #[test]
    fn emit_send_pushes_event_and_progress() {
        let (b, mut rx) = make_builder();
        let span = b.open_span(SpanKind::Test, None, None);
        b.emit_send(span, "sh", "hello");
        let inner = b.inner.lock().unwrap();
        assert!(matches!(
            &inner.events.last().unwrap().kind,
            EventKind::Send { data } if data == "hello"
        ));
        drop(inner);
        assert!(matches!(rx.try_recv(), Ok(ProgressEvent::Send)));
    }

    #[test]
    fn emit_match_done_pushes_buffer_event_and_referencing_event() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test, None, None);
        b.emit_match_done(
            span,
            "sh",
            "ok",
            Duration::from_millis(5),
            None,
            "before",
            "after",
        );
        let inner = b.inner.lock().unwrap();
        assert_eq!(inner.buffer_events.len(), 1);
        let buf_seq = inner.buffer_events[0].seq;
        let last = inner.events.last().unwrap();
        match &last.kind {
            EventKind::MatchDone { buffer_seq, .. } => assert_eq!(*buffer_seq, buf_seq),
            _ => panic!("expected MatchDone"),
        }
    }

    #[test]
    fn round_trip_serde_json() {
        let (b, _rx) = make_builder();
        let span = b.open_span(SpanKind::Test, None, None);
        b.emit_match_done(span, "sh", "ok", Duration::from_millis(1), None, "", "");
        b.close_span(span);
        let log = b.build(
            TestInfo {
                name: "t".into(),
                path: "t.relux".into(),
                outcome: "pass".into(),
                duration_ms: 1,
            },
            EnvInfo::default(),
            None,
        );
        let json = serde_json::to_string(&log).unwrap();
        let back: StructuredLog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.events.len(), log.events.len());
        assert_eq!(back.buffer_events.len(), log.buffer_events.len());
        assert_eq!(back.spans.len(), log.spans.len());
    }
}
