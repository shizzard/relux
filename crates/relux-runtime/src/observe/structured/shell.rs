use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct ShellRecord {
    /// Stable mnemonic identity (matches the key in `StructuredLog.shells`).
    pub marker: String,
    /// Spawn-time bare name (e.g. `inner`, `__cleanup`). Display layer
    /// may show qualified forms (`Db.inner`) derived from events.
    pub name: String,
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub spawn_ts: Duration,
    #[serde(with = "super::ts_duration_ms_opt")]
    #[ts(as = "Option<f64>")]
    pub terminate_ts: Option<Duration>,
    pub command: String,
}
