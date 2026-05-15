use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::SourceLocation;

pub type SpanId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(rename_all = "kebab-case")]
pub enum FnCallKind {
    User,
    Bif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(rename_all = "kebab-case")]
pub enum MatchKind {
    Regex,
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerEvalKind {
    Skip,
    Run,
    Flaky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerEvalModifier {
    If,
    Unless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerEvalDecision {
    /// Marker's action did not apply.
    Pass,
    /// Marker's action applied — the kind tells which (skip / run /
    /// flaky).
    Mark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum MarkerEvalDetail {
    Unconditional,
    Bare {
        value: String,
        met: bool,
    },
    Eq {
        lhs: String,
        rhs: String,
        met: bool,
    },
    Regex {
        value: String,
        pattern: String,
        met: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SpanKind {
    Test {
        name: String,
    },
    EffectSetup {
        effect: String,
        overlay: Vec<(String, String)>,
        alias: Option<String>,
        /// Identity marker computed from the effect-instance dedup key.
        /// Same value on every `EffectSetup` for the same instance —
        /// the bootstrap span plus every dedup'd reuse share it.
        marker: String,
        /// `false` on the bootstrap span that runs the setup body.
        /// `true` on zero-duration spans emitted by dedup'd acquires.
        is_reuse: bool,
    },
    EffectCleanup {
        effect: String,
        alias: Option<String>,
        /// `EffectSetup` span this cleanup releases. Cleanups are parented
        /// directly under the test span (not the long-closed `EffectSetup`)
        /// so they stay well-ordered and reachable in the viewer; this
        /// back-reference preserves the link so consumers can resolve a
        /// cleanup shell's scope to the owning effect's vars.
        setup_span: SpanId,
        /// Identity marker, identical to the paired `EffectSetup`'s.
        marker: String,
        /// `false` on the final-release span that runs the cleanup body.
        /// `true` on zero-duration spans emitted by non-last releases.
        is_deferred: bool,
    },
    ShellBlock {
        shell: String,
    },
    CleanupBlock,
    FnCall {
        name: String,
        args: Vec<(String, String)>,
        result: Option<String>,
        callee_kind: FnCallKind,
        is_pure: bool,
    },
    /// Synthetic root span grouping per-test marker evaluations.
    /// Opened before the test root; carries no payload of its own.
    Markers,
    /// One marker evaluation. Child of `Markers`. Inner sink-op
    /// events (`var-read`, `interpolation`, `fn-call`, `pure-match`)
    /// describe how the condition was computed; a final `bool-check`
    /// event carries the truthy/falsy outcome that the `decision`
    /// summarises. `marker_kind` avoids collision with serde tag `kind`.
    MarkerEval {
        marker_kind: MarkerEvalKind,
        modifier: MarkerEvalModifier,
        decision: MarkerEvalDecision,
    },
}

impl SpanKind {
    /// Discriminator string matching the `serde(tag = "kind")` representation.
    /// Used by stack-frame rendering so that consumers see the same string
    /// they'd see in the JSON `spans` glossary.
    pub fn kind_str(&self) -> &'static str {
        match self {
            SpanKind::Test { .. } => "test",
            SpanKind::EffectSetup { .. } => "effect-setup",
            SpanKind::EffectCleanup { .. } => "effect-cleanup",
            SpanKind::ShellBlock { .. } => "shell-block",
            SpanKind::CleanupBlock => "cleanup-block",
            SpanKind::FnCall { .. } => "fn-call",
            SpanKind::Markers => "markers",
            SpanKind::MarkerEval { .. } => "marker-eval",
        }
    }

    /// Frame name and args used in stack-frame rendering. `name` is the
    /// test, effect, or function name; `args` is the call args or effect overlay.
    pub fn frame_data(&self) -> (Option<String>, Vec<(String, String)>) {
        match self {
            SpanKind::CleanupBlock => (None, Vec::new()),
            SpanKind::Test { name } => (Some(name.clone()), Vec::new()),
            SpanKind::EffectSetup {
                effect, overlay, ..
            } => (Some(effect.clone()), overlay.clone()),
            SpanKind::EffectCleanup { effect, .. } => (Some(effect.clone()), Vec::new()),
            SpanKind::ShellBlock { shell } => (Some(shell.clone()), Vec::new()),
            SpanKind::FnCall { name, args, .. } => (Some(name.clone()), args.clone()),
            SpanKind::Markers => (None, Vec::new()),
            SpanKind::MarkerEval { .. } => (None, Vec::new()),
        }
    }

    /// User-supplied alias bound at start time, if any (`start FX as Alias`).
    /// Only effect-setup / effect-cleanup frames carry one today.
    pub fn frame_alias(&self) -> Option<String> {
        match self {
            SpanKind::EffectSetup { alias, .. } => alias.clone(),
            SpanKind::EffectCleanup { alias, .. } => alias.clone(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_span_kind_serialises_as_kebab_kind() {
        let kind = SpanKind::Markers;
        let v = serde_json::to_value(&kind).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "markers" }));
    }

    #[test]
    fn marker_eval_span_kind_serialises_payload() {
        let kind = SpanKind::MarkerEval {
            marker_kind: MarkerEvalKind::Skip,
            modifier: MarkerEvalModifier::If,
            decision: MarkerEvalDecision::Mark,
        };
        let v = serde_json::to_value(&kind).unwrap();
        assert_eq!(v["kind"], serde_json::json!("marker-eval"));
        assert_eq!(v["marker_kind"], serde_json::json!("skip"));
        assert_eq!(v["modifier"], serde_json::json!("if"));
        assert_eq!(v["decision"], serde_json::json!("mark"));
    }

    #[test]
    fn fn_call_span_serializes_callee_kind_and_is_pure() {
        let span = SpanKind::FnCall {
            name: "trim".into(),
            args: vec![("$0".into(), "  hi  ".into())],
            result: Some("hi".into()),
            callee_kind: FnCallKind::Bif,
            is_pure: true,
        };
        let json = serde_json::to_value(&span).unwrap();
        assert_eq!(json["kind"], "fn-call");
        assert_eq!(json["name"], "trim");
        assert_eq!(json["callee_kind"], "bif");
        assert_eq!(json["is_pure"], true);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct Span {
    pub id: SpanId,
    #[serde(flatten)]
    pub kind: SpanKind,
    pub parent: Option<SpanId>,
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub start_ts: Duration,
    #[serde(with = "super::ts_duration_ms_opt")]
    #[ts(as = "Option<f64>")]
    pub end_ts: Option<Duration>,
    pub location: Option<SourceLocation>,
}
