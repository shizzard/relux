use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

// ─── test/select ──────────────────────────────────────────

/// Incoming request for `test/select`.
#[derive(Debug, Deserialize, Serialize)]
pub struct TestSelectRequest {
    /// Path relative to the suite's `relux/` directory.
    pub filename: String,
    /// Test name (matches `plan.meta().name()`).
    pub test: String,
}

/// Outgoing response for `test/select` — basic ack. The actual new state
/// is delivered out-of-band via a `stage/change` event.
#[derive(Debug, Builder, Serialize)]
pub struct TestSelectResponse {}
