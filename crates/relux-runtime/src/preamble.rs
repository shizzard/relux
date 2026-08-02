//! Shared evaluation of a test/effect preamble - the `let` and pure-match
//! items that run before shells spawn. Both `run_test_body` and
//! `bootstrap_effect` iterate their own IR item type and delegate each arm
//! here, so the failure translation (real seq, vars snapshot, typed match
//! context) lives in one place.

use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;
use relux_core::pure::LayeredEnv;
use relux_ir::IrInterpolation;
use relux_ir::IrPureExpr;
use relux_ir::IrPureLetStmt;
use relux_ir::PureFnTable;

use crate::observe::structured::MatchContext;
use crate::observe::structured::SpanId;
use crate::observe::structured::StructuredLogBuilder;
use crate::observe::structured::log_sink::LogSink;
use crate::report::result::ExecError;
use crate::report::result::pure_eval_failure;
use crate::vm::context::Scope;

/// Evaluate one preamble `let` into `scope`. `captures` is read-only - a
/// `let` never binds `$n`. On a pure-eval failure, returns the `ExecError`
/// via `pure_eval_failure`, carrying the real seq and a vars snapshot.
#[allow(clippy::too_many_arguments)] // mirrors `apply_pure_match`; arg clump is a separate data-model cleanup (arch #5)
pub(crate) async fn eval_preamble_let(
    log: &StructuredLogBuilder,
    env: &LayeredEnv,
    pure_fns: &PureFnTable,
    scope: &Scope,
    span: SpanId,
    match_context: &MatchContext,
    stmt: &IrPureLetStmt,
    step_span: &IrSpan,
    captures: &HashMap<String, String>,
) -> Result<(), ExecError> {
    let mut vars = scope.vars().lock().await;
    let mut sink = LogSink::new(log, span);
    let value = if let Some(expr) = stmt.value() {
        match relux_ir::evaluator::eval_pure_expr(expr, &vars, captures, env, pure_fns, &mut sink) {
            Ok(v) => v,
            Err(err) => {
                let vars_in_scope = vars.snapshot();
                return Err(pure_eval_failure(
                    err,
                    span,
                    match_context.clone(),
                    vars_in_scope,
                    &sink,
                    log,
                ));
            }
        }
    } else {
        String::new()
    };
    let name = stmt.name().name();
    vars.insert(name.to_string(), value.clone());
    drop(vars);
    log.emit_var_let(span, None, None, name, &value, Some(step_span));
    Ok(())
}

/// Evaluate one preamble pure-match into `scope`, mutating `captures` - a
/// regex `?` binds `$n` for later steps and overlays. Same failure
/// translation as `eval_preamble_let`.
#[allow(clippy::too_many_arguments)] // mirrors `apply_pure_match`; arg clump is a separate data-model cleanup (arch #5)
pub(crate) async fn eval_preamble_pure_match(
    log: &StructuredLogBuilder,
    env: &LayeredEnv,
    pure_fns: &PureFnTable,
    scope: &Scope,
    span: SpanId,
    match_context: &MatchContext,
    lhs: &IrPureExpr,
    pattern: &IrInterpolation,
    is_regex: bool,
    step_span: &IrSpan,
    captures: &mut HashMap<String, String>,
) -> Result<(), ExecError> {
    let vars = scope.vars().lock().await;
    let mut sink = LogSink::new(log, span);
    if let Err(err) = relux_ir::evaluator::apply_pure_match(
        &mut sink, &vars, captures, lhs, pattern, is_regex, step_span, env, pure_fns,
    ) {
        let vars_in_scope = vars.snapshot();
        return Err(pure_eval_failure(
            err,
            span,
            match_context.clone(),
            vars_in_scope,
            &sink,
            log,
        ));
    }
    Ok(())
}
