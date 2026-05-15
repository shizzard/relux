use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::event::EventSeq;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct BufferEvent {
    pub seq: EventSeq,
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub ts: Duration,
    pub shell: String,
    /// Stable identity for the shell. Buffer events always have a shell.
    pub shell_marker: String,
    #[serde(flatten)]
    pub kind: BufferEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BufferEventKind {
    Grew {
        data: String,
    },
    Matched {
        before: String,
        matched: String,
        after: String,
    },
    Reset {
        discarded: String,
    },
}
