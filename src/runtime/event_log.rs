use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub timestamp: Duration,
    pub shell: String,
    pub kind: LogEventKind,
}

#[derive(Debug, Clone)]
pub enum BufferSnapshot {
    /// Successful match: before (skipped), matched, after (remaining)
    Match { before: String, matched: String, after: String },
    /// Timeout: tail of buffer
    Tail { content: String },
}

#[derive(Debug, Clone)]
pub enum LogEventKind {
    ShellSwitch { name: String },
    ShellSpawn { name: String, command: String },
    ShellReady { name: String },
    ShellTerminate { name: String },
    ShellAlias { name: String, source: String },
    Send { data: String },
    Recv { data: String },
    MatchStart { pattern: String, is_regex: bool },
    MatchDone { matched: String, elapsed: Duration, buffer: BufferSnapshot, captures: Option<HashMap<String, String>> },
    Timeout { pattern: String, buffer: BufferSnapshot },
    BufferReset { buffer: BufferSnapshot },
    FailPatternSet { pattern: String },
    FailPatternCleared,
    FailPatternTriggered { pattern: String, matched_line: String, buffer: BufferSnapshot },
    EffectSetup { effect: String },
    EffectTeardown { effect: String },
    EffectSkip { effect: String, reason: String },
    Sleep { duration: Duration },
    Annotate { text: String },
    Log { message: String },
    VarLet { name: String, value: String },
    VarAssign { name: String, value: String },
    FnEnter { name: String, args: Vec<(String, String)> },
    FnExit { name: String, return_value: String, restored_timeout: Option<String>, restored_fail_pattern: Option<String> },
    Cleanup { shell: String },
    TimeoutSet { timeout: String, previous: String },
    StringEval { result: String },
Interpolation { template: String, result: String, bindings: Vec<(String, String)> },
}

#[derive(Clone)]
pub struct EventCollector {
    events: Arc<Mutex<Vec<LogEvent>>>,
    test_start: Instant,
}

impl EventCollector {
    pub fn new(test_start: Instant) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            test_start,
        }
    }

    pub async fn push(&self, shell: &str, kind: LogEventKind) {
        let event = LogEvent {
            timestamp: self.test_start.elapsed(),
            shell: shell.to_string(),
            kind,
        };
        self.events.lock().await.push(event);
    }

    pub async fn take(self) -> Vec<LogEvent> {
        match Arc::try_unwrap(self.events) {
            Ok(mutex) => mutex.into_inner(),
            Err(arc) => arc.lock().await.clone(),
        }
    }
}
