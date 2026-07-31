use crate::IrInterpolation;
use crate::IrPureCallExpr;
use crate::IrPureExpr;
use crate::IrPureFn;
use crate::IrPureStmt;
use crate::IrStringPart;
use crate::PureEvalError;
use crate::PureFnTable;
use crate::pure_sink::PureEvalSink;
use relux_core::diagnostics::IrSpan;
use relux_core::pure::LayeredEnv;
use relux_core::pure::VarScope;
use std::collections::HashMap;

// --- Public API ------------------------------------------

/// Evaluate a pure expression to a string value.
///
/// Returns `Result<String, PureEvalError>`. In this milestone the error
/// is uninhabited, so evaluation is effectively infallible - all failure
/// modes (undefined functions, wrong arity, cycles) are still caught at
/// lowering time, and missing variables evaluate to empty string.
///
/// The `sink` parameter is informed of every pure-fn call entry/exit
/// and every interpolation that resolves a variable, so callers that
/// care about structured-log emission (runtime, marker recording) can
/// observe the chain of work. Callers that don't care pass `&mut
/// NoOpSink`.
pub fn eval_pure_expr(
    expr: &IrPureExpr,
    vars: &VarScope,
    captures: &HashMap<String, String>,
    env: &LayeredEnv,
    fns: &PureFnTable,
    sink: &mut dyn PureEvalSink,
) -> Result<String, PureEvalError> {
    match expr {
        IrPureExpr::String { value, span } => {
            Ok(eval_interpolation(value, span, vars, captures, env, sink))
        }
        IrPureExpr::Var { name, span } => {
            let value = resolve_var(name, vars, env);
            sink.record_var_read(name, &value, span);
            Ok(value)
        }
        IrPureExpr::Call { call, .. } => eval_pure_call(call, vars, captures, env, fns, sink),
        IrPureExpr::Capture { index, .. } => Ok(captures
            .get(&index.to_string())
            .cloned()
            .unwrap_or_default()),
    }
}

/// Evaluate a resolved pure function with the given arguments.
///
/// See `eval_pure_expr` for the fallibility contract.
pub fn eval_pure_fn(
    func: &IrPureFn,
    args: Vec<String>,
    env: &LayeredEnv,
    fns: &PureFnTable,
    sink: &mut dyn PureEvalSink,
) -> Result<String, PureEvalError> {
    match func {
        IrPureFn::Builtin { name, .. } => Ok(relux_core::pure::bifs::dispatch(name, args)),
        IrPureFn::UserDefined { params, body, .. } => {
            let mut scope = VarScope::new();
            let mut captures: HashMap<String, String> = HashMap::new();
            for (param, arg) in params.iter().zip(args) {
                scope.insert(param.name().to_string(), arg);
            }
            eval_body(body, &mut scope, &mut captures, env, fns, sink)
        }
    }
}

// --- Internal helpers ------------------------------------

fn eval_pure_call(
    call: &IrPureCallExpr,
    vars: &VarScope,
    captures: &HashMap<String, String>,
    env: &LayeredEnv,
    fns: &PureFnTable,
    sink: &mut dyn PureEvalSink,
) -> Result<String, PureEvalError> {
    use crate::IrNode;
    let args: Vec<String> = call
        .args()
        .iter()
        .map(|arg| eval_pure_expr(arg, vars, captures, env, fns, sink))
        .collect::<Result<Vec<_>, _>>()?;

    let resolved = call.resolved();
    let func = fns
        .get(resolved)
        .expect("resolved FnId must be in PureFnTable")
        .as_ref()
        .expect("resolved function must not be a LoweringBail");

    let (is_builtin, named_args, name) = match func {
        IrPureFn::Builtin { name, .. } => (
            true,
            args.iter()
                .enumerate()
                .map(|(i, v)| (format!("${i}"), v.clone()))
                .collect::<Vec<_>>(),
            name.clone(),
        ),
        IrPureFn::UserDefined {
            name: fn_name,
            params,
            ..
        } => (
            false,
            params
                .iter()
                .zip(args.iter())
                .map(|(p, v)| (p.name().to_string(), v.clone()))
                .collect::<Vec<_>>(),
            fn_name.name().to_string(),
        ),
    };
    sink.enter_pure_fn(&name, &named_args, is_builtin, call.span());

    let result = eval_pure_fn(func, args, env, fns, sink)?;
    sink.leave_pure_fn(&result);
    Ok(result)
}

fn eval_interpolation(
    interp: &IrInterpolation,
    span: &IrSpan,
    vars: &VarScope,
    captures: &HashMap<String, String>,
    env: &LayeredEnv,
    sink: &mut dyn PureEvalSink,
) -> String {
    let mut result = String::new();
    let mut bindings: Vec<(String, String)> = Vec::new();
    let mut has_interp = false;
    let mut template = String::new();
    for part in interp.parts() {
        match part {
            IrStringPart::Literal { value, .. } => {
                result.push_str(value);
                template.push_str(value);
            }
            IrStringPart::Var { name, .. } => {
                has_interp = true;
                let v = resolve_var(name, vars, env);
                result.push_str(&v);
                template.push_str("${");
                template.push_str(name);
                template.push('}');
                bindings.push((name.clone(), v));
            }
            IrStringPart::QualifiedVar { .. } => {
                unreachable!("QualifiedVar in pure interpolation context")
            }
            IrStringPart::EscapedDollar { .. } => {
                result.push('$');
                template.push_str("$$");
            }
            IrStringPart::CaptureRef { index, .. } => {
                let v = captures
                    .get(&index.to_string())
                    .map(String::as_str)
                    .unwrap_or("");
                result.push_str(v);
                template.push_str(&format!("${{{index}}}"));
            }
        }
    }
    if has_interp {
        sink.record_interpolation(&template, &result, &bindings, span);
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
    captures: &mut HashMap<String, String>,
    env: &LayeredEnv,
    fns: &PureFnTable,
    sink: &mut dyn PureEvalSink,
) -> Result<String, PureEvalError> {
    let mut last_value = String::new();
    for (i, stmt) in body.iter().enumerate() {
        let is_last = i == body.len() - 1;
        match stmt {
            IrPureStmt::Comment { .. } => {}
            IrPureStmt::Let { stmt: let_stmt, .. } => {
                let value = match let_stmt.value() {
                    Some(v) => eval_pure_expr(v, scope, &*captures, env, fns, sink)?,
                    None => String::new(),
                };
                scope.insert(let_stmt.name().name().to_string(), value.clone());
                if is_last {
                    last_value = value;
                }
            }
            IrPureStmt::Assign {
                stmt: assign_stmt, ..
            } => {
                let value = eval_pure_expr(assign_stmt.value(), scope, &*captures, env, fns, sink)?;
                scope.assign(assign_stmt.name().name(), value.clone());
                if is_last {
                    last_value = value;
                }
            }
            IrPureStmt::Expr { expr, .. } => {
                let value = eval_pure_expr(expr, scope, &*captures, env, fns, sink)?;
                if is_last {
                    last_value = value;
                }
            }
            IrPureStmt::PureMatch {
                lhs,
                pattern,
                is_regex,
                span,
            } => {
                let value = eval_pure_expr(lhs, scope, &*captures, env, fns, sink)?;
                let pat = eval_interpolation(pattern, span, scope, &*captures, env, sink);
                match crate::eval_pure_match(sink, &value, &pat, *is_regex, span) {
                    Ok(Some(hit)) => {
                        if *is_regex {
                            *captures = hit.captures;
                        }
                    }
                    Ok(None) => {
                        return Err(PureEvalError::PureMatchFailed {
                            value,
                            pattern: pat,
                            is_regex: *is_regex,
                            span: span.clone(),
                            call_stack: Vec::new(),
                        });
                    }
                    Err(e) => {
                        return Err(PureEvalError::MalformedPattern {
                            pattern: e.pattern,
                            reason: e.reason,
                            span: span.clone(),
                        });
                    }
                }
                if is_last {
                    last_value = value;
                }
            }
        }
    }
    Ok(last_value)
}

// --- Tests -----------------------------------------------

#[cfg(test)]
mod sink_tests {
    use super::*;
    use crate::PureFnTable;
    use crate::pure_sink::NoOpSink;
    use crate::pure_sink::RecordingSink;
    use crate::pure_sink::SinkOp;
    use relux_core::diagnostics::IrSpan;
    use relux_core::pure::Env;
    use relux_core::pure::LayeredEnv;
    use relux_core::pure::VarScope;

    fn empty_env() -> LayeredEnv {
        LayeredEnv::root(Env::new())
    }

    fn empty_fns() -> PureFnTable {
        PureFnTable::new()
    }

    #[test]
    fn interpolation_with_a_var_emits_record_interpolation() {
        let interp = IrInterpolation::new(
            vec![
                IrStringPart::Literal {
                    value: "hi ".into(),
                    span: IrSpan::synthetic(),
                },
                IrStringPart::Var {
                    name: "who".into(),
                    span: IrSpan::synthetic(),
                },
            ],
            IrSpan::synthetic(),
        );
        let expr = IrPureExpr::String {
            value: interp,
            span: IrSpan::synthetic(),
        };
        let mut vars = VarScope::new();
        vars.insert("who".into(), "world".into());
        let mut sink = RecordingSink::default();
        let out = eval_pure_expr(
            &expr,
            &vars,
            &HashMap::new(),
            &empty_env(),
            &empty_fns(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(out, "hi world");
        match sink.ops.as_slice() {
            [SinkOp::RecordInterpolation { result, .. }] => assert_eq!(result, "hi world"),
            other => panic!("expected one RecordInterpolation, got {other:?}"),
        }
    }

    #[test]
    fn literal_only_interpolation_does_not_emit() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::Literal {
                value: "static".into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let expr = IrPureExpr::String {
            value: interp,
            span: IrSpan::synthetic(),
        };
        let mut sink = RecordingSink::default();
        let _ = eval_pure_expr(
            &expr,
            &VarScope::new(),
            &HashMap::new(),
            &empty_env(),
            &empty_fns(),
            &mut sink,
        )
        .unwrap();
        assert!(sink.ops.is_empty());
    }

    #[test]
    fn noop_sink_does_not_break_evaluation() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::Literal {
                value: "x".into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let expr = IrPureExpr::String {
            value: interp,
            span: IrSpan::synthetic(),
        };
        let out = eval_pure_expr(
            &expr,
            &VarScope::new(),
            &HashMap::new(),
            &empty_env(),
            &empty_fns(),
            &mut NoOpSink,
        )
        .unwrap();
        assert_eq!(out, "x");
    }

    #[test]
    fn eval_pure_expr_returns_ok_for_a_literal() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::Literal {
                value: "v".into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let expr = IrPureExpr::String {
            value: interp,
            span: IrSpan::synthetic(),
        };
        // `.unwrap()` pins the `Ok` contract without requiring
        // `PureEvalError: PartialEq` (it derives none - it is uninhabited).
        let out = eval_pure_expr(
            &expr,
            &VarScope::new(),
            &HashMap::new(),
            &empty_env(),
            &empty_fns(),
            &mut NoOpSink,
        )
        .unwrap();
        assert_eq!(out, "v");
    }

    #[test]
    fn capture_resolves_from_frame_and_empty_when_unbound() {
        let mut caps = HashMap::new();
        caps.insert("1".to_string(), "alice".to_string());
        let bound = IrPureExpr::Capture {
            index: 1,
            span: IrSpan::synthetic(),
        };
        let unbound = IrPureExpr::Capture {
            index: 2,
            span: IrSpan::synthetic(),
        };
        assert_eq!(
            eval_pure_expr(
                &bound,
                &VarScope::new(),
                &caps,
                &empty_env(),
                &empty_fns(),
                &mut NoOpSink
            )
            .unwrap(),
            "alice"
        );
        assert_eq!(
            eval_pure_expr(
                &unbound,
                &VarScope::new(),
                &caps,
                &empty_env(),
                &empty_fns(),
                &mut NoOpSink
            )
            .unwrap(),
            ""
        );
    }
}
