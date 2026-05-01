//! Compute the set of breakpointable lines for a selected test.
//!
//! Granularity is statement-level: a breakpoint pauses *between* IR
//! statements. Expression evaluation (interpolation trees, function
//! call argument resolution) is atomic in both pure and impure
//! contexts, so we don't descend into expressions.
//!
//! Walked from `reachable_from_test`'s output:
//! - the selected test's body (test items + nested shell/cleanup blocks)
//! - each reachable impure fn body (`Vec<IrShellStmt>`)
//! - each reachable pure fn body (`Vec<IrPureStmt>`) — sequenced by
//!   `eval_body` in `relux_ir::evaluator`, so each statement is a real
//!   pause point
//! - each reachable effect's body (effect items + nested blocks)

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;

use relux_core::table::FileId;
use relux_ir::IrCleanupBlock;
use relux_ir::IrEffect;
use relux_ir::IrEffectItem;
use relux_ir::IrFn;
use relux_ir::IrNode;
use relux_ir::IrPureFn;
use relux_ir::IrPureStmt;
use relux_ir::IrShellBlock;
use relux_ir::IrShellStmt;
use relux_ir::IrTest;
use relux_ir::IrTestItem;
use relux_ir::Suite;
use relux_ir::reachable::Reachable;

/// Compute breakpointable lines for the reachable IR of a selected test.
/// Keys are suite-relative filenames (matching the wire form on
/// `PreRunSource` entries).
pub fn compute_breakpointable_lines(
    suite: &Suite,
    relux_dir: &Path,
    reachable: &Reachable,
    selected_test: &IrTest,
) -> HashMap<String, BTreeSet<usize>> {
    let mut acc: HashMap<String, BTreeSet<usize>> = HashMap::new();

    walk_test(selected_test, suite, relux_dir, &mut acc);

    for fn_id in &reachable.fns {
        if let Some(Ok(IrFn::UserDefined { body, .. })) = suite.tables.fns.get(fn_id) {
            walk_shell_stmts(body, suite, relux_dir, &mut acc);
        }
        if let Some(Ok(IrPureFn::UserDefined { body, .. })) = suite.tables.pure_fns.get(fn_id) {
            walk_pure_stmts(body, suite, relux_dir, &mut acc);
        }
    }

    for effect_id in &reachable.effects {
        if let Some(Ok(effect)) = suite.tables.effects.get(effect_id) {
            walk_effect(effect, suite, relux_dir, &mut acc);
        }
    }

    acc
}

fn walk_test(
    test: &IrTest,
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    for item in test.body() {
        match item {
            IrTestItem::Start { span, .. } | IrTestItem::Let { span, .. } => {
                record(span, suite, relux_dir, acc);
            }
            IrTestItem::Shell { block, .. } => walk_shell_block(block, suite, relux_dir, acc),
            IrTestItem::Cleanup { block, .. } => walk_cleanup_block(block, suite, relux_dir, acc),
            IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => {}
        }
    }
}

fn walk_effect(
    effect: &IrEffect,
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    for item in effect.body() {
        match item {
            IrEffectItem::Start { span, .. } | IrEffectItem::Let { span, .. } => {
                record(span, suite, relux_dir, acc);
            }
            IrEffectItem::Shell { block, .. } => walk_shell_block(block, suite, relux_dir, acc),
            IrEffectItem::Cleanup { block, .. } => walk_cleanup_block(block, suite, relux_dir, acc),
            IrEffectItem::Comment { .. }
            | IrEffectItem::Expect { .. }
            | IrEffectItem::Expose { .. } => {}
        }
    }
}

fn walk_shell_block(
    block: &IrShellBlock,
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    walk_shell_stmts(block.body(), suite, relux_dir, acc);
}

fn walk_cleanup_block(
    block: &IrCleanupBlock,
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    walk_shell_stmts(block.body(), suite, relux_dir, acc);
}

fn walk_shell_stmts(
    stmts: &[IrShellStmt],
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    for stmt in stmts {
        // Skip comments; every other variant is a runtime-stepping kind.
        if matches!(stmt, IrShellStmt::Comment { .. }) {
            continue;
        }
        record(IrNode::span(stmt), suite, relux_dir, acc);
    }
}

fn walk_pure_stmts(
    stmts: &[IrPureStmt],
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    for stmt in stmts {
        if matches!(stmt, IrPureStmt::Comment { .. }) {
            continue;
        }
        record(IrNode::span(stmt), suite, relux_dir, acc);
    }
}

fn record(
    span: &relux_core::diagnostics::IrSpan,
    suite: &Suite,
    relux_dir: &Path,
    acc: &mut HashMap<String, BTreeSet<usize>>,
) {
    let Some(sf) = suite.tables.sources.get(span.file()) else {
        return;
    };
    let line = sf.line_at(span.span().start());
    let filename = suite_relative(span.file(), relux_dir);
    acc.entry(filename).or_default().insert(line);
}

fn suite_relative(file_id: &FileId, relux_dir: &Path) -> String {
    file_id
        .path()
        .strip_prefix(relux_dir)
        .unwrap_or(file_id.path())
        .to_string_lossy()
        .into_owned()
}
