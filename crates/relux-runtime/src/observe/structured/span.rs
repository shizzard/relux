use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::SourceLocation;

pub type SpanId = u64;

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
    },
    EffectCleanup {
        effect: String,
    },
    ShellBlock {
        shell: String,
    },
    CleanupBlock,
    FnCall {
        name: String,
        args: Vec<(String, String)>,
        result: Option<String>,
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
            SpanKind::EffectCleanup { effect } => (Some(effect.clone()), Vec::new()),
            SpanKind::ShellBlock { shell } => (Some(shell.clone()), Vec::new()),
            SpanKind::FnCall { name, args, .. } => (Some(name.clone()), args.clone()),
        }
    }

    /// User-supplied alias bound at start time, if any (`start FX as Alias`).
    /// Only effect-setup frames carry one today.
    pub fn frame_alias(&self) -> Option<String> {
        match self {
            SpanKind::EffectSetup { alias, .. } => alias.clone(),
            _ => None,
        }
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
