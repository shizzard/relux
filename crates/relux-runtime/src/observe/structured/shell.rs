use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ShellRecord {
    #[serde(with = "super::ts_duration_ms")]
    #[ts(as = "f64")]
    pub spawn_ts: Duration,
    #[serde(with = "super::ts_duration_ms_opt")]
    #[ts(as = "Option<f64>")]
    pub terminate_ts: Option<Duration>,
    pub command: String,
    pub alias_of: Option<String>,
}
