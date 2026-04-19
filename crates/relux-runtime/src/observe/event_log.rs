use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub timestamp: Duration,
    pub shell: String,
    pub kind: LogEventKind,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone)]
pub enum BufferSnapshot {
    /// Successful match: before (skipped), matched, after (remaining)
    Match {
        before: String,
        matched: String,
        after: String,
    },
    /// Timeout: tail of buffer
    Tail { content: String },
}

#[derive(Debug, Clone)]
pub enum LogEventKind {
    ShellSwitch {
        name: String,
    },
    ShellSpawn {
        name: String,
        command: String,
    },
    ShellReady {
        name: String,
    },
    ShellTerminate {
        name: String,
    },
    ShellAlias {
        name: String,
        source: String,
    },
    Send {
        data: String,
    },
    Recv {
        data: String,
    },
    MatchStart {
        pattern: String,
        is_regex: bool,
    },
    MatchDone {
        matched: String,
        elapsed: Duration,
        buffer: BufferSnapshot,
        captures: Option<HashMap<String, String>>,
    },
    Timeout {
        pattern: String,
        buffer: BufferSnapshot,
    },
    BufferReset {
        buffer: BufferSnapshot,
    },
    FailPatternSet {
        pattern: String,
    },
    FailPatternCleared,
    FailPatternTriggered {
        pattern: String,
        matched_line: String,
        buffer: BufferSnapshot,
    },
    EffectSetup {
        effect: String,
    },
    EffectTeardown {
        effect: String,
    },
    SleepStart {
        duration: Duration,
    },
    SleepDone,
    Annotate {
        text: String,
    },
    Log {
        message: String,
    },
    VarLet {
        name: String,
        value: String,
    },
    VarAssign {
        name: String,
        value: String,
    },
    FnEnter {
        name: String,
        args: Vec<(String, String)>,
    },
    FnExit {
        name: String,
        return_value: String,
        restored_timeout: Option<String>,
        restored_fail_pattern: Option<String>,
    },
    Cleanup {
        shell: String,
    },
    TimeoutSet {
        timeout: String,
        previous: String,
    },
    StringEval {
        result: String,
    },
    Interpolation {
        template: String,
        result: String,
        bindings: Vec<(String, String)>,
    },
    Failure,
    Error {
        message: String,
    },
    Warning {
        message: String,
    },
}
