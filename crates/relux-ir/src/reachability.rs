//! Transitive marker-decision collection for a test.
//!
//! Relux is deterministic - no branching, no recursion. Every fn-call
//! and effect-start written in a test's body (or in any function or
//! effect transitively reachable from it) will execute. Marker
//! conditions on those functions and effects therefore apply to the
//! test in the same way they would if the test itself were marked.
//!
//! At resolve time, we walk the test's IR, collect every reachable
//! `FnId` and `EffectId`, deduplicate, and decide-or-reuse each
//! definition's `MarkerDecision` against `env`: if a decision was
//! already memoized for `(definition, StackHash(env.stack_hash()))` (by
//! an earlier test sharing the same stack, or an earlier reachable def
//! in this same walk), it is reused as-is; otherwise the definition's
//! cached lowered markers are decided against `env` and the result is
//! memoized before folding. The aggregate: the first (pre-order) skip
//! wins, flaky is the OR across every reachable definition, and
//! recordings concatenate in pre-order (test first) for replay.
//!
//! Test-level recordings come first; effect-level and fn-level
//! recordings follow in pre-order traversal order. The set of
//! collected recordings is fully determined by the test's IR plus the
//! resolved fn/effect tables and the env, so two runs of the same
//! suite against the same env produce identical marker traces.

use std::collections::HashSet;
use std::sync::Arc;

use relux_core::diagnostics::DefinitionRef;
use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::FnId;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::SkipReport;
use relux_core::pure::LayeredEnv;

use crate::IrEffect;
use crate::IrEffectItem;
use crate::IrEffectStart;
use crate::IrExpr;
use crate::IrFn;
use crate::IrPureExpr;
use crate::IrPureFn;
use crate::IrPureStmt;
use crate::IrShellStmt;
use crate::IrTest;
use crate::IrTestItem;
use crate::Tables;
use crate::TestMeta;
use crate::marker::MarkerRecording;
use crate::tables::StackHash;

/// Walk a test's reachable defs and decide-or-reuse their marker decisions
/// against `env`, memoizing each into `tables.marker_decisions` keyed by
/// `(definition, StackHash(env.stack_hash()))`. A skip on the test or any
/// reachable def skips the test; flaky is the OR; recordings concatenate in
/// pre-order (test first). Defs with no lowered-markers entry contribute
/// nothing (they had no markers). Propagates the first decision-time error
/// (e.g. an invalid interpolated regex) encountered during the walk.
pub fn collect_test_decision(
    test: &IrTest,
    test_meta: &TestMeta,
    tables: &Tables,
    env: &Arc<LayeredEnv>,
) -> Result<AggregatedDecision, LoweringBail> {
    let stack = StackHash(env.stack_hash());
    let fns = tables.pure_fns.clone();
    let mut v = Visitor::new(tables, env, stack, fns);
    v.take(test_meta.definition())?;
    for start in test.starts() {
        v.visit_effect_start(start)?;
    }
    for item in test.body() {
        v.visit_test_item(item)?;
    }
    Ok(v.finish())
}

/// Aggregated marker outcome for a test, folded from every reachable
/// definition's decision at a single env stack.
pub struct AggregatedDecision {
    pub skip: Option<SkipReport>,
    pub flaky: bool,
    pub recordings: Vec<MarkerRecording>,
}

struct Visitor<'a> {
    tables: &'a Tables,
    env: &'a Arc<LayeredEnv>,
    stack: StackHash,
    fns: crate::PureFnTable,
    seen_effects: HashSet<EffectId>,
    seen_fns: HashSet<FnId>,
    seen_pure_fns: HashSet<FnId>,
    skip: Option<SkipReport>,
    flaky: bool,
    recordings: Vec<MarkerRecording>,
}

impl<'a> Visitor<'a> {
    fn new(
        tables: &'a Tables,
        env: &'a Arc<LayeredEnv>,
        stack: StackHash,
        fns: crate::PureFnTable,
    ) -> Self {
        Self {
            tables,
            env,
            stack,
            fns,
            seen_effects: HashSet::new(),
            seen_fns: HashSet::new(),
            seen_pure_fns: HashSet::new(),
            skip: None,
            flaky: false,
            recordings: Vec::new(),
        }
    }

    fn finish(self) -> AggregatedDecision {
        AggregatedDecision {
            skip: self.skip,
            flaky: self.flaky,
            recordings: self.recordings,
        }
    }

    /// Fold `def`'s decision at `self.stack` into the aggregate: reuse the
    /// memoized decision if present, otherwise decide `def`'s lowered
    /// markers (if any) against `self.env` and memoize the result before
    /// folding. Defs with no lowered-markers entry contribute nothing. The
    /// first (pre-order) skip wins; flaky is OR'd; recordings concatenate.
    fn take(&mut self, def: &DefinitionRef) -> Result<(), LoweringBail> {
        let key = (def.clone(), self.stack);
        let decision = match self.tables.marker_decisions.get(&key) {
            Some(decision) => decision.clone(),
            None => {
                let Some(lowered) = self.tables.lowered_markers.get(def) else {
                    return Ok(());
                };
                let decision =
                    crate::marker::decide_markers(lowered, def.clone(), self.env, &self.fns)?;
                self.tables.marker_decisions.insert(key, decision.clone());
                decision
            }
        };
        if self.skip.is_none() {
            self.skip = decision.skip.clone();
        }
        self.flaky = self.flaky || decision.flaky;
        self.recordings.extend(decision.recordings.iter().cloned());
        Ok(())
    }

    fn visit_effect_start(&mut self, start: &IrEffectStart) -> Result<(), LoweringBail> {
        for entry in start.overlay() {
            self.visit_pure_expr(entry.value())?;
        }
        let effect_id = start.effect().clone();
        if !self.seen_effects.insert(effect_id.clone()) {
            return Ok(());
        }
        let Some(result) = self.tables.effects.get(&effect_id) else {
            return Ok(());
        };
        let Ok(effect) = result.as_ref() else {
            return Ok(());
        };
        let effect = effect.clone();
        self.take(&DefinitionRef::Effect(effect_id))?;
        self.visit_effect(&effect)
    }

    fn visit_effect(&mut self, effect: &IrEffect) -> Result<(), LoweringBail> {
        for start in effect.starts() {
            self.visit_effect_start(start)?;
        }
        for item in effect.body() {
            self.visit_effect_item(item)?;
        }
        Ok(())
    }

    fn visit_test_item(&mut self, item: &IrTestItem) -> Result<(), LoweringBail> {
        match item {
            IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => {}
            IrTestItem::Start { start, .. } => self.visit_effect_start(start)?,
            IrTestItem::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr)?;
                }
            }
            IrTestItem::Shell { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt)?;
                }
            }
            IrTestItem::Cleanup { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt)?;
                }
            }
        }
        Ok(())
    }

    fn visit_effect_item(&mut self, item: &IrEffectItem) -> Result<(), LoweringBail> {
        match item {
            IrEffectItem::Comment { .. }
            | IrEffectItem::Expect { .. }
            | IrEffectItem::Expose { .. } => {}
            IrEffectItem::Start { start, .. } => self.visit_effect_start(start)?,
            IrEffectItem::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr)?;
                }
            }
            IrEffectItem::Shell { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt)?;
                }
            }
            IrEffectItem::Cleanup { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt)?;
                }
            }
        }
        Ok(())
    }

    fn visit_shell_stmt(&mut self, stmt: &IrShellStmt) -> Result<(), LoweringBail> {
        match stmt {
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
            | IrShellStmt::MultiMatch { .. }
            | IrShellStmt::BufferReset { .. } => {}
            IrShellStmt::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_expr(expr)?;
                }
            }
            IrShellStmt::Assign { stmt, .. } => self.visit_expr(stmt.value())?,
            IrShellStmt::Expr { expr, .. } => self.visit_expr(expr)?,
            IrShellStmt::PureMatch { lhs, .. } => self.visit_expr(lhs)?,
        }
        Ok(())
    }

    fn visit_pure_stmt(&mut self, stmt: &IrPureStmt) -> Result<(), LoweringBail> {
        match stmt {
            IrPureStmt::Comment { .. } => {}
            IrPureStmt::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr)?;
                }
            }
            IrPureStmt::Assign { stmt, .. } => self.visit_pure_expr(stmt.value())?,
            IrPureStmt::Expr { expr, .. } => self.visit_pure_expr(expr)?,
            IrPureStmt::PureMatch { lhs, .. } => self.visit_pure_expr(lhs)?,
        }
        Ok(())
    }

    fn visit_expr(&mut self, expr: &IrExpr) -> Result<(), LoweringBail> {
        match expr {
            IrExpr::String { .. }
            | IrExpr::Var { .. }
            | IrExpr::QualifiedVar { .. }
            | IrExpr::CaptureRef { .. } => {}
            IrExpr::Call { call, .. } => {
                for arg in call.args() {
                    self.visit_expr(arg)?;
                }
                self.visit_fn(call.resolved())?;
            }
        }
        Ok(())
    }

    fn visit_pure_expr(&mut self, expr: &IrPureExpr) -> Result<(), LoweringBail> {
        match expr {
            IrPureExpr::String { .. } | IrPureExpr::Var { .. } | IrPureExpr::Capture { .. } => {}
            IrPureExpr::Call { call, .. } => {
                for arg in call.args() {
                    self.visit_pure_expr(arg)?;
                }
                self.visit_pure_fn(call.resolved())?;
            }
        }
        Ok(())
    }

    fn visit_fn(&mut self, fn_id: &FnId) -> Result<(), LoweringBail> {
        if !self.seen_fns.insert(fn_id.clone()) {
            return Ok(());
        }
        let Some(result) = self.tables.fns.get(fn_id) else {
            return Ok(());
        };
        let Ok(ir_fn) = result.as_ref() else {
            return Ok(());
        };
        if let IrFn::UserDefined { body, .. } = ir_fn.clone() {
            self.take(&DefinitionRef::Fn(fn_id.clone()))?;
            for stmt in body {
                self.visit_shell_stmt(&stmt)?;
            }
        }
        Ok(())
    }

    fn visit_pure_fn(&mut self, fn_id: &FnId) -> Result<(), LoweringBail> {
        if !self.seen_pure_fns.insert(fn_id.clone()) {
            return Ok(());
        }
        let Some(result) = self.tables.pure_fns.get(fn_id) else {
            return Ok(());
        };
        let Ok(ir_fn) = result.as_ref() else {
            return Ok(());
        };
        if let IrPureFn::UserDefined { body, .. } = ir_fn.clone() {
            self.take(&DefinitionRef::Fn(fn_id.clone()))?;
            for stmt in body {
                self.visit_pure_stmt(&stmt)?;
            }
        }
        Ok(())
    }
}
