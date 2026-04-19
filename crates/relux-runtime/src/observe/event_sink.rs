use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use relux_core::diagnostics::IrSpan;
use relux_core::table::SourceTable;

use crate::observe::event_log::BufferSnapshot;
use crate::observe::event_log::LogEvent;
use crate::observe::event_log::LogEventKind;
use crate::observe::event_log::SourceLocation;
use crate::observe::progress::ProgressEvent;
use crate::observe::progress::ProgressTx;

#[derive(Clone)]
pub struct EventSink {
    events: Arc<Mutex<Vec<LogEvent>>>,
    progress_tx: ProgressTx,
    test_start: Instant,
    sources: SourceTable,
    project_root: Arc<Path>,
}

impl EventSink {
    pub fn new(
        progress_tx: ProgressTx,
        test_start: Instant,
        sources: SourceTable,
        project_root: Arc<Path>,
    ) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            progress_tx,
            test_start,
            sources,
            project_root,
        }
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

    fn push(&self, shell: &str, kind: LogEventKind, span: Option<&IrSpan>) {
        let location = span.and_then(|s| self.resolve_location(s));
        let event = LogEvent {
            timestamp: self.test_start.elapsed(),
            shell: shell.to_string(),
            kind,
            location,
        };
        self.events.lock().unwrap().push(event);
    }

    fn push_progress(&self, event: ProgressEvent) {
        let _ = self.progress_tx.send(event);
    }

    /// Extract all collected events at test end.
    pub fn take(self) -> Vec<LogEvent> {
        match Arc::try_unwrap(self.events) {
            Ok(mutex) => mutex.into_inner().unwrap(),
            Err(arc) => {
                let mut guard = arc.lock().unwrap();
                std::mem::take(&mut *guard)
            }
        }
    }

    // ─── Shell Lifecycle ────────────────────────────────────────

    pub fn emit_shell_spawn(
        &self,
        shell: impl AsRef<str>,
        command: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellSpawn {
                name: shell.to_string(),
                command: command.as_ref().to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::ShellSpawn);
    }

    pub fn emit_shell_ready(&self, shell: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellReady {
                name: shell.to_string(),
            },
            None,
        );
    }

    pub fn emit_shell_switch(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellSwitch {
                name: shell.to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::ShellSwitch(shell.to_string()));
    }

    pub fn emit_shell_terminate(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellTerminate {
                name: shell.to_string(),
            },
            span,
        );
    }

    pub fn emit_shell_alias(&self, shell: impl AsRef<str>, source: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellAlias {
                name: shell.to_string(),
                source: source.as_ref().to_string(),
            },
            None,
        );
    }

    // ─── I/O ────────────────────────────────────────────────────

    pub fn emit_send(&self, shell: impl AsRef<str>, data: impl AsRef<str>, span: Option<&IrSpan>) {
        self.push(
            shell.as_ref(),
            LogEventKind::Send {
                data: data.as_ref().to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::Send);
    }

    pub fn emit_recv(&self, shell: impl AsRef<str>, data: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::Recv {
                data: data.as_ref().to_string(),
            },
            None,
        );
    }

    // ─── Pattern Matching ───────────────────────────────────────

    pub fn emit_match_start(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        is_regex: bool,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::MatchStart {
                pattern: pattern.as_ref().to_string(),
                is_regex,
            },
            span,
        );
        self.push_progress(ProgressEvent::MatchStart);
    }

    pub fn emit_match_done(
        &self,
        shell: impl AsRef<str>,
        matched: impl AsRef<str>,
        elapsed: Duration,
        buffer: BufferSnapshot,
        captures: Option<HashMap<String, String>>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::MatchDone {
                matched: matched.as_ref().to_string(),
                elapsed,
                buffer,
                captures,
            },
            span,
        );
        self.push_progress(ProgressEvent::MatchDone);
    }

    pub fn emit_timeout(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        buffer: BufferSnapshot,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::Timeout {
                pattern: pattern.as_ref().to_string(),
                buffer,
            },
            span,
        );
        self.push_progress(ProgressEvent::Timeout);
    }

    pub fn emit_buffer_reset(
        &self,
        shell: impl AsRef<str>,
        buffer: BufferSnapshot,
        span: Option<&IrSpan>,
    ) {
        self.push(shell.as_ref(), LogEventKind::BufferReset { buffer }, span);
    }

    // ─── Fail Patterns ──────────────────────────────────────────

    pub fn emit_fail_pattern_set(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::FailPatternSet {
                pattern: pattern.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_fail_pattern_cleared(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        self.push(shell.as_ref(), LogEventKind::FailPatternCleared, span);
    }

    pub fn emit_fail_pattern_triggered(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        matched_line: impl AsRef<str>,
        buffer: BufferSnapshot,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::FailPatternTriggered {
                pattern: pattern.as_ref().to_string(),
                matched_line: matched_line.as_ref().to_string(),
                buffer,
            },
            span,
        );
        self.push_progress(ProgressEvent::FailPattern);
    }

    // ─── Effects ────────────────────────────────────────────────

    pub fn emit_effect_setup(
        &self,
        shell: impl AsRef<str>,
        effect: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::EffectSetup {
                effect: effect.as_ref().to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::EffectSetup(effect.as_ref().to_string()));
    }

    pub fn emit_effect_teardown(
        &self,
        shell: impl AsRef<str>,
        effect: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::EffectTeardown {
                effect: effect.as_ref().to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::EffectTeardown);
    }

    pub fn emit_cleanup(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::Cleanup {
                shell: shell.to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::Cleanup);
    }

    // ─── Control Flow ───────────────────────────────────────────

    pub fn emit_sleep_start(
        &self,
        shell: impl AsRef<str>,
        duration: Duration,
        span: Option<&IrSpan>,
    ) {
        self.push(shell.as_ref(), LogEventKind::SleepStart { duration }, span);
        self.push_progress(ProgressEvent::SleepStart);
    }

    pub fn emit_sleep_done(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        self.push(shell.as_ref(), LogEventKind::SleepDone, span);
        self.push_progress(ProgressEvent::SleepDone);
    }

    pub fn emit_fn_enter(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        args: &[(String, String)],
        span: Option<&IrSpan>,
    ) {
        let name = name.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::FnEnter {
                name: name.to_string(),
                args: args.to_vec(),
            },
            span,
        );
        self.push_progress(ProgressEvent::FnEnter(name.to_string()));
    }

    pub fn emit_fn_exit(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        return_value: impl AsRef<str>,
        restored_timeout: Option<impl AsRef<str>>,
        restored_fail_pattern: Option<impl AsRef<str>>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::FnExit {
                name: name.as_ref().to_string(),
                return_value: return_value.as_ref().to_string(),
                restored_timeout: restored_timeout.as_ref().map(|s| s.as_ref().to_string()),
                restored_fail_pattern: restored_fail_pattern
                    .as_ref()
                    .map(|s| s.as_ref().to_string()),
            },
            span,
        );
        self.push_progress(ProgressEvent::FnExit);
    }

    // ─── Variables & Evaluation ─────────────────────────────────

    pub fn emit_var_let(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::VarLet {
                name: name.as_ref().to_string(),
                value: value.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_var_assign(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::VarAssign {
                name: name.as_ref().to_string(),
                value: value.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_timeout_set(
        &self,
        shell: impl AsRef<str>,
        timeout: impl AsRef<str>,
        previous: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::TimeoutSet {
                timeout: timeout.as_ref().to_string(),
                previous: previous.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_string_eval(
        &self,
        shell: impl AsRef<str>,
        result: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::StringEval {
                result: result.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_interpolation(
        &self,
        shell: impl AsRef<str>,
        template: impl AsRef<str>,
        result: impl AsRef<str>,
        bindings: &[(String, String)],
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::Interpolation {
                template: template.as_ref().to_string(),
                result: result.as_ref().to_string(),
                bindings: bindings.to_vec(),
            },
            span,
        );
    }

    // ─── Diagnostics ────────────────────────────────────────────

    pub fn emit_annotate(
        &self,
        shell: impl AsRef<str>,
        text: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        let text = text.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Annotate {
                text: text.to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::Annotation(text.to_string()));
    }

    pub fn emit_log(
        &self,
        shell: impl AsRef<str>,
        message: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::Log {
                message: message.as_ref().to_string(),
            },
            span,
        );
    }

    pub fn emit_warning(
        &self,
        shell: impl AsRef<str>,
        message: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        let message = message.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Warning {
                message: message.to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::Warning(message.to_string()));
    }

    pub fn emit_error(
        &self,
        shell: impl AsRef<str>,
        message: impl AsRef<str>,
        span: Option<&IrSpan>,
    ) {
        let message = message.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Error {
                message: message.to_string(),
            },
            span,
        );
        self.push_progress(ProgressEvent::Error(message.to_string()));
    }

    pub fn emit_failure(&self, shell: impl AsRef<str>, span: Option<&IrSpan>) {
        self.push(shell.as_ref(), LogEventKind::Failure, span);
        self.push_progress(ProgressEvent::Failure);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::observe::progress;
    use relux_core::table::SharedTable;

    fn make_sink() -> (
        EventSink,
        tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>,
    ) {
        let (tx, rx) = progress::channel();
        let sources: SourceTable = SharedTable::new();
        let sink = EventSink::new(
            tx,
            Instant::now(),
            sources,
            Arc::from(PathBuf::from("/project").as_path()),
        );
        (sink, rx)
    }

    #[test]
    fn emit_send_pushes_event_and_progress() {
        let (sink, mut rx) = make_sink();
        sink.emit_send("sh", "hello", None);
        let events = sink.take();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].kind, LogEventKind::Send { data } if data == "hello"));
        assert_eq!(events[0].shell, "sh");
        assert!(events[0].location.is_none());
        assert!(matches!(rx.try_recv(), Ok(ProgressEvent::Send)));
    }

    #[test]
    fn emit_recv_pushes_event_only() {
        let (sink, mut rx) = make_sink();
        sink.emit_recv("sh", "data");
        let events = sink.take();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].kind, LogEventKind::Recv { .. }));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn take_extracts_all_events() {
        let (sink, _rx) = make_sink();
        sink.emit_send("sh", "a", None);
        sink.emit_send("sh", "b", None);
        let events = sink.take();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn clone_shares_event_storage() {
        let (sink, _rx) = make_sink();
        let sink2 = sink.clone();
        sink.emit_send("sh", "a", None);
        sink2.emit_send("sh", "b", None);
        let events = sink.take();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn resolve_location_from_span() {
        use relux_core::Span;
        use relux_core::table::FileId;
        use relux_core::table::SourceFile;

        let (tx, _rx) = progress::channel();
        let sources: SourceTable = SharedTable::new();
        let file_id = FileId::new(PathBuf::from("/project/tests/login.relux"));
        sources.insert(
            file_id.clone(),
            SourceFile::new(
                PathBuf::from("/project/tests/login.relux"),
                "line1\nline2\nline3\n".to_string(),
            ),
        );
        let sink = EventSink::new(
            tx,
            Instant::now(),
            sources,
            Arc::from(PathBuf::from("/project").as_path()),
        );

        let span = IrSpan::new(file_id, Span::new(6, 11)); // "line2"
        let loc = sink.resolve_location(&span).unwrap();
        assert_eq!(loc.file, "tests/login.relux");
        assert_eq!(loc.line, 2);
    }
}
