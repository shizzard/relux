use std::collections::HashSet;

use crate::Spanned as AstSpanned;

use super::effect_graph::EffectGraphBuilder;
use super::lower::{lower_as_pure_stmt, lower_spanned, lower_stmt, lower_test_def, sp};
use super::*;

pub(super) fn build_plan(
    file_id: FileId,
    test_def: &parser::AstTestDef,
    test_span: &parser::Span,
    scopes_by_file: &ir::IndexVec<FileId, Option<&ModuleScope>>,
    multiplier: f64,
) -> PlanResult {
    let mut graph_builder = EffectGraphBuilder::new(scopes_by_file, multiplier);

    // Resolve test-level needs
    let mut ir_needs = Vec::new();
    let test_needs: Vec<_> = test_def
        .body
        .iter()
        .filter_map(|item| {
            if let parser::AstTestItem::Need { decl: n, .. } = &item.node {
                Some((n.clone(), item.span))
            } else {
                None
            }
        })
        .collect();

    for (need, need_span) in &test_needs {
        if let Some(node_idx) = graph_builder.resolve_need(need, file_id, None) {
            let alias = need
                .alias
                .as_ref()
                .map(|a| ir::Spanned::new(a.node.clone(), sp(file_id, &a.span)));

            ir_needs.push(ir::Spanned::new(
                ir::TestNeed {
                    instance: node_idx,
                    alias,
                },
                sp(file_id, need_span),
            ));
        }
    }

    let mut errors: Vec<DiagnosticError> = graph_builder.diagnostics;

    let reachable_effects: HashSet<&str> = graph_builder
        .effects
        .iter()
        .map(|e| e.name.node.as_str())
        .collect();

    // Collect reachable functions (both impure and pure)
    let mut fns: FunctionRegistry<ir::Function, ir::FnId> = FunctionRegistry::new();
    let mut pure_fns: FunctionRegistry<ir::PureFunction, ir::PureFnId> = FunctionRegistry::new();
    collect_reachable_functions(
        test_def,
        file_id,
        scopes_by_file,
        &reachable_effects,
        multiplier,
        &mut fns,
        &mut pure_fns,
        &mut errors,
    );

    let scope = scopes_by_file[file_id].expect("scope missing for test file");
    let mut ctx = LoweringContext {
        file_id,
        scope,
        multiplier,
        errors: Vec::new(),
    };
    let test = lower_test_def(&mut ctx, test_def, test_span, ir_needs);
    errors.extend(ctx.errors);

    if errors.is_empty() {
        PlanResult::Ok {
            plan: Box::new(ir::Plan {
                functions: fns.entries,
                pure_functions: pure_fns.entries,
                effects: graph_builder.effects,
                effect_graph: ir::EffectGraph {
                    dag: graph_builder.dag,
                },
                test,
            }),
            warnings: Vec::new(),
        }
    } else {
        PlanResult::Err {
            errors,
            warnings: Vec::new(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_reachable_functions(
    test_def: &parser::AstTestDef,
    file_id: FileId,
    scopes_by_file: &ir::IndexVec<FileId, Option<&ModuleScope>>,
    reachable_effects: &HashSet<&str>,
    multiplier: f64,
    functions: &mut FunctionRegistry<ir::Function, ir::FnId>,
    pure_functions: &mut FunctionRegistry<ir::PureFunction, ir::PureFnId>,
    errors: &mut Vec<DiagnosticError>,
) {
    let scope = scopes_by_file[file_id].expect("scope missing for test file");

    // Walk test body to find all function calls
    let mut call_keys: Vec<FnKey> = Vec::new();

    // Walk test markers for pure function calls
    for marker in &test_def.markers {
        collect_calls_from_marker(&marker.node, &mut call_keys);
    }

    for item in &test_def.body {
        match &item.node {
            parser::AstTestItem::Shell { block, .. } => {
                collect_calls_from_stmts(&block.stmts, &mut call_keys);
            }
            parser::AstTestItem::Let { stmt: l, .. } => {
                if let Some(v) = &l.value {
                    collect_calls_from_expr(&v.node, &mut call_keys);
                }
            }
            parser::AstTestItem::Need { decl: need, .. } => {
                for entry in &need.overlay {
                    collect_calls_from_expr(&entry.node.value.node, &mut call_keys);
                }
            }
            _ => {}
        }
    }

    // Also walk effect shells, lets, need overlays, and markers.
    // These calls originate from the effect's home module, so carry its FileId.
    let mut effect_call_keys: Vec<(FnKey, Option<FileId>)> = Vec::new();
    for (name, located) in scope.effects.iter() {
        if !reachable_effects.contains(name as &str) {
            continue;
        }
        let effect_file = located.file;
        let effect_def = &located.def;
        let mut keys = Vec::new();
        for marker in &effect_def.markers {
            collect_calls_from_marker(&marker.node, &mut keys);
        }
        for item in &effect_def.body {
            match &item.node {
                parser::AstEffectItem::Shell { block, .. } => {
                    collect_calls_from_stmts(&block.stmts, &mut keys);
                }
                parser::AstEffectItem::Let { stmt: l, .. } => {
                    if let Some(v) = &l.value {
                        collect_calls_from_expr(&v.node, &mut keys);
                    }
                }
                parser::AstEffectItem::Need { decl: need, .. } => {
                    for entry in &need.overlay {
                        collect_calls_from_expr(&entry.node.value.node, &mut keys);
                    }
                }
                _ => {}
            }
        }
        effect_call_keys.extend(keys.into_iter().map(|k| (k, Some(effect_file))));
    }

    // Resolve each call — check impure functions first, then pure functions.
    // Each queue entry carries an optional FileId indicating the source module
    // whose scope should be used for resolution. None means use the test module's scope.
    // This ensures that when an imported function calls a sibling in its home module,
    // the sibling is resolved against the home module's scope, not the importer's.
    let mut queue: Vec<(FnKey, Option<FileId>)> =
        call_keys.into_iter().map(|k| (k, None)).collect();
    queue.extend(effect_call_keys);
    while let Some((key, source_file)) = queue.pop() {
        if functions.contains(&key) || pure_functions.contains(&key) {
            continue;
        }
        let resolve_scope =
            scopes_by_file[source_file.unwrap_or(file_id)].expect("scope missing for source file");
        if let Some(located) = resolve_scope.functions.get(&key) {
            let fn_file_id = located.file;
            let fn_def = &located.def;
            let mut child_keys = Vec::new();
            collect_calls_from_stmts(&fn_def.body, &mut child_keys);
            queue.extend(child_keys.into_iter().map(|k| (k, Some(fn_file_id))));

            let fn_scope = scopes_by_file[fn_file_id].expect("scope missing for function file");
            let body = {
                let mut ctx = LoweringContext {
                    file_id: fn_file_id,
                    scope: fn_scope,
                    multiplier,
                    errors: Vec::new(),
                };
                let stmts = fn_def
                    .body
                    .iter()
                    .filter_map(|s| lower_stmt(&mut ctx, &s.node, &s.span))
                    .collect();
                errors.extend(ctx.errors);
                stmts
            };

            functions.register(
                key.clone(),
                ir::Function {
                    name: lower_spanned(fn_file_id, key.name, &fn_def.name.span),
                    params: fn_def
                        .params
                        .iter()
                        .map(|p| lower_spanned(fn_file_id, p.node.clone(), &p.span))
                        .collect(),
                    body,
                    span: sp(fn_file_id, &fn_def.name.span),
                },
            );
        } else if let Some(located) = resolve_scope.pure_functions.get(&key) {
            let fn_file_id = located.file;
            let fn_def = &located.def;
            let mut child_keys = Vec::new();
            collect_calls_from_stmts(&fn_def.body, &mut child_keys);
            queue.extend(child_keys.into_iter().map(|k| (k, Some(fn_file_id))));

            let fn_scope = scopes_by_file[fn_file_id].expect("scope missing for function file");
            let body = {
                let mut ctx = LoweringContext {
                    file_id: fn_file_id,
                    scope: fn_scope,
                    multiplier,
                    errors: Vec::new(),
                };
                let stmts = fn_def
                    .body
                    .iter()
                    .filter_map(|s| lower_as_pure_stmt(&mut ctx, &s.node, &s.span))
                    .collect();
                errors.extend(ctx.errors);
                stmts
            };

            pure_functions.register(
                key.clone(),
                ir::PureFunction {
                    name: lower_spanned(fn_file_id, key.name, &fn_def.name.span),
                    params: fn_def
                        .params
                        .iter()
                        .map(|p| lower_spanned(fn_file_id, p.node.clone(), &p.span))
                        .collect(),
                    body,
                    span: sp(fn_file_id, &fn_def.name.span),
                },
            );
        }
    }
}

fn collect_calls_from_stmts(stmts: &[AstSpanned<parser::AstStmt>], keys: &mut Vec<FnKey>) {
    for stmt in stmts {
        match &stmt.node {
            parser::AstStmt::Expr { expr: e, .. } => {
                collect_calls_from_expr(e, keys);
            }
            parser::AstStmt::Let { stmt: l, .. } => {
                if let Some(v) = &l.value {
                    collect_calls_from_expr(&v.node, keys);
                }
            }
            parser::AstStmt::Assign { stmt: a, .. } => {
                collect_calls_from_expr(&a.value.node, keys);
            }
            // Operator statements have no function calls to collect
            parser::AstStmt::Comment { .. }
            | parser::AstStmt::Timeout { .. }
            | parser::AstStmt::FailRegex { .. }
            | parser::AstStmt::FailLiteral { .. }
            | parser::AstStmt::ClearFailPattern { .. }
            | parser::AstStmt::Send { .. }
            | parser::AstStmt::SendRaw { .. }
            | parser::AstStmt::MatchRegex { .. }
            | parser::AstStmt::MatchLiteral { .. }
            | parser::AstStmt::TimedMatchRegex { .. }
            | parser::AstStmt::TimedMatchLiteral { .. }
            | parser::AstStmt::BufferReset { .. } => {}
        }
    }
}

fn collect_calls_from_expr(expr: &parser::AstExpr, keys: &mut Vec<FnKey>) {
    match expr {
        parser::AstExpr::Call { call, .. } => {
            keys.push(FnKey {
                name: call.name.node.clone(),
                arity: call.args.len(),
            });
            for arg in &call.args {
                collect_calls_from_expr(&arg.node, keys);
            }
        }
        parser::AstExpr::String { .. }
        | parser::AstExpr::Var { .. }
        | parser::AstExpr::CaptureRef { .. } => {}
    }
}

fn collect_calls_from_marker(marker: &parser::AstMarkerDecl, keys: &mut Vec<FnKey>) {
    if let Some(cond) = &marker.condition {
        match &cond.body {
            parser::AstMarkerCondBody::Bare { expr, .. } => {
                collect_calls_from_expr(expr, keys);
            }
            parser::AstMarkerCondBody::Eq { lhs, rhs, .. } => {
                collect_calls_from_expr(lhs, keys);
                collect_calls_from_expr(rhs, keys);
            }
            parser::AstMarkerCondBody::Regex { expr, .. } => {
                collect_calls_from_expr(expr, keys);
            }
        }
    }
}
