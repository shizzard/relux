//! Transitive marker-recording collection for a test.
//!
//! Relux is deterministic — no branching, no recursion. Every fn-call
//! and effect-start written in a test's body (or in any function or
//! effect transitively reachable from it) will execute. Marker
//! conditions on those functions and effects therefore apply to the
//! test in the same way they would if the test itself were marked.
//!
//! At test start, we walk the test's IR, collect every reachable
//! `FnId` and `EffectId`, deduplicate, and concatenate each
//! definition's `marker_recordings` (in deterministic visit order)
//! into a single list. The runtime replays those recordings flat
//! under the synthetic `markers` root span — no nesting under fn-call
//! or effect-setup spans (markers run before any test execution).
//!
//! Test-level recordings come first; effect-level and fn-level
//! recordings follow in pre-order traversal order. The set of
//! collected recordings is fully determined by the test's IR plus
//! the resolved fn/effect tables, so two runs of the same suite
//! produce identical marker traces.

use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::FnId;
use relux_ir::IrEffect;
use relux_ir::IrEffectItem;
use relux_ir::IrEffectStart;
use relux_ir::IrExpr;
use relux_ir::IrFn;
use relux_ir::IrPureExpr;
use relux_ir::IrPureFn;
use relux_ir::IrPureStmt;
use relux_ir::IrShellStmt;
use relux_ir::IrTest;
use relux_ir::IrTestItem;
use relux_ir::Tables;
use relux_ir::marker::MarkerRecording;
use std::collections::HashSet;

pub fn collect_test_marker_recordings(
    test: &IrTest,
    test_meta: &relux_ir::TestMeta,
    tables: &Tables,
) -> Vec<MarkerRecording> {
    let mut visitor = Visitor::new(tables);
    visitor
        .recordings
        .extend(test_meta.marker_recordings().iter().cloned());
    for start in test.starts() {
        visitor.visit_effect_start(start);
    }
    for item in test.body() {
        visitor.visit_test_item(item);
    }
    visitor.recordings
}

struct Visitor<'a> {
    tables: &'a Tables,
    seen_effects: HashSet<EffectId>,
    seen_fns: HashSet<FnId>,
    seen_pure_fns: HashSet<FnId>,
    recordings: Vec<MarkerRecording>,
}

impl<'a> Visitor<'a> {
    fn new(tables: &'a Tables) -> Self {
        Self {
            tables,
            seen_effects: HashSet::new(),
            seen_fns: HashSet::new(),
            seen_pure_fns: HashSet::new(),
            recordings: Vec::new(),
        }
    }

    fn visit_effect_start(&mut self, start: &IrEffectStart) {
        for entry in start.overlay() {
            self.visit_pure_expr(entry.value());
        }
        let effect_id = start.effect().clone();
        if !self.seen_effects.insert(effect_id.clone()) {
            return;
        }
        let Some(result) = self.tables.effects.get(&effect_id) else {
            return;
        };
        let Ok(effect) = result.as_ref() else {
            return;
        };
        let effect = effect.clone();
        self.recordings
            .extend(effect.marker_recordings().iter().cloned());
        self.visit_effect(&effect);
    }

    fn visit_effect(&mut self, effect: &IrEffect) {
        for start in effect.starts() {
            self.visit_effect_start(start);
        }
        for item in effect.body() {
            self.visit_effect_item(item);
        }
    }

    fn visit_test_item(&mut self, item: &IrTestItem) {
        match item {
            IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => {}
            IrTestItem::Start { start, .. } => self.visit_effect_start(start),
            IrTestItem::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr);
                }
            }
            IrTestItem::Shell { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt);
                }
            }
            IrTestItem::Cleanup { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt);
                }
            }
        }
    }

    fn visit_effect_item(&mut self, item: &IrEffectItem) {
        match item {
            IrEffectItem::Comment { .. }
            | IrEffectItem::Expect { .. }
            | IrEffectItem::Expose { .. } => {}
            IrEffectItem::Start { start, .. } => self.visit_effect_start(start),
            IrEffectItem::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr);
                }
            }
            IrEffectItem::Shell { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt);
                }
            }
            IrEffectItem::Cleanup { block, .. } => {
                for stmt in block.body() {
                    self.visit_shell_stmt(stmt);
                }
            }
        }
    }

    fn visit_shell_stmt(&mut self, stmt: &IrShellStmt) {
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
            | IrShellStmt::BufferReset { .. } => {}
            IrShellStmt::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_expr(expr);
                }
            }
            IrShellStmt::Assign { stmt, .. } => self.visit_expr(stmt.value()),
            IrShellStmt::Expr { expr, .. } => self.visit_expr(expr),
        }
    }

    fn visit_pure_stmt(&mut self, stmt: &IrPureStmt) {
        match stmt {
            IrPureStmt::Comment { .. } => {}
            IrPureStmt::Let { stmt, .. } => {
                if let Some(expr) = stmt.value() {
                    self.visit_pure_expr(expr);
                }
            }
            IrPureStmt::Assign { stmt, .. } => self.visit_pure_expr(stmt.value()),
            IrPureStmt::Expr { expr, .. } => self.visit_pure_expr(expr),
        }
    }

    fn visit_expr(&mut self, expr: &IrExpr) {
        match expr {
            IrExpr::String { .. }
            | IrExpr::Var { .. }
            | IrExpr::QualifiedVar { .. }
            | IrExpr::CaptureRef { .. } => {}
            IrExpr::Call { call, .. } => {
                for arg in call.args() {
                    self.visit_expr(arg);
                }
                self.visit_fn(call.resolved());
            }
        }
    }

    fn visit_pure_expr(&mut self, expr: &IrPureExpr) {
        match expr {
            IrPureExpr::String { .. } | IrPureExpr::Var { .. } => {}
            IrPureExpr::Call { call, .. } => {
                for arg in call.args() {
                    self.visit_pure_expr(arg);
                }
                self.visit_pure_fn(call.resolved());
            }
        }
    }

    fn visit_fn(&mut self, fn_id: &FnId) {
        if !self.seen_fns.insert(fn_id.clone()) {
            return;
        }
        let Some(result) = self.tables.fns.get(fn_id) else {
            return;
        };
        let Ok(ir_fn) = result.as_ref() else {
            return;
        };
        if let IrFn::UserDefined {
            body,
            marker_recordings,
            ..
        } = ir_fn.clone()
        {
            self.recordings.extend(marker_recordings);
            for stmt in body {
                self.visit_shell_stmt(&stmt);
            }
        }
    }

    fn visit_pure_fn(&mut self, fn_id: &FnId) {
        if !self.seen_pure_fns.insert(fn_id.clone()) {
            return;
        }
        let Some(result) = self.tables.pure_fns.get(fn_id) else {
            return;
        };
        let Ok(ir_fn) = result.as_ref() else {
            return;
        };
        if let IrPureFn::UserDefined {
            body,
            marker_recordings,
            ..
        } = ir_fn.clone()
        {
            self.recordings.extend(marker_recordings);
            for stmt in body {
                self.visit_pure_stmt(&stmt);
            }
        }
    }
}
