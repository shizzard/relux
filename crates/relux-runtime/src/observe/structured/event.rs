use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::SourceLocation;
use super::span::SpanId;

pub type EventSeq = u64;

/// Structured representation of an effective timeout (the `IrTimeout` value
/// that bounded a wait or was installed by a `timeout` statement). Pre-formatted
/// with humantime so consumers never do duration arithmetic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TimeoutValue {
    Tolerance {
        duration: String,
        multiplier: String,
        total_duration: String,
        source: Option<SourceLocation>,
    },
    Assertion {
        duration: String,
        source: Option<SourceLocation>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct Event {
    pub seq: EventSeq,
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub ts: Duration,
    pub span: SpanId,
    pub shell: Option<String>,
    /// Stable identity for the shell, when present. Present iff
    /// `shell` is present. Viewers index by marker; `shell` is the
    /// display name at emit time (qualified post-export, bare pre).
    pub shell_marker: Option<String>,
    /// Source byte range that produced this event, when one is in
    /// scope at the emit site. Resolves against `StructuredLog.sources`.
    pub source: Option<SourceLocation>,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
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

    // Effect exposes — emitted at the end of effect setup, one per
    // expose decl. Hidden from the viewer timeline; surfaced as inline
    // props on the owning effect-setup span.
    EffectExposeShell {
        /// Caller-visible name (the rename target, or the source name
        /// when no `as <name>`).
        name: String,
        /// Source name in the local scope: a local shell key, or an
        /// imported dep's exposed-shell key.
        target: String,
        /// `Some(alias)` when re-exposing from a dependency
        /// (`expose shell <alias>.<target> as <name>`).
        qualifier: Option<String>,
    },
    EffectExposeVar {
        name: String,
        target: String,
        qualifier: Option<String>,
        /// Resolved value at expose time.
        value: String,
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
        /// The timeout that bounds this wait.
        effective: TimeoutValue,
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
        /// The timeout that fired.
        effective: TimeoutValue,
    },

    // Fail patterns
    FailPatternSet {
        pattern: String,
        is_regex: bool,
    },
    FailPatternCleared,
    FailPatternTriggered {
        pattern: String,
        is_regex: bool,
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
        timeout: TimeoutValue,
        previous: TimeoutValue,
    },

    // Values
    VarLet {
        name: String,
        value: String,
    },
    VarAssign {
        name: String,
        value: String,
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
    /// Pure string-match. Today: marker `expr ? ^pat$` conditions.
    /// `result` is the matched substring (`$0`) or `""` on no match.
    /// `captures` mirrors shell-buffer match captures.
    PureMatch {
        match_kind: super::span::MatchKind,
        value: String,
        pattern: String,
        result: String,
        captures: HashMap<String, String>,
    },
    /// Pure variable read: a bare-var expression resolved against the
    /// active scope or environment. `value` is the resolved string
    /// (`""` when the variable is undefined). Read counterpart to
    /// `var-let` / `var-assign`.
    VarRead {
        name: String,
        value: String,
    },
    /// Final truthy/falsy evaluation of a marker condition. Carries
    /// the shape-specific payload (Unconditional / Bare / Eq / Regex)
    /// and the `met` outcome that determined the marker's decision.
    /// Emitted as the last event inside a `marker-eval` span.
    BoolCheck {
        evaluation: super::span::MarkerEvalDetail,
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

#[cfg(test)]
mod pure_match_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pure_match_event_kind_serialises() {
        let mut caps = HashMap::new();
        caps.insert("0".to_string(), "abc".to_string());
        let k = EventKind::PureMatch {
            match_kind: super::super::span::MatchKind::Regex,
            value: "abc".into(),
            pattern: "^a.c$".into(),
            result: "abc".into(),
            captures: caps,
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["kind"], serde_json::json!("pure-match"));
        assert_eq!(v["match_kind"], serde_json::json!("regex"));
        assert_eq!(v["result"], serde_json::json!("abc"));
    }
}
