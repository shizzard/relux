use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::SourceLocation;
use super::event::EventSeq;
use super::event::TimeoutValue;
use super::span::SpanId;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct StackFrame {
    pub span: SpanId,
    /// Span-kind discriminator name (e.g. `"fn-call"`, `"shell-block"`).
    pub kind: String,
    /// Function name or effect name, when applicable.
    pub name: Option<String>,
    pub args: Vec<(String, String)>,
    /// User-supplied alias bound at start time (`start FX as Alias`).
    /// Only effect-setup frames carry one today.
    pub alias: Option<String>,
    pub location: Option<SourceLocation>,
}

/// Self-contained record of a test failure. Variant-specific fields plus
/// pre-computed convenience fields (`call_stack`, `buffer_tail`,
/// `vars_in_scope`) lifted from the `FailureContext` captured at the
/// failure site. `FailureContext::Vm` populates every field; the
/// `PreVm` variant (effect resolution, pre-VM init) carries only the
/// surrounding span and lands here as `event_seq: 0` / empty stack /
/// empty buffer tail - the artifact stays well-formed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FailureRecord {
    MatchTimeout {
        span: SpanId,
        event_seq: EventSeq,
        shell: String,
        pattern: String,
        /// The timeout that fired.
        effective: TimeoutValue,
        call_stack: Vec<StackFrame>,
        buffer_tail: String,
        vars_in_scope: Vec<(String, String)>,
    },
    FailPatternMatched {
        span: SpanId,
        event_seq: EventSeq,
        shell: String,
        pattern: String,
        matched_line: String,
        call_stack: Vec<StackFrame>,
        buffer_tail: String,
        vars_in_scope: Vec<(String, String)>,
    },
    ShellExited {
        span: SpanId,
        event_seq: EventSeq,
        shell: String,
        exit_code: Option<i32>,
        call_stack: Vec<StackFrame>,
        buffer_tail: String,
        vars_in_scope: Vec<(String, String)>,
    },
    Runtime {
        span: Option<SpanId>,
        event_seq: Option<EventSeq>,
        shell: Option<String>,
        message: String,
        call_stack: Vec<StackFrame>,
        vars_in_scope: Vec<(String, String)>,
    },
    MultiMatch {
        span: SpanId,
        event_seq: EventSeq,
        shell: String,
        /// All patterns in source order (mirrors `MultiMatchStart.patterns`).
        patterns: Vec<super::event::MultiMatchPattern>,
        /// Indices into `patterns` that matched before the block timed out.
        /// The unmatched indices are the complement.
        matched: Vec<usize>,
        /// The block-level timeout that fired.
        effective: TimeoutValue,
        call_stack: Vec<StackFrame>,
        buffer_tail: String,
        vars_in_scope: Vec<(String, String)>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::structured::event::MultiMatchPattern;

    #[test]
    fn failure_record_multimatch_round_trips_serde() {
        let original = FailureRecord::MultiMatch {
            span: 3,
            event_seq: 11,
            shell: "default".into(),
            patterns: vec![
                MultiMatchPattern {
                    pattern: "^a$".into(),
                    is_regex: true,
                },
                MultiMatchPattern {
                    pattern: "b".into(),
                    is_regex: false,
                },
            ],
            matched: vec![1],
            effective: TimeoutValue::Assertion {
                duration: "5s".into(),
                source: None,
            },
            call_stack: vec![],
            buffer_tail: "tail".into(),
            vars_in_scope: vec![("k".into(), "v".into())],
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"type\":\"multi-match\""), "json: {json}");
        let parsed: FailureRecord = serde_json::from_str(&json).unwrap();
        match parsed {
            FailureRecord::MultiMatch {
                matched, patterns, ..
            } => {
                assert_eq!(matched, vec![1usize]);
                assert_eq!(patterns.len(), 2);
            }
            other => panic!("expected MultiMatch, got {other:?}"),
        }
    }
}

/// Self-contained record of a test cancellation. Distinct from
/// `FailureRecord` because cancellation is not a failure: the test was
/// running fine when an external event (the per-test watchdog, the
/// suite-wide watchdog, fail-fast, SIGINT) cut it short.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct CancellationRecord {
    pub reason: super::event::CancelReasonRecord,
    pub span: Option<SpanId>,
    pub event_seq: Option<EventSeq>,
    pub shell: Option<String>,
    pub call_stack: Vec<StackFrame>,
}
