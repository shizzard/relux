use std::collections::HashMap;

use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

use super::Breakpoint;

// ─── breakpoint/set ───────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct BreakpointSetRequest {
    pub filename: String,
    pub line: usize,
}

#[derive(Debug, Builder, Serialize)]
pub struct BreakpointSetResponse {
    pub breakpoint: Breakpoint,
}

// ─── breakpoint/unset ─────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct BreakpointUnsetRequest {
    pub filename: String,
    pub line: usize,
}

#[derive(Debug, Builder, Serialize, Default)]
pub struct BreakpointUnsetResponse {}

// ─── breakpoint/reset ─────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct BreakpointResetRequest {}

#[derive(Debug, Builder, Serialize, Default)]
pub struct BreakpointResetResponse {}

// ─── breakpoint/list ──────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct BreakpointListRequest {}

#[derive(Debug, Builder, Serialize)]
pub struct BreakpointListResponse {
    pub breakpoints: HashMap<String, Vec<Breakpoint>>,
}
