use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::span::SpanId;

pub type EventSeq = u64;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Event {
    pub seq: EventSeq,
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub ts: Duration,
    pub span: SpanId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EventKind {
    // Shell lifecycle
    ShellSpawn {
        name: String,
        command: String,
    },
    ShellReady {
        name: String,
    },
    ShellSwitch {
        name: String,
    },
    ShellTerminate {
        name: String,
    },
    ShellAlias {
        name: String,
        source: String,
    },

    // I/O
    Send {
        data: String,
    },
    Recv {
        data: String,
    },

    // Matching — buffer_seq references the corresponding buffer_events entry.
    MatchStart {
        pattern: String,
        is_regex: bool,
    },
    MatchDone {
        matched: String,
        #[serde(with = "super::ts_duration_ms")]
        #[ts(as = "f64")]
        elapsed: Duration,
        captures: Option<HashMap<String, String>>,
        buffer_seq: EventSeq,
    },
    Timeout {
        pattern: String,
        /// `None` when no buffer event corresponds (the failure record's
        /// `buffer_tail` is canonical for the timeout state).
        buffer_seq: Option<EventSeq>,
    },

    // Fail patterns
    FailPatternSet {
        pattern: String,
    },
    FailPatternCleared,
    FailPatternTriggered {
        pattern: String,
        matched_line: String,
        /// `None` for fail-pattern hits — they observe without advancing the
        /// cursor, so no `Matched` buffer event corresponds.
        buffer_seq: Option<EventSeq>,
    },

    // Control flow
    SleepStart {
        #[serde(with = "super::ts_duration_ms")]
        #[ts(as = "f64")]
        duration: Duration,
    },
    SleepDone,
    TimeoutSet {
        timeout: String,
        previous: String,
    },

    // Values
    VarLet {
        name: String,
        value: String,
    },
    VarAssign {
        name: String,
        value: String,
    },
    StringEval {
        result: String,
    },
    Interpolation {
        template: String,
        result: String,
        bindings: Vec<(String, String)>,
    },

    // Diagnostics
    Annotate {
        text: String,
    },
    Log {
        message: String,
    },
    Warning {
        message: String,
    },
    Error {
        message: String,
    },
}
