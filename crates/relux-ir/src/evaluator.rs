use crate::IrInterpolation;
use crate::IrPureCallExpr;
use crate::IrPureExpr;
use crate::IrPureFn;
use crate::IrPureStmt;
use crate::IrStringPart;
use crate::PureFnTable;
use relux_core::pure::LayeredEnv;
use relux_core::pure::VarScope;

// ─── Public API ─────────────────────────────────────────────

/// Evaluate a pure expression to a string value.
///
/// Infallible — all failure modes (undefined functions, wrong arity,
/// cycles) are caught at lowering time. Missing variables evaluate
/// to empty string.
pub fn eval_pure_expr(
    expr: &IrPureExpr,
    vars: &VarScope,
    env: &LayeredEnv,
    fns: &PureFnTable,
) -> String {
    match expr {
        IrPureExpr::String { value, .. } => eval_interpolation(value, vars, env),
        IrPureExpr::Var { name, .. } => resolve_var(name, vars, env),
        IrPureExpr::Call { call, .. } => eval_pure_call(call, vars, env, fns),
    }
}

/// Evaluate a resolved pure function with the given arguments.
///
/// Infallible — see `eval_pure_expr`.
pub fn eval_pure_fn(
    func: &IrPureFn,
    args: Vec<String>,
    env: &LayeredEnv,
    fns: &PureFnTable,
) -> String {
    match func {
        IrPureFn::Builtin { name, .. } => relux_core::pure::bifs::dispatch(name, args),
        IrPureFn::UserDefined { params, body, .. } => {
            let mut scope = VarScope::new();
            for (param, arg) in params.iter().zip(args) {
                scope.insert(param.name().to_string(), arg);
            }
            eval_body(body, &mut scope, env, fns)
        }
    }
}

// ─── Internal helpers ───────────────────────────────────────

fn eval_pure_call(
    call: &IrPureCallExpr,
    vars: &VarScope,
    env: &LayeredEnv,
    fns: &PureFnTable,
) -> String {
    let args: Vec<String> = call
        .args()
        .iter()
        .map(|arg| eval_pure_expr(arg, vars, env, fns))
        .collect();

    let resolved = call.resolved();
    let func = fns
        .get(resolved)
        .expect("resolved FnId must be in PureFnTable")
        .as_ref()
        .expect("resolved function must not be a LoweringBail");

    eval_pure_fn(func, args, env, fns)
}

fn eval_interpolation(interp: &IrInterpolation, vars: &VarScope, env: &LayeredEnv) -> String {
    let mut result = String::new();
    for part in interp.parts() {
        match part {
            IrStringPart::Literal { value, .. } => result.push_str(value),
            IrStringPart::Var { name, .. } => result.push_str(&resolve_var(name, vars, env)),
            IrStringPart::QualifiedVar { .. } => {
                unreachable!("QualifiedVar in pure interpolation context")
            }
            IrStringPart::EscapedDollar { .. } => result.push('$'),
            IrStringPart::CaptureRef { .. } => {
                unreachable!("CaptureRef in pure interpolation context")
            }
        }
    }
    result
}

fn resolve_var(name: &str, vars: &VarScope, env: &LayeredEnv) -> String {
    vars.get(name)
        .or_else(|| env.get(name))
        .unwrap_or("")
        .to_string()
}

fn eval_body(
    body: &[IrPureStmt],
    scope: &mut VarScope,
    env: &LayeredEnv,
    fns: &PureFnTable,
) -> String {
    let mut last_value = String::new();
    for (i, stmt) in body.iter().enumerate() {
        let is_last = i == body.len() - 1;
        match stmt {
            IrPureStmt::Comment { .. } => {}
            IrPureStmt::Let { stmt: let_stmt, .. } => {
                let value = let_stmt
                    .value()
                    .map(|v| eval_pure_expr(v, scope, env, fns))
                    .unwrap_or_default();
                scope.insert(let_stmt.name().name().to_string(), value.clone());
                if is_last {
                    last_value = value;
                }
            }
            IrPureStmt::Assign {
                stmt: assign_stmt, ..
            } => {
                let value = eval_pure_expr(assign_stmt.value(), scope, env, fns);
                scope.assign(assign_stmt.name().name(), value.clone());
                if is_last {
                    last_value = value;
                }
            }
            IrPureStmt::Expr { expr, .. } => {
                let value = eval_pure_expr(expr, scope, env, fns);
                if is_last {
                    last_value = value;
                }
            }
        }
    }
    last_value
}

// ─── Tests ──────────────────────────────────────────────────
