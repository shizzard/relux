use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

// ─── source/get ───────────────────────────────────────────

/// Incoming request for `source/get`.
#[derive(Debug, Deserialize, Serialize)]
pub struct SourceGetRequest {
    pub filename: String,
}

/// Outgoing response for `source/get`.
#[derive(Debug, Builder, Serialize)]
pub struct SourceGetResponse {
    pub filename: String,
    pub content: String,
}
