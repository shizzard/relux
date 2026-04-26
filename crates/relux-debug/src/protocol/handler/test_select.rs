use std::collections::HashMap;
use std::sync::Arc;

use jsonrpsee::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::FnId;
use relux_core::table::FileId;
use relux_ir::IrFn;
use relux_ir::IrNode;
use relux_ir::IrPureFn;
use relux_ir::Plan;
use relux_ir::Suite;
use relux_ir::reachable::Reachable;
use relux_ir::reachable::reachable_from_test;

use super::super::message::Definition;
use super::super::message::DefinitionKind;
use super::super::message::Event;
use super::super::message::PreRunConfig;
use super::super::message::PreRunSource;
use super::super::message::PreRunTimeouts;
use super::super::message::SessionState;
use super::super::message::SourceFileEntry;
use super::super::message::TestSelectRequest;
use super::super::message::TestSelectResponse;
use crate::protocol::Context;
use crate::protocol::error_code;
use crate::protocol::state;
use crate::protocol::state::PreRunInner;
use crate::protocol::state::SelectedTest;
use crate::protocol::state::Stage;
use crate::protocol::state::end_line;
use crate::protocol::state::start_line;

pub async fn test_select(
    params: Params<'static>,
    ctx: Arc<Context>,
    _extensions: Extensions,
) -> Result<serde_json::Value, ErrorObjectOwned> {
    let req: TestSelectRequest = params.parse()?;
    let file_id = FileId::new(ctx.relux_dir.join(&req.filename));

    // Locate the matching plan: filename + test name.
    let plan = ctx
        .suite
        .plans
        .iter()
        .find(|p| p.meta().name() == req.test && IrNode::span(p.meta()).file() == &file_id);
    let Some(plan) = plan else {
        return Err(ErrorObjectOwned::owned(
            error_code::TEST_NOT_RUNNABLE,
            format!("test not found: {} in {}", req.test, req.filename),
            None::<()>,
        ));
    };
    let test = match plan {
        Plan::Runnable { test, .. } => test,
        Plan::Skipped { .. } | Plan::Invalid { .. } => {
            return Err(ErrorObjectOwned::owned(
                error_code::TEST_NOT_RUNNABLE,
                format!("test not runnable: {} in {}", req.test, req.filename),
                None::<()>,
            ));
        }
    };

    let reachable = reachable_from_test(test, &ctx.suite.tables);
    let source = build_pre_run_source(&ctx.suite, &ctx.relux_dir, plan, &file_id, &reachable);
    let config = build_pre_run_config(&ctx.relux_config, ctx.multiplier);
    let pre_run_inner = PreRunInner::builder()
        .selected(SelectedTest {
            filename: req.filename.clone(),
            test: req.test.clone(),
        })
        .source(source)
        .config(config)
        .build();

    // Lock order: stage before per-stage slot. Hold both while
    // transitioning so observers never see the marker advanced ahead of
    // its data.
    {
        let mut stage = ctx.stage.lock().await;
        let mut pre_run = ctx.pre_run.lock().await;
        *pre_run = Some(Box::new(pre_run_inner.clone()));
        *stage = Stage::PreRun;
    }

    // Subscribers may not exist yet (no events/subscribe). Drop the send
    // error in that case — the state is also reachable via `session/init`.
    let projected = state::project_pre_run(&pre_run_inner, &ctx.env);
    let _ = ctx.events.send(Event::StageChange {
        state: SessionState::PreRun(Box::new(projected)),
    });

    Ok(serde_json::to_value(TestSelectResponse::builder().build()).unwrap())
}

fn build_pre_run_config(cfg: &relux_core::config::ReluxConfig, multiplier: f64) -> PreRunConfig {
    PreRunConfig::builder()
        .shell(cfg.shell.command.clone())
        .prompt(cfg.shell.prompt.clone())
        .timeouts(
            PreRunTimeouts::builder()
                .match_timeout(humantime::format_duration(cfg.timeout.match_timeout).to_string())
                .test(humantime::format_duration(cfg.timeout.test).to_string())
                .suite(humantime::format_duration(cfg.timeout.suite).to_string())
                .build(),
        )
        .timeout_multiplier(multiplier)
        .build()
}

fn build_pre_run_source(
    suite: &Suite,
    relux_dir: &std::path::Path,
    test_plan: &Plan,
    test_file_id: &FileId,
    reachable: &Reachable,
) -> PreRunSource {
    // Group reachable function definitions by file.
    let mut fn_files: HashMap<FileId, Vec<Definition>> = HashMap::new();
    for fn_id in &reachable.fns {
        if let Some(def) = function_definition(suite, fn_id) {
            fn_files.entry(def.0).or_default().push(def.1);
        }
    }

    // Group reachable effect definitions by file.
    let mut effect_files: HashMap<FileId, Vec<Definition>> = HashMap::new();
    for effect_id in &reachable.effects {
        if let Some(def) = effect_definition(suite, effect_id) {
            effect_files.entry(def.0).or_default().push(def.1);
        }
    }

    // Test entry: a single SourceFileEntry for the test's file with one
    // test definition (the selected test).
    let test_definition = test_definition(suite, test_plan).expect("test plan source missing");
    let test_entry = make_entry(relux_dir, test_file_id, vec![test_definition]);

    let functions = files_to_entries(relux_dir, fn_files);
    let effects = files_to_entries(relux_dir, effect_files);

    PreRunSource::builder()
        .test(test_entry)
        .functions(functions)
        .effects(effects)
        .build()
}

fn function_definition(suite: &Suite, fn_id: &FnId) -> Option<(FileId, Definition)> {
    if let Some(Ok(IrFn::UserDefined { name, span, .. })) = suite.tables.fns.get(fn_id) {
        let sf = suite.tables.sources.get(span.file())?;
        return Some((
            span.file().clone(),
            Definition::builder()
                .kind(DefinitionKind::Function)
                .name(name.name().to_string())
                .start_line(start_line(sf, span.span().start()))
                .end_line(end_line(sf, span.span().end()))
                .build(),
        ));
    }
    if let Some(Ok(IrPureFn::UserDefined { name, span, .. })) = suite.tables.pure_fns.get(fn_id) {
        let sf = suite.tables.sources.get(span.file())?;
        return Some((
            span.file().clone(),
            Definition::builder()
                .kind(DefinitionKind::PureFunction)
                .name(name.name().to_string())
                .start_line(start_line(sf, span.span().start()))
                .end_line(end_line(sf, span.span().end()))
                .build(),
        ));
    }
    None
}

fn effect_definition(suite: &Suite, effect_id: &EffectId) -> Option<(FileId, Definition)> {
    let result = suite.tables.effects.get(effect_id)?;
    let effect = result.as_ref().ok()?;
    let span = IrNode::span(effect);
    let sf = suite.tables.sources.get(span.file())?;
    Some((
        span.file().clone(),
        Definition::builder()
            .kind(DefinitionKind::Effect)
            .name(effect.name().name().to_string())
            .start_line(start_line(sf, span.span().start()))
            .end_line(end_line(sf, span.span().end()))
            .build(),
    ))
}

fn test_definition(suite: &Suite, plan: &Plan) -> Option<Definition> {
    let span = IrNode::span(plan.meta());
    let sf = suite.tables.sources.get(span.file())?;
    Some(
        Definition::builder()
            .kind(DefinitionKind::Test)
            .name(plan.meta().name().to_string())
            .start_line(start_line(sf, span.span().start()))
            .end_line(end_line(sf, span.span().end()))
            .build(),
    )
}

fn make_entry(
    relux_dir: &std::path::Path,
    file_id: &FileId,
    mut defs: Vec<Definition>,
) -> SourceFileEntry {
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
}

fn files_to_entries(
    relux_dir: &std::path::Path,
    files: HashMap<FileId, Vec<Definition>>,
) -> Vec<SourceFileEntry> {
    let mut entries: Vec<SourceFileEntry> = files
        .into_iter()
        .map(|(file_id, defs)| make_entry(relux_dir, &file_id, defs))
        .collect();
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    entries
}
