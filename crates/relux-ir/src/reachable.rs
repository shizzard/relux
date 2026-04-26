use std::collections::HashSet;

use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::FnId;

use super::block::IrCleanupBlock;
use super::block::IrShellBlock;
use super::effect::IrEffect;
use super::effect::IrEffectItem;
use super::effect::IrEffectStart;
use super::expr::IrExpr;
use super::expr::IrPureExpr;
use super::func::IrFn;
use super::func::IrPureFn;
use super::stmt::IrPureStmt;
use super::stmt::IrShellStmt;
use super::tables::Tables;
use super::test_def::IrTest;
use super::test_def::IrTestItem;

/// Set of definitions reachable from a starting test, by call/start chains.
/// Includes both user-defined and builtin functions; the consumer can filter.
#[derive(Debug, Default)]
pub struct Reachable {
    pub fns: HashSet<FnId>,
    pub effects: HashSet<EffectId>,
}

/// Compute the set of functions and effects transitively reachable from
/// `test` by following calls (`IrCallExpr` / `IrPureCallExpr`) and effect
/// starts (`IrEffectStart`). Bodies are pulled from `tables` on demand.
///
/// Builtins are recorded in `fns` but have no body to traverse.
pub fn reachable_from_test(test: &IrTest, tables: &Tables) -> Reachable {
    let mut acc = Reachable::default();
    walk_test(test, tables, &mut acc);
    acc
}

fn walk_test(test: &IrTest, tables: &Tables, acc: &mut Reachable) {
    for item in test.body() {
        match item {
            IrTestItem::Let { stmt, .. } => {
                if let Some(value) = stmt.value() {
                    walk_pure_expr(value, tables, acc);
                }
            }
            IrTestItem::Start { start, .. } => {
                walk_effect_start(start, tables, acc);
            }
            IrTestItem::Shell { block, .. } => {
                walk_shell_block(block, tables, acc);
            }
            IrTestItem::Cleanup { block, .. } => {
                walk_cleanup_block(block, tables, acc);
            }
            IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => {}
        }
    }
}

fn walk_effect_start(start: &IrEffectStart, tables: &Tables, acc: &mut Reachable) {
    let effect_id = start.effect().clone();
    let inserted = acc.effects.insert(effect_id.clone());
    for entry in start.overlay() {
        walk_pure_expr(entry.value(), tables, acc);
    }
    if !inserted {
        return;
    }
    if let Some(Ok(effect)) = tables.effects.get(&effect_id) {
        walk_effect(effect, tables, acc);
    }
}

fn walk_effect(effect: &IrEffect, tables: &Tables, acc: &mut Reachable) {
    for item in effect.body() {
        match item {
            IrEffectItem::Let { stmt, .. } => {
                if let Some(value) = stmt.value() {
                    walk_pure_expr(value, tables, acc);
                }
            }
            IrEffectItem::Start { start, .. } => {
                walk_effect_start(start, tables, acc);
            }
            IrEffectItem::Shell { block, .. } => {
                walk_shell_block(block, tables, acc);
            }
            IrEffectItem::Cleanup { block, .. } => {
                walk_cleanup_block(block, tables, acc);
            }
            IrEffectItem::Comment { .. }
            | IrEffectItem::Expect { .. }
            | IrEffectItem::Expose { .. } => {}
        }
    }
}

fn walk_shell_block(block: &IrShellBlock, tables: &Tables, acc: &mut Reachable) {
    walk_shell_stmts(block.body(), tables, acc);
}

fn walk_cleanup_block(block: &IrCleanupBlock, tables: &Tables, acc: &mut Reachable) {
    walk_shell_stmts(block.body(), tables, acc);
}

fn walk_shell_stmts(stmts: &[IrShellStmt], tables: &Tables, acc: &mut Reachable) {
    for stmt in stmts {
        match stmt {
            IrShellStmt::Let { stmt, .. } => {
                if let Some(value) = stmt.value() {
                    walk_expr(value, tables, acc);
                }
            }
            IrShellStmt::Assign { stmt, .. } => {
                walk_expr(stmt.value(), tables, acc);
            }
            IrShellStmt::Expr { expr, .. } => {
                walk_expr(expr, tables, acc);
            }
            IrShellStmt::Comment { .. }
            | IrShellStmt::Send { .. }
            | IrShellStmt::SendRaw { .. }
            | IrShellStmt::MatchRegex { .. }
            | IrShellStmt::MatchLiteral { .. }
            | IrShellStmt::TimedMatchRegex { .. }
            | IrShellStmt::TimedMatchLiteral { .. }
            | IrShellStmt::Timeout { .. }
            | IrShellStmt::FailRegex { .. }
            | IrShellStmt::FailLiteral { .. }
            | IrShellStmt::ClearFailPattern { .. }
            | IrShellStmt::BufferReset { .. } => {}
        }
    }
}

fn walk_pure_stmts(stmts: &[IrPureStmt], tables: &Tables, acc: &mut Reachable) {
    for stmt in stmts {
        match stmt {
            IrPureStmt::Let { stmt, .. } => {
                if let Some(value) = stmt.value() {
                    walk_pure_expr(value, tables, acc);
                }
            }
            IrPureStmt::Assign { stmt, .. } => {
                walk_pure_expr(stmt.value(), tables, acc);
            }
            IrPureStmt::Expr { expr, .. } => {
                walk_pure_expr(expr, tables, acc);
            }
            IrPureStmt::Comment { .. } => {}
        }
    }
}

fn walk_expr(expr: &IrExpr, tables: &Tables, acc: &mut Reachable) {
    if let IrExpr::Call { call, .. } = expr {
        let fn_id = call.resolved().clone();
        let inserted = acc.fns.insert(fn_id.clone());
        for arg in call.args() {
            walk_expr(arg, tables, acc);
        }
        if !inserted {
            return;
        }
        // Check both fn tables — pure fns can be called from impure contexts.
        if let Some(Ok(IrFn::UserDefined { body, .. })) = tables.fns.get(&fn_id) {
            walk_shell_stmts(body, tables, acc);
        } else if let Some(Ok(IrPureFn::UserDefined { body, .. })) = tables.pure_fns.get(&fn_id) {
            walk_pure_stmts(body, tables, acc);
        }
    }
}

fn walk_pure_expr(expr: &IrPureExpr, tables: &Tables, acc: &mut Reachable) {
    if let IrPureExpr::Call { call, .. } = expr {
        let fn_id = call.resolved().clone();
        let inserted = acc.fns.insert(fn_id.clone());
        for arg in call.args() {
            walk_pure_expr(arg, tables, acc);
        }
        if !inserted {
            return;
        }
        if let Some(Ok(IrPureFn::UserDefined { body, .. })) = tables.pure_fns.get(&fn_id) {
            walk_pure_stmts(body, tables, acc);
        }
    }
}
