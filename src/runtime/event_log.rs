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
pub enum LogEventKind {
    ShellSwitch { name: String },
    Send { data: String },
    Recv { data: String },
    MatchStart { pattern: String, is_regex: bool },
    MatchDone { matched: String, elapsed: Duration },
    Timeout { pattern: String },
    NegMatchStart { pattern: String, is_regex: bool },
    NegMatchPass { pattern: String, elapsed: Duration },
    NegMatchFail { pattern: String, matched_text: String },
    FailPatternSet { pattern: String },
    FailPatternTriggered { pattern: String, matched_line: String },
    EffectSetup { effect: String },
    EffectTeardown { effect: String },
    Sleep { duration: Duration },
    Annotate { text: String },
    Log { message: String },
    VarLet { name: String, value: String },
    VarAssign { name: String, value: String },
    FnEnter { name: String },
    FnExit,
    Cleanup { shell: String },
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
