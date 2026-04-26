use std::collections::HashMap;

use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use relux_core::table::FileId;
use relux_core::table::SourceFile;
use relux_ir::IrFn;
use relux_ir::IrNode;
use relux_ir::IrPureFn;
use relux_ir::Plan;
use relux_ir::Suite;

use super::super::message::Definition;
use super::super::message::DefinitionKind;
use super::super::message::SessionInitRequest;
use super::super::message::SessionInitResponse;
use super::super::message::SessionState;
use super::super::message::SourceFileEntry;
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
                .files(collect_files(&ctx.suite, &ctx.relux_dir))
                .build(),
        ))
        .build();

    Ok(serde_json::to_value(result).unwrap())
}

/// Walk all loaded sources and emit one `SourceFileEntry` per file with
/// every reachable test/function/effect definition. `content` is `None` —
/// the client fetches source on demand via `source/get`.
fn collect_files(suite: &Suite, relux_dir: &std::path::Path) -> Vec<SourceFileEntry> {
    // Collect definitions, keyed by file_id, so we can group per file.
    let mut by_file: HashMap<FileId, Vec<Definition>> = HashMap::new();

    // Tests come from plans (one plan per test).
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

    for (_id, fn_result) in suite.tables.fns.as_vec() {
        if let Ok(IrFn::UserDefined { name, span, .. }) = fn_result
            && let Some(sf) = suite.tables.sources.get(span.file())
        {
            by_file.entry(span.file().clone()).or_default().push(
                Definition::builder()
                    .kind(DefinitionKind::Function)
                    .name(name.name().to_string())
                    .start_line(start_line(sf, span.span().start()))
                    .end_line(end_line(sf, span.span().end()))
                    .build(),
            );
        }
    }

    for (_id, fn_result) in suite.tables.pure_fns.as_vec() {
        if let Ok(IrPureFn::UserDefined { name, span, .. }) = fn_result
            && let Some(sf) = suite.tables.sources.get(span.file())
        {
            by_file.entry(span.file().clone()).or_default().push(
                Definition::builder()
                    .kind(DefinitionKind::PureFunction)
                    .name(name.name().to_string())
                    .start_line(start_line(sf, span.span().start()))
                    .end_line(end_line(sf, span.span().end()))
                    .build(),
            );
        }
    }

    for (_id, effect_result) in suite.tables.effects.as_vec() {
        if let Ok(effect) = effect_result {
            let span = IrNode::span(effect);
            if let Some(sf) = suite.tables.sources.get(span.file()) {
                by_file.entry(span.file().clone()).or_default().push(
                    Definition::builder()
                        .kind(DefinitionKind::Effect)
                        .name(effect.name().name().to_string())
                        .start_line(start_line(sf, span.span().start()))
                        .end_line(end_line(sf, span.span().end()))
                        .build(),
                );
            }
        }
    }

    // Ensure files with no definitions (rare — empty files reachable via
    // imports) are still listed.
    for (file_id, _sf) in suite.tables.sources.as_vec() {
        by_file.entry(file_id).or_default();
    }

    // Emit, sorted by relative filename, with definitions sorted by start line.
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

fn start_line(sf: &SourceFile, byte_start: usize) -> usize {
    sf.line_at(byte_start)
}

/// `endLine` per `00-common.md`: line *after* the last line (1-based, exclusive).
fn end_line(sf: &SourceFile, byte_end: usize) -> usize {
    if byte_end == 0 {
        return 1;
    }
    sf.line_at(byte_end - 1) + 1
}

fn plan_display_name(plan: &Plan) -> String {
    plan.meta().name().to_string()
}
