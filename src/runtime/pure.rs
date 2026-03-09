use std::collections::HashMap;
use std::sync::Arc;

use crate::dsl::resolver::ir::{self, Span, Spanned, StringExpr, StringPart};
use async_trait::async_trait;

use crate::runtime::bifs::PureContext;
use crate::runtime::progress::ProgressEvent;
use crate::runtime::result::Failure;
use crate::runtime::vars::Env;
use crate::runtime::{Callable, CodeServer};

// ─── Lightweight PureContext for non-shell evaluation ────────

/// A minimal PureContext for evaluating pure expressions outside shell blocks
/// (test/effect scope lets, overlay values).
pub struct SimplePureContext;

#[async_trait]
impl PureContext for SimplePureContext {
    fn emit_progress(&self, _event: ProgressEvent) {}
    async fn emit_log(&mut self, _message: String) {}
}

// ─── String Interpolation ───────────────────────────────────
// Pure-context interpolation: looks up variables in local vars,
// then env. No test scope, no captures, no overlay.

fn interpolate_pure(expr: &StringExpr, vars: &HashMap<String, String>, env: &Env) -> String {
    let mut out = String::new();
    for part in &expr.parts {
        match &part.node {
            StringPart::Literal(s) => out.push_str(s),
            StringPart::Interp(name) => {
                if let Some(v) = vars.get(name).or_else(|| env.get(name)) {
                    out.push_str(v);
                }
            }
            StringPart::EscapedDollar => out.push('$'),
        }
    }
    out
}

// ─── Pure Expression Evaluator ──────────────────────────────

#[async_recursion::async_recursion]
pub async fn eval_pure_expr(
    expr: &Spanned<ir::PureExpr>,
    vars: &mut HashMap<String, String>,
    env: &Arc<Env>,
    code_server: &CodeServer,
    ctx: &mut dyn PureContext,
) -> Result<String, Failure> {
    match &expr.node {
        ir::PureExpr::String(s) => Ok(interpolate_pure(s, vars, env)),
        ir::PureExpr::Var(name) => {
            Ok(vars.get(name).or_else(|| env.get(name)).cloned().unwrap_or_default())
        }
        ir::PureExpr::Call(call) => {
            eval_pure_call(call, &expr.span, vars, env, code_server, ctx).await
        }
    }
}

async fn eval_pure_call(
    call: &ir::PureFnCall,
    span: &Span,
    vars: &mut HashMap<String, String>,
    env: &Arc<Env>,
    code_server: &CodeServer,
    ctx: &mut dyn PureContext,
) -> Result<String, Failure> {
    let callable = code_server
        .lookup_pure(&call.name.node, call.args.len())
        .ok_or_else(|| Failure::Runtime {
            message: format!(
                "undefined pure function `{}` with arity {}",
                call.name.node,
                call.args.len()
            ),
            span: Some(span.clone()),
            shell: None,
        })?;

    let mut evaluated_args = Vec::with_capacity(call.args.len());
    for arg in &call.args {
        evaluated_args.push(eval_pure_expr(arg, vars, env, code_server, ctx).await?);
    }

    match callable {
        Callable::UserDefinedPure(fn_id) => {
            let func = code_server.get_pure(fn_id).ok_or_else(|| Failure::Runtime {
                message: format!("invalid pure function id {fn_id}"),
                span: Some(span.clone()),
                shell: None,
            })?.clone();

            let mut fn_vars = HashMap::new();
            for (param, value) in func.params.iter().zip(evaluated_args.into_iter()) {
                fn_vars.insert(param.node.clone(), value);
            }
            exec_pure_body(&func.body, &mut fn_vars, env, code_server, ctx).await
        }
        Callable::PureBuiltin(bif) => {
            bif.call(ctx, evaluated_args, span).await
        }
        _ => Err(Failure::Runtime {
            message: format!(
                "cannot call impure function `{}` from pure context",
                call.name.node
            ),
            span: Some(span.clone()),
            shell: None,
        }),
    }
}

// ─── Pure Statement Executor ────────────────────────────────

async fn exec_pure_stmt(
    stmt: &Spanned<ir::PureStmt>,
    vars: &mut HashMap<String, String>,
    env: &Arc<Env>,
    code_server: &CodeServer,
    ctx: &mut dyn PureContext,
) -> Result<String, Failure> {
    match &stmt.node {
        ir::PureStmt::Let(decl) => {
            let value = if let Some(expr) = &decl.value {
                eval_pure_expr(expr, vars, env, code_server, ctx).await?
            } else {
                String::new()
            };
            vars.insert(decl.name.node.clone(), value.clone());
            Ok(value)
        }
        ir::PureStmt::Assign(assign) => {
            let value = eval_pure_expr(&assign.value, vars, env, code_server, ctx).await?;
            if vars.contains_key(&assign.name.node) {
                vars.insert(assign.name.node.clone(), value.clone());
                Ok(value)
            } else {
                Err(Failure::Runtime {
                    message: format!(
                        "assignment to undeclared variable `{}`",
                        assign.name.node
                    ),
                    span: Some(assign.name.span.clone()),
                    shell: None,
                })
            }
        }
        ir::PureStmt::Expr(expr) => {
            eval_pure_expr(&Spanned::new(expr.clone(), stmt.span.clone()), vars, env, code_server, ctx).await
        }
    }
}

/// Execute a pure function body. Returns the value of the last expression.
pub async fn exec_pure_body(
    body: &[Spanned<ir::PureStmt>],
    vars: &mut HashMap<String, String>,
    env: &Arc<Env>,
    code_server: &CodeServer,
    ctx: &mut dyn PureContext,
) -> Result<String, Failure> {
    let mut last = String::new();
    for stmt in body {
        last = exec_pure_stmt(stmt, vars, env, code_server, ctx).await?;
    }
    Ok(last)
}

// ─── Convenience helpers for non-shell contexts ─────────────

/// Evaluate an overlay's PureExpr values into a HashMap of strings.
pub async fn eval_overlay(
    overlay: &[ir::OverlayEntry],
    env: &Arc<Env>,
    code_server: &CodeServer,
) -> HashMap<String, String> {
    let mut ctx = SimplePureContext;
    let mut vars = HashMap::new();
    let mut result = HashMap::new();
    for entry in overlay {
        let value = eval_pure_expr(&entry.value, &mut vars, env, code_server, &mut ctx)
            .await
            .unwrap_or_default();
        result.insert(entry.key.node.clone(), value);
    }
    result
}

/// Evaluate a PureVarDecl's optional value expression.
pub async fn eval_pure_var_value(
    expr: &Spanned<ir::PureExpr>,
    env: &Arc<Env>,
    code_server: &CodeServer,
    ctx: &mut dyn PureContext,
) -> Result<String, Failure> {
    let mut vars = HashMap::new();
    eval_pure_expr(expr, &mut vars, env, code_server, ctx).await
}
