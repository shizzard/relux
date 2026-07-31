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

/// Per-pattern descriptor for a `MultiMatch*` event. Carries the pattern's
/// source text and whether it is a regex or literal. The matched substring
/// and offsets live on the corresponding `BufferEventKind::Matched` event
/// referenced by `MultiMatchPatternDone.buffer_seq` - they are not
/// duplicated here.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct MultiMatchPattern {
    pub pattern: String,
    pub is_regex: bool,
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

    // Effect exposes - emitted at the end of effect setup, one per
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

    // Matching - buffer_seq references the corresponding buffer_events entry.
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
        /// `None` for fail-pattern hits - they observe without advancing the
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
    /// Pure string-match attempt - emitted before the match runs.
    /// `value` is the haystack inline (a shell match reads the buffer).
    PureMatchStart {
        value: String,
        pattern: String,
        is_regex: bool,
    },
    /// Pure string-match success. `matched` is the whole-match substring
    /// (`$0` / the literal needle); `captures` mirrors shell-buffer captures.
    PureMatchDone {
        matched: String,
        captures: HashMap<String, String>,
    },
    /// Pure string-match failure (no match). Empty payload; the preceding
    /// `PureMatchStart` in the same span carries value/pattern.
    PureMatchFailed {},
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

    // External interruption observed by the VM. Tagged with the reason
    // (test-timeout, suite-timeout, fail-fast, sigint). Emitted on the
    // span the VM was in when it noticed `cancel.is_cancelled()`.
    Cancelled {
        reason: CancelReasonRecord,
    },

    // --- Multimatch (R014) -------------------------------
    //
    // `<{ ?... =... }` and timed variants record this set of events:
    //   Success: MultiMatchStart + N MultiMatchPatternDone (in completion
    //            order, not source order) + MultiMatchDone.
    //   Timeout: MultiMatchStart + 0..N MultiMatchPatternDone +
    //            MultiMatchTimeout.
    //   Fail-pattern abort: MultiMatchStart + 0..N MultiMatchPatternDone,
    //            then the existing FailPatternTriggered path takes over.
    MultiMatchStart {
        /// The block-level timeout that bounds this wait.
        effective: TimeoutValue,
        /// All patterns in source order. Indices into this vec are
        /// referenced by `MultiMatchPatternDone.index` and
        /// `MultiMatchTimeout.unmatched`.
        patterns: Vec<MultiMatchPattern>,
    },
    MultiMatchPatternDone {
        /// Index into `MultiMatchStart.patterns`.
        index: usize,
        /// Time since block entry (not test start).
        #[serde(with = "super::ts_duration_ms")]
        #[ts(as = "f64")]
        elapsed: Duration,
        /// `EventSeq` of the per-pattern `BufferEventKind::Matched` event
        /// emitted under the same buf lock as this pattern's success.
        /// Single source of truth for matched text and absolute offsets.
        buffer_seq: EventSeq,
    },
    MultiMatchDone {
        /// `EventSeq` of the per-pattern `Matched` whose match ends
        /// farthest in the buffer. The viewer applies the block-end
        /// cursor advance by `len(before) + len(matched)` of this event.
        advance_to: EventSeq,
    },
    MultiMatchTimeout {
        /// Indices into `MultiMatchStart.patterns` that did not match
        /// before the block timeout fired.
        unmatched: Vec<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CancelReasonRecord {
    TestTimeout { duration_ms: u64 },
    SuiteTimeout { duration_ms: u64 },
    FailFast { trigger_test: String },
    Sigint,
}

impl CancelReasonRecord {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::TestTimeout { .. } => "test-timeout",
            Self::SuiteTimeout { .. } => "suite-timeout",
            Self::FailFast { .. } => "fail-fast",
            Self::Sigint => "sigint",
        }
    }
}

impl From<&crate::cancel::CancelReason> for CancelReasonRecord {
    fn from(r: &crate::cancel::CancelReason) -> Self {
        use crate::cancel::CancelReason;
        match r {
            CancelReason::TestTimeout { duration } => Self::TestTimeout {
                duration_ms: duration.as_millis() as u64,
            },
            CancelReason::SuiteTimeout { duration } => Self::SuiteTimeout {
                duration_ms: duration.as_millis() as u64,
            },
            CancelReason::FailFast { trigger_test } => Self::FailFast {
                trigger_test: trigger_test.clone(),
            },
            CancelReason::Sigint => Self::Sigint,
        }
    }
}

#[cfg(test)]
mod pure_match_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pure_match_trio_serialises() {
        let mut caps = HashMap::new();
        caps.insert("0".to_string(), "abc".to_string());
        let start = EventKind::PureMatchStart {
            value: "abc".into(),
            pattern: "^a.c$".into(),
            is_regex: true,
        };
        let done = EventKind::PureMatchDone {
            matched: "abc".into(),
            captures: caps,
        };
        let failed = EventKind::PureMatchFailed {};
        assert_eq!(
            serde_json::to_value(&start).unwrap()["kind"],
            serde_json::json!("pure-match-start")
        );
        assert_eq!(
            serde_json::to_value(&start).unwrap()["is_regex"],
            serde_json::json!(true)
        );
        assert_eq!(
            serde_json::to_value(&done).unwrap()["kind"],
            serde_json::json!("pure-match-done")
        );
        assert_eq!(
            serde_json::to_value(&failed).unwrap()["kind"],
            serde_json::json!("pure-match-failed")
        );
    }

    #[test]
    fn multimatch_start_event_kind_serialises() {
        let k = EventKind::MultiMatchStart {
            effective: TimeoutValue::Assertion {
                duration: "5s".into(),
                source: None,
            },
            patterns: vec![
                MultiMatchPattern {
                    pattern: "^ok$".into(),
                    is_regex: true,
                },
                MultiMatchPattern {
                    pattern: "batch complete".into(),
                    is_regex: false,
                },
            ],
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["kind"], serde_json::json!("multi-match-start"));
        assert_eq!(v["patterns"][0]["is_regex"], serde_json::json!(true));
        assert_eq!(
            v["patterns"][1]["pattern"],
            serde_json::json!("batch complete")
        );
    }

    #[test]
    fn multimatch_pattern_done_event_kind_serialises() {
        let k = EventKind::MultiMatchPatternDone {
            index: 1,
            elapsed: Duration::from_millis(42),
            buffer_seq: 17,
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["kind"], serde_json::json!("multi-match-pattern-done"));
        assert_eq!(v["index"], serde_json::json!(1));
        assert_eq!(v["buffer_seq"], serde_json::json!(17));
    }

    #[test]
    fn multimatch_done_event_kind_serialises() {
        let k = EventKind::MultiMatchDone { advance_to: 23 };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["kind"], serde_json::json!("multi-match-done"));
        assert_eq!(v["advance_to"], serde_json::json!(23));
    }

    #[test]
    fn multimatch_timeout_event_kind_serialises() {
        let k = EventKind::MultiMatchTimeout {
            unmatched: vec![0, 2],
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["kind"], serde_json::json!("multi-match-timeout"));
        assert_eq!(v["unmatched"], serde_json::json!([0, 2]));
    }
}
