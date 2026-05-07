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
