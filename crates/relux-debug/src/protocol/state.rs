//! Session-stage data structures and wire projections.
//!
//! `Stage` is a flat marker — the bookkeeping that says "we're in
//! pre-run." Per-stage data lives in dedicated slots on `Context`
//! (`test_select`, `pre_run`, future `run`/`post_run`). This decouples
//! the stage marker from state lifetime, so backtracking transitions
//! (e.g. run → pre-run) don't lose data.
//!
//! Wire types (`TestSelectState`, `PreRunState`) are projections of
//! these inner structs, built on demand by `session/init` and stage
//! transition handlers.
//!
//! Lock order: `stage` before any per-stage slot.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;

use bon::Builder;
use relux_core::table::FileId;
use relux_core::table::SourceFile;
use relux_ir::IrNode;
use relux_ir::Plan;
use relux_ir::Suite;

use super::message::Breakpoint;
use super::message::Definition;
use super::message::DefinitionKind;
use super::message::PreRunConfig;
use super::message::PreRunSource;
use super::message::PreRunState;
use super::message::SourceFileEntry;
use super::message::TestSelectState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    TestSelect,
    PreRun,
}

/// Truth state for the test-select stage. Populated at session start;
/// today the file list is derived from the immutable suite and never
/// changes. Future mutable surface (filter, search, sort, suite-level
/// diagnostics) will live here.
#[derive(Debug, Clone, Builder)]
pub struct TestSelectInner {
    pub project: String,
    pub files: Vec<SourceFileEntry>,
}

/// Truth state for the pre-run stage. Built by `test/select` from the
/// resolved test plan. Cross-stage state that conceptually belongs to
/// pre-run (frozen flag) will be added here as fields, not as
/// separate `Context` slots.
#[derive(Debug, Clone, Builder)]
pub struct PreRunInner {
    pub selected: SelectedTest,
    pub source: PreRunSource,
    pub config: PreRunConfig,
    /// Currently set breakpoints, keyed by suite-relative filename.
    /// `BTreeMap`/`BTreeSet` give deterministic ordering for the
    /// projected wire form.
    #[builder(default)]
    pub breakpoints: BTreeMap<String, BTreeSet<usize>>,
    /// Internal — never serialized. Filled at `test/select` time by
    /// walking the reachable IR. Used by `breakpoint/set` to validate
    /// that a `(filename, line)` corresponds to a runtime statement.
    /// Keys are suite-relative filenames (matching the wire form on
    /// `PreRunSource` entries) so validation is a string-keyed lookup.
    #[builder(default)]
    pub breakpointable_lines: HashMap<String, BTreeSet<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTest {
    pub filename: String,
    pub test: String,
}

pub fn project_test_select(inner: &TestSelectInner) -> TestSelectState {
    TestSelectState::builder()
        .project(inner.project.clone())
        .files(inner.files.clone())
        .build()
}

pub fn project_pre_run(inner: &PreRunInner, env: &HashMap<String, String>) -> PreRunState {
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

    PreRunState::builder()
        .source(inner.source.clone())
        .env(env.clone())
        .config(inner.config.clone())
        .breakpoints(breakpoints)
        .build()
}

/// Returns true iff `line` is in the breakpointable-line set for the
/// given filename in this pre-run.
pub fn breakpointable(inner: &PreRunInner, filename: &str, line: usize) -> bool {
    inner
        .breakpointable_lines
        .get(filename)
        .is_some_and(|lines| lines.contains(&line))
}

/// Walk the suite's plans and emit one `SourceFileEntry` per file
/// holding tests, with the test definitions inside. `content` stays
/// `None` — the client fetches source on demand via `source/get`.
pub fn build_initial_test_select(suite: &Suite, relux_dir: &Path) -> TestSelectInner {
    let mut by_file: std::collections::HashMap<FileId, Vec<Definition>> =
        std::collections::HashMap::new();

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

    let mut files: Vec<SourceFileEntry> = by_file
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
    files.sort_by(|a, b| a.filename.cmp(&b.filename));

    TestSelectInner::builder()
        .project(suite.name.clone())
        .files(files)
        .build()
}

fn plan_display_name(plan: &Plan) -> String {
    plan.meta().name().to_string()
}

pub fn start_line(sf: &SourceFile, byte_start: usize) -> usize {
    sf.line_at(byte_start)
}

/// `endLine` per `00-common.md`: line *after* the last line (1-based, exclusive).
pub fn end_line(sf: &SourceFile, byte_end: usize) -> usize {
    if byte_end == 0 {
        return 1;
    }
    sf.line_at(byte_end - 1) + 1
}
