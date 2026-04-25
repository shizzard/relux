use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;

use super::super::message::SessionInitRequest;
use super::super::message::SessionInitResponse;
use super::super::message::SessionState;
use super::super::message::TestSelectState;
use crate::protocol::Context;
use crate::protocol::error_code;

pub fn session_init(
    params: Params,
    ctx: &Context,
    _extensions: &Extensions,
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

    let result = SessionInitResponse::builder()
        .server("relux".to_string())
        .version(server_version.to_string())
        .state(SessionState::TestSelect(
            TestSelectState::builder()
                .project(ctx.suite.name.clone())
                .tests(ctx.suite.plans.len())
                .build(),
        ))
        .build();

    Ok(serde_json::to_value(result).unwrap())
}
