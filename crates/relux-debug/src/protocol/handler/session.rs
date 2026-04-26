use std::sync::Arc;

use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;

use super::super::message::SessionInitRequest;
use super::super::message::SessionInitResponse;
use super::super::message::SessionState;
use crate::protocol::Context;
use crate::protocol::error_code;
use crate::protocol::state;
use crate::protocol::state::Stage;

pub async fn session_init(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let params: SessionInitRequest = params.parse()?;
    let server_version = relux_core::VERSION;

    if params.version != server_version {
        return Err(ErrorObjectOwned::owned(
            error_code::VERSION_MISMATCH,
            format!(
                "version mismatch: client {}, server {server_version}",
                params.version,
            ),
            None::<()>,
        ));
    }

    // Reflect the current session stage. A client connecting (or
    // reconnecting) at any stage gets that stage's full state, projected
    // from the corresponding `Context` slot.
    let stage = *ctx.stage.lock().await;
    let state = match stage {
        Stage::TestSelect => {
            let inner = ctx.test_select.lock().await;
            SessionState::TestSelect(state::project_test_select(&inner))
        }
        Stage::PreRun => {
            let guard = ctx.pre_run.lock().await;
            let inner = guard
                .as_ref()
                .expect("PreRun stage invariant: pre_run slot is Some");
            SessionState::PreRun(Box::new(state::project_pre_run(inner, &ctx.env)))
        }
    };

    let result = SessionInitResponse::builder()
        .server("relux".to_string())
        .version(server_version.to_string())
        .state(state)
        .build();

    Ok(serde_json::to_value(result).unwrap())
}
