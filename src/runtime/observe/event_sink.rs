use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use crate::runtime::observe::event_log::BufferSnapshot;
use crate::runtime::observe::event_log::LogEvent;
use crate::runtime::observe::event_log::LogEventKind;
use crate::runtime::observe::progress::ProgressEvent;
use crate::runtime::observe::progress::ProgressTx;

#[derive(Clone)]
pub struct EventSink {
    events: Arc<Mutex<Vec<LogEvent>>>,
    progress_tx: ProgressTx,
    test_start: Instant,
}

impl EventSink {
    pub fn new(progress_tx: ProgressTx, test_start: Instant) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            progress_tx,
            test_start,
        }
    }

    fn push(&self, shell: &str, kind: LogEventKind) {
        let event = LogEvent {
            timestamp: self.test_start.elapsed(),
            shell: shell.to_string(),
            kind,
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

    pub fn emit_shell_spawn(&self, shell: impl AsRef<str>, command: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellSpawn {
                name: shell.to_string(),
                command: command.as_ref().to_string(),
            },
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
        );
    }

    pub fn emit_shell_switch(&self, shell: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellSwitch {
                name: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSwitch(shell.to_string()));
    }

    pub fn emit_shell_terminate(&self, shell: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::ShellTerminate {
                name: shell.to_string(),
            },
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
        );
    }

    // ─── I/O ────────────────────────────────────────────────────

    pub fn emit_send(&self, shell: impl AsRef<str>, data: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::Send {
                data: data.as_ref().to_string(),
            },
        );
        self.push_progress(ProgressEvent::Send);
    }

    pub fn emit_recv(&self, shell: impl AsRef<str>, data: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::Recv {
                data: data.as_ref().to_string(),
            },
        );
    }

    // ─── Pattern Matching ───────────────────────────────────────

    pub fn emit_match_start(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        is_regex: bool,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::MatchStart {
                pattern: pattern.as_ref().to_string(),
                is_regex,
            },
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
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::MatchDone {
                matched: matched.as_ref().to_string(),
                elapsed,
                buffer,
                captures,
            },
        );
        self.push_progress(ProgressEvent::MatchDone);
    }

    pub fn emit_timeout(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        buffer: BufferSnapshot,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::Timeout {
                pattern: pattern.as_ref().to_string(),
                buffer,
            },
        );
        self.push_progress(ProgressEvent::Timeout);
    }

    pub fn emit_buffer_reset(&self, shell: impl AsRef<str>, buffer: BufferSnapshot) {
        self.push(shell.as_ref(), LogEventKind::BufferReset { buffer });
    }

    // ─── Fail Patterns ──────────────────────────────────────────

    pub fn emit_fail_pattern_set(&self, shell: impl AsRef<str>, pattern: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::FailPatternSet {
                pattern: pattern.as_ref().to_string(),
            },
        );
    }

    pub fn emit_fail_pattern_cleared(&self, shell: impl AsRef<str>) {
        self.push(shell.as_ref(), LogEventKind::FailPatternCleared);
    }

    pub fn emit_fail_pattern_triggered(
        &self,
        shell: impl AsRef<str>,
        pattern: impl AsRef<str>,
        matched_line: impl AsRef<str>,
        buffer: BufferSnapshot,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::FailPatternTriggered {
                pattern: pattern.as_ref().to_string(),
                matched_line: matched_line.as_ref().to_string(),
                buffer,
            },
        );
        self.push_progress(ProgressEvent::FailPattern);
    }

    // ─── Effects ────────────────────────────────────────────────

    pub fn emit_effect_setup(&self, shell: impl AsRef<str>, effect: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::EffectSetup {
                effect: effect.as_ref().to_string(),
            },
        );
        self.push_progress(ProgressEvent::EffectSetup(effect.as_ref().to_string()));
    }

    pub fn emit_effect_teardown(&self, shell: impl AsRef<str>, effect: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::EffectTeardown {
                effect: effect.as_ref().to_string(),
            },
        );
        self.push_progress(ProgressEvent::EffectTeardown);
    }

    pub fn emit_cleanup(&self, shell: impl AsRef<str>) {
        let shell = shell.as_ref();
        self.push(
            shell,
            LogEventKind::Cleanup {
                shell: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Cleanup);
    }

    // ─── Control Flow ───────────────────────────────────────────

    pub fn emit_sleep_start(&self, shell: impl AsRef<str>, duration: Duration) {
        self.push(shell.as_ref(), LogEventKind::SleepStart { duration });
        self.push_progress(ProgressEvent::SleepStart);
    }

    pub fn emit_sleep_done(&self, shell: impl AsRef<str>) {
        self.push(shell.as_ref(), LogEventKind::SleepDone);
        self.push_progress(ProgressEvent::SleepDone);
    }

    pub fn emit_fn_enter(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        args: &[(String, String)],
    ) {
        let name = name.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::FnEnter {
                name: name.to_string(),
                args: args.to_vec(),
            },
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
        );
        self.push_progress(ProgressEvent::FnExit);
    }

    // ─── Variables & Evaluation ─────────────────────────────────

    pub fn emit_var_let(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::VarLet {
                name: name.as_ref().to_string(),
                value: value.as_ref().to_string(),
            },
        );
    }

    pub fn emit_var_assign(
        &self,
        shell: impl AsRef<str>,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::VarAssign {
                name: name.as_ref().to_string(),
                value: value.as_ref().to_string(),
            },
        );
    }

    pub fn emit_timeout_set(
        &self,
        shell: impl AsRef<str>,
        timeout: impl AsRef<str>,
        previous: impl AsRef<str>,
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::TimeoutSet {
                timeout: timeout.as_ref().to_string(),
                previous: previous.as_ref().to_string(),
            },
        );
    }

    pub fn emit_string_eval(&self, shell: impl AsRef<str>, result: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::StringEval {
                result: result.as_ref().to_string(),
            },
        );
    }

    pub fn emit_interpolation(
        &self,
        shell: impl AsRef<str>,
        template: impl AsRef<str>,
        result: impl AsRef<str>,
        bindings: &[(String, String)],
    ) {
        self.push(
            shell.as_ref(),
            LogEventKind::Interpolation {
                template: template.as_ref().to_string(),
                result: result.as_ref().to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    // ─── Diagnostics ────────────────────────────────────────────

    pub fn emit_annotate(&self, shell: impl AsRef<str>, text: impl AsRef<str>) {
        let text = text.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Annotate {
                text: text.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Annotation(text.to_string()));
    }

    pub fn emit_log(&self, shell: impl AsRef<str>, message: impl AsRef<str>) {
        self.push(
            shell.as_ref(),
            LogEventKind::Log {
                message: message.as_ref().to_string(),
            },
        );
    }

    pub fn emit_warning(&self, shell: impl AsRef<str>, message: impl AsRef<str>) {
        let message = message.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Warning {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Warning(message.to_string()));
    }

    pub fn emit_error(&self, shell: impl AsRef<str>, message: impl AsRef<str>) {
        let message = message.as_ref();
        self.push(
            shell.as_ref(),
            LogEventKind::Error {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Error(message.to_string()));
    }

    pub fn emit_failure(&self, shell: impl AsRef<str>) {
        self.push(shell.as_ref(), LogEventKind::Failure);
        self.push_progress(ProgressEvent::Failure);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::observe::progress;

    fn make_sink() -> (
        EventSink,
        tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>,
    ) {
        let (tx, rx) = progress::channel();
        let sink = EventSink::new(tx, Instant::now());
        (sink, rx)
    }

    #[test]
    fn emit_send_pushes_event_and_progress() {
        let (sink, mut rx) = make_sink();
        sink.emit_send("sh", "hello");
        let events = sink.take();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].kind, LogEventKind::Send { data } if data == "hello"));
        assert_eq!(events[0].shell, "sh");
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
        sink.emit_send("sh", "a");
        sink.emit_send("sh", "b");
        let events = sink.take();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn clone_shares_event_storage() {
        let (sink, _rx) = make_sink();
        let sink2 = sink.clone();
        sink.emit_send("sh", "a");
        sink2.emit_send("sh", "b");
        let events = sink.take();
        assert_eq!(events.len(), 2);
    }
}
