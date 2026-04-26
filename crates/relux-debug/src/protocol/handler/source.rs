use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use relux_core::table::FileId;

use super::super::message::SourceGetRequest;
use super::super::message::SourceGetResponse;
use crate::protocol::Context;
use crate::protocol::error_code;

pub fn source_get(
    params: Params,
    ctx: &Context,
    _extensions: &Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let req: SourceGetRequest = params.parse()?;
    let file_id = FileId::new(ctx.relux_dir.join(&req.filename));

    let Some(sf) = ctx.suite.tables.sources.get(&file_id) else {
        return Err(ErrorObjectOwned::owned(
            error_code::FILE_NOT_FOUND,
            format!("file not found: {}", req.filename),
            None::<()>,
        ));
    };

    let response = SourceGetResponse::builder()
        .filename(req.filename)
        .content(sf.source.clone())
        .build();
    Ok(serde_json::to_value(response).unwrap())
}
