use std::collections::HashMap;
use std::sync::Arc;

use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use relux_core::table::FileId;
use relux_ir::IrNode;
use relux_ir::Plan;
use relux_ir::Suite;

use super::super::message::Definition;
use super::super::message::DefinitionKind;
use super::super::message::SessionInitRequest;
use super::super::message::SessionInitResponse;
use super::super::message::SessionState;
use super::super::message::SourceFileEntry;
use super::super::message::TestSelectState;
use super::common::end_line;
use super::common::start_line;
use crate::protocol::Context;
use crate::protocol::SessionStage;
use crate::protocol::error_code;

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
    // reconnecting) at any stage gets that stage's full state.
    let state = match &*ctx.session.lock().await {
        SessionStage::TestSelect => SessionState::TestSelect(
            TestSelectState::builder()
                .project(ctx.suite.name.clone())
                .files(collect_files(&ctx.suite, &ctx.relux_dir))
                .build(),
        ),
        SessionStage::PreRun { state } => SessionState::PreRun(state.clone()),
    };

    let result = SessionInitResponse::builder()
        .server("relux".to_string())
        .version(server_version.to_string())
        .state(state)
        .build();

    Ok(serde_json::to_value(result).unwrap())
}

/// Walk plans and emit one `SourceFileEntry` per file holding tests, with
/// the test definitions inside. `content` is `None` — the client fetches
/// source on demand via `source/get`. Functions and effects are not
/// included at this stage; they're delivered in pre-run state, scoped to
/// what's reachable from the selected test.
fn collect_files(suite: &Suite, relux_dir: &std::path::Path) -> Vec<SourceFileEntry> {
    let mut by_file: HashMap<FileId, Vec<Definition>> = HashMap::new();

    for plan in suite.plans.iter() {
        let span = IrNode::span(plan.meta());
        let file_id = span.file().clone();
        if let Some(sf) = suite.tables.sources.get(&file_id) {
            by_file.entry(file_id).or_default().push(
                Definition::builder()
                    .kind(DefinitionKind::Test)
                    .name(plan_display_name(plan))
                    .start_line(start_line(sf, span.span().start()))
                    .end_line(end_line(sf, span.span().end()))
                    .build(),
            );
        }
    }

    let mut entries: Vec<SourceFileEntry> = by_file
        .into_iter()
        .map(|(file_id, mut defs)| {
            defs.sort_by_key(|d| d.start_line);
            let filename = file_id
                .path()
                .strip_prefix(relux_dir)
                .unwrap_or(file_id.path())
                .to_string_lossy()
                .into_owned();
            SourceFileEntry::builder()
                .filename(filename)
                .definitions(defs)
                .build()
        })
        .collect();
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    entries
}

fn plan_display_name(plan: &Plan) -> String {
    plan.meta().name().to_string()
}
