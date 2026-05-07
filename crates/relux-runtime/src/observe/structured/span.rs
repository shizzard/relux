use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::SourceLocation;

pub type SpanId = u64;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SpanKind {
    Test,
    EffectSetup {
        effect: String,
        overlay: Vec<(String, String)>,
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
    },
}

impl SpanKind {
    /// Discriminator string matching the `serde(tag = "kind")` representation.
    /// Used by stack-frame rendering so that consumers see the same string
    /// they'd see in the JSON `spans` glossary.
    pub fn kind_str(&self) -> &'static str {
        match self {
            SpanKind::Test => "test",
            SpanKind::EffectSetup { .. } => "effect-setup",
            SpanKind::EffectCleanup { .. } => "effect-cleanup",
            SpanKind::ShellBlock { .. } => "shell-block",
            SpanKind::CleanupBlock => "cleanup-block",
            SpanKind::FnCall { .. } => "fn-call",
        }
    }

    /// Frame name and args used in stack-frame rendering. `name` is the
    /// effect or function name; `args` is the call args or effect overlay.
    pub fn frame_data(&self) -> (Option<String>, Vec<(String, String)>) {
        match self {
            SpanKind::Test | SpanKind::CleanupBlock => (None, Vec::new()),
            SpanKind::EffectSetup { effect, overlay } => (Some(effect.clone()), overlay.clone()),
            SpanKind::EffectCleanup { effect } => (Some(effect.clone()), Vec::new()),
            SpanKind::ShellBlock { shell } => (Some(shell.clone()), Vec::new()),
            SpanKind::FnCall { name, args } => (Some(name.clone()), args.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
