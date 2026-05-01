use std::sync::Arc;

use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;

use super::super::message::Breakpoint;
use super::super::message::BreakpointListRequest;
use super::super::message::BreakpointListResponse;
use super::super::message::BreakpointResetRequest;
use super::super::message::BreakpointResetResponse;
use super::super::message::BreakpointSetRequest;
use super::super::message::BreakpointSetResponse;
use super::super::message::BreakpointUnsetRequest;
use super::super::message::BreakpointUnsetResponse;
use crate::protocol::Context;
use crate::protocol::error_code;
use crate::protocol::state::PreRunInner;
use crate::protocol::state::breakpointable;

pub async fn breakpoint_set(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let req: BreakpointSetRequest = params.parse()?;
    let mut pre_run = ctx.pre_run.lock().await;
    let inner = require_pre_run(&mut pre_run)?;

    if !breakpointable(inner, &req.filename, req.line) {
        return Err(ErrorObjectOwned::owned(
            error_code::BREAKPOINT_INVALID,
            format!("breakpoint not allowed at {}:{}", req.filename, req.line),
            None::<()>,
        ));
    }

    inner
        .breakpoints
        .entry(req.filename)
        .or_default()
        .insert(req.line);

    let breakpoint = Breakpoint::builder().line(req.line).build();
    let response = BreakpointSetResponse::builder()
        .breakpoint(breakpoint)
        .build();
    Ok(serde_json::to_value(response).unwrap())
}

pub async fn breakpoint_unset(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let req: BreakpointUnsetRequest = params.parse()?;
    let mut pre_run = ctx.pre_run.lock().await;
    let inner = require_pre_run(&mut pre_run)?;

    if let Some(lines) = inner.breakpoints.get_mut(&req.filename) {
        lines.remove(&req.line);
        if lines.is_empty() {
            inner.breakpoints.remove(&req.filename);
        }
    }

    Ok(serde_json::to_value(BreakpointUnsetResponse::default()).unwrap())
}

pub async fn breakpoint_reset(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let _req: BreakpointResetRequest = params.parse().unwrap_or_default();
    let mut pre_run = ctx.pre_run.lock().await;
    let inner = require_pre_run(&mut pre_run)?;

    inner.breakpoints.clear();

    Ok(serde_json::to_value(BreakpointResetResponse::default()).unwrap())
}

pub async fn breakpoint_list(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let _req: BreakpointListRequest = params.parse().unwrap_or_default();
    let mut pre_run = ctx.pre_run.lock().await;
    let inner = require_pre_run(&mut pre_run)?;

    let breakpoints = inner
        .breakpoints
        .iter()
        .map(|(filename, lines)| {
            let bps = lines
                .iter()
                .map(|&line| Breakpoint::builder().line(line).build())
                .collect();
            (filename.clone(), bps)
        })
        .collect();
    let response = BreakpointListResponse::builder()
        .breakpoints(breakpoints)
        .build();
    Ok(serde_json::to_value(response).unwrap())
}

/// Defensive guard for handlers that need a populated pre-run slot.
/// Until the cross-cutting stage filter exists, returns JSON-RPC's
/// standard `Invalid Request` rather than a relux-specific error code.
fn require_pre_run(
    slot: &mut Option<Box<PreRunInner>>,
) -> Result<&mut PreRunInner, ErrorObjectOwned> {
    slot.as_deref_mut().ok_or_else(|| {
        ErrorObjectOwned::owned(
            INVALID_REQUEST_CODE,
            "no test selected; call test/select first",
            None::<()>,
        )
    })
}
