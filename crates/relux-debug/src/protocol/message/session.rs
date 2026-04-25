use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

// ─── session/init ─────────────────────────────────────────

/// Incoming request for `session/init`.
#[derive(Debug, Deserialize, Serialize)]
pub struct SessionInitRequest {
    pub client: String,
    pub version: String,
}

/// Outgoing response for `session/init`.
#[derive(Debug, Builder, Serialize)]
pub struct SessionInitResponse {
    pub server: String,
    pub version: String,
    pub state: SessionState,
}

// ─── State snapshots ──────────────────────────────────────

/// Stage-specific state snapshot. The `stage` field is the enum
/// discriminant — serde embeds it as an internal tag, so the
/// stage and state are always in sync.
///
/// Used in `session/init` response and `stage/change` events.
#[derive(Debug, Serialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum SessionState {
    TestSelect(TestSelectState),
}

/// State for the `test-select` stage.
#[derive(Debug, Builder, Serialize)]
pub struct TestSelectState {
    pub project: String,
    pub tests: usize,
}
