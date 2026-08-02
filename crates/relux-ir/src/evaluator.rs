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
use relux_core::pure::lookup_var;
use std::collections::HashMap;

// --- Public API ------------------------------------------

/// Evaluate a pure expression to a string value.
///
/// Returns `Result<String, PureEvalError>`. Evaluation is fallible: a
/// pure match that does not match yields `PureMatchFailed` and a
/// malformed interpolated regex yields `MalformedPattern`. The remaining
/// failure modes (undefined functions, wrong arity, cycles) are still
/// caught at lowering time, and missing variables evaluate to empty
/// string.
///
/// The `sink` parameter is informed of every pure-fn call entry/exit
/// and every interpolation containing a value-bearing part (a variable
/// or a capture), so callers that
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
            let value = lookup_var(&[vars], env, name).unwrap_or_default();
            sink.record_var_read(name, &value, span);
            Ok(value)
        }
        IrPureExpr::QualifiedVar {
            qualifier,
            name,
            span,
        } => {
            let key = format!("{qualifier}.{name}");
            let value = lookup_var(&[vars], env, &key).unwrap_or_default();
            sink.record_var_read(&key, &value, span);
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

/// Evaluate a pure-match statement: resolve the LHS and interpolated
/// pattern, run the match, and on a regex hit overwrite `captures`.
/// Returns the matched text -- `$0` (the whole match) for a `?` regex
/// match, and the whole value for a `=` exact match -- the same value
/// the shell `<?` operator returns. A no-match yields `PureMatchFailed`;
/// a bad interpolated regex yields `MalformedPattern`.
#[allow(clippy::too_many_arguments)]
pub fn apply_pure_match(
    sink: &mut dyn PureEvalSink,
    vars: &VarScope,
    captures: &mut HashMap<String, String>,
    lhs: &IrPureExpr,
    pattern: &IrInterpolation,
    is_regex: bool,
    span: &IrSpan,
    env: &LayeredEnv,
    fns: &PureFnTable,
) -> Result<String, PureEvalError> {
    let value = eval_pure_expr(lhs, vars, &*captures, env, fns, sink)?;
    let pat = eval_interpolation(pattern, span, vars, &*captures, env, sink);
    match crate::eval_pure_match(sink, &value, &pat, is_regex, span) {
        Ok(Some(hit)) => {
            let matched = hit.matched_text;
            if is_regex {
                *captures = hit.captures;
            }
            Ok(matched)
        }
        Ok(None) => Err(PureEvalError::PureMatchFailed {
            value,
            pattern: pat,
            is_regex,
            span: span.clone(),
        }),
        Err(e) => Err(PureEvalError::MalformedPattern {
            pattern: e.pattern,
            reason: e.reason,
            span: span.clone(),
        }),
    }
}

/// The result of assembling one interpolation: the substituted string,
/// the redisplay template (placeholders un-substituted), the resolved
/// bindings, and whether any value-bearing part was present (callers
/// emit the Interpolation event only when `emitted`).
pub struct Rendered {
    pub result: String,
    pub template: String,
    pub bindings: Vec<(String, String)>,
    pub emitted: bool,
}

/// Assemble an interpolation in a single walk. `scopes` is an ordered
/// resolution chain (first hit wins) with `env` as the final fallback;
/// `captures` backs `${n}`. This is the one renderer shared by the pure
/// evaluator and the shell VM.
pub fn render_interpolation(
    interp: &IrInterpolation,
    scopes: &[&VarScope],
    env: &LayeredEnv,
    captures: &HashMap<String, String>,
) -> Rendered {
    let mut result = String::new();
    let mut template = String::new();
    let mut bindings: Vec<(String, String)> = Vec::new();
    let mut emitted = false;
    for part in interp.parts() {
        match part {
            IrStringPart::Literal { value, .. } => {
                result.push_str(value);
                template.push_str(value);
            }
            IrStringPart::Var { name, .. } => {
                emitted = true;
                let value = lookup_var(scopes, env, name).unwrap_or_default();
                result.push_str(&value);
                template.push_str("${");
                template.push_str(name);
                template.push('}');
                bindings.push((name.clone(), value));
            }
            IrStringPart::QualifiedVar {
                qualifier, name, ..
            } => {
                emitted = true;
                let key = format!("{qualifier}.{name}");
                let value = lookup_var(scopes, env, &key).unwrap_or_default();
                result.push_str(&value);
                template.push_str("${");
                template.push_str(&key);
                template.push('}');
                bindings.push((key, value));
            }
            IrStringPart::EscapedDollar { .. } => {
                result.push('$');
                template.push_str("$$");
            }
            IrStringPart::CaptureRef { index, .. } => {
                emitted = true;
                let key = index.to_string();
                let value = captures.get(&key).cloned().unwrap_or_default();
                result.push_str(&value);
                template.push_str("${");
                template.push_str(&key);
                template.push('}');
                bindings.push((key, value));
            }
        }
    }
    Rendered {
        result,
        template,
        bindings,
        emitted,
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
    let rendered = render_interpolation(interp, &[vars], env, captures);
    if rendered.emitted {
        sink.record_interpolation(
            &rendered.template,
            &rendered.result,
            &rendered.bindings,
            span,
        );
    }
    rendered.result
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
                let value = apply_pure_match(
                    sink, scope, captures, lhs, pattern, *is_regex, span, env, fns,
                )?;
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
    fn pure_capture_only_interpolation_now_emits_with_binding() {
        // A capture-only pure interpolation used to emit nothing; unified
        // behavior emits with the capture in bindings.
        let interp = IrInterpolation::new(
            vec![IrStringPart::CaptureRef {
                index: 1,
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let expr = IrPureExpr::String {
            value: interp,
            span: IrSpan::synthetic(),
        };
        let mut caps = HashMap::new();
        caps.insert("1".to_string(), "alice".to_string());
        let mut sink = RecordingSink::default();
        let out = eval_pure_expr(
            &expr,
            &VarScope::new(),
            &caps,
            &empty_env(),
            &empty_fns(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(out, "alice");
        match sink.ops.as_slice() {
            [
                SinkOp::RecordInterpolation {
                    result, bindings, ..
                },
            ] => {
                assert_eq!(result, "alice");
                assert_eq!(bindings, &vec![("1".to_string(), "alice".to_string())]);
            }
            other => {
                panic!("expected one RecordInterpolation with a capture binding, got {other:?}")
            }
        }
    }

    fn literal_expr(value: &str) -> IrPureExpr {
        IrPureExpr::String {
            value: IrInterpolation::new(
                vec![IrStringPart::Literal {
                    value: value.into(),
                    span: IrSpan::synthetic(),
                }],
                IrSpan::synthetic(),
            ),
            span: IrSpan::synthetic(),
        }
    }

    fn literal_interp(value: &str) -> IrInterpolation {
        IrInterpolation::new(
            vec![IrStringPart::Literal {
                value: value.into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        )
    }

    #[test]
    fn regex_pure_match_returns_dollar_zero_not_the_lhs() {
        // A `?` pure match evaluates to `$0` (the whole match), like the
        // shell `<?` operator -- not to the whole left-hand value. The
        // pattern is unanchored so `$0` ("v=42") differs from the LHS
        // ("prefix v=42 suffix"), which makes the two observably distinct.
        let lhs = literal_expr("prefix v=42 suffix");
        let pattern = literal_interp("v=(\\d+)");
        let mut caps = HashMap::new();
        let mut sink = NoOpSink;
        let out = apply_pure_match(
            &mut sink,
            &VarScope::new(),
            &mut caps,
            &lhs,
            &pattern,
            true,
            &IrSpan::synthetic(),
            &empty_env(),
            &empty_fns(),
        )
        .unwrap();
        assert_eq!(out, "v=42", "regex pure match returns $0, not the LHS");
        assert_eq!(caps.get("0").map(String::as_str), Some("v=42"));
        assert_eq!(caps.get("1").map(String::as_str), Some("42"));
    }

    #[test]
    fn exact_pure_match_returns_the_whole_value() {
        // A `=` exact match's matched text is the whole value (value ==
        // pattern by definition), and it binds no captures.
        let lhs = literal_expr("linux");
        let pattern = literal_interp("linux");
        let mut caps = HashMap::new();
        let mut sink = NoOpSink;
        let out = apply_pure_match(
            &mut sink,
            &VarScope::new(),
            &mut caps,
            &lhs,
            &pattern,
            false,
            &IrSpan::synthetic(),
            &empty_env(),
            &empty_fns(),
        )
        .unwrap();
        assert_eq!(out, "linux");
        assert!(caps.is_empty(), "`=` binds no captures");
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
        // A plain-string interpolation cannot fail, so `.unwrap()` pins
        // the `Ok` contract without requiring `PureEvalError: PartialEq`.
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
    fn render_interpolation_assembles_result_template_bindings() {
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
        let mut vars = VarScope::new();
        vars.insert("who".into(), "world".into());
        let env = LayeredEnv::root(Env::new());
        let caps: HashMap<String, String> = HashMap::new();
        let r = render_interpolation(&interp, &[&vars], &env, &caps);
        assert_eq!(r.result, "hi world");
        assert_eq!(r.template, "hi ${who}");
        assert_eq!(r.bindings, vec![("who".to_string(), "world".to_string())]);
        assert!(r.emitted);
    }

    #[test]
    fn render_interpolation_qualified_var_resolves_via_flat_key() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::QualifiedVar {
                qualifier: "Db".into(),
                name: "port".into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let mut vars = VarScope::new();
        vars.insert("Db.port".into(), "5432".into());
        let env = LayeredEnv::root(Env::new());
        let caps: HashMap<String, String> = HashMap::new();
        let r = render_interpolation(&interp, &[&vars], &env, &caps);
        assert_eq!(r.result, "5432");
        assert_eq!(r.template, "${Db.port}");
        assert_eq!(
            r.bindings,
            vec![("Db.port".to_string(), "5432".to_string())]
        );
        assert!(r.emitted);
    }

    #[test]
    fn render_interpolation_includes_captureref_in_gate_and_bindings() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::CaptureRef {
                index: 1,
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let vars = VarScope::new();
        let env = LayeredEnv::root(Env::new());
        let mut caps: HashMap<String, String> = HashMap::new();
        caps.insert("1".into(), "alice".into());
        let r = render_interpolation(&interp, &[&vars], &env, &caps);
        assert_eq!(r.result, "alice");
        assert_eq!(r.template, "${1}");
        assert_eq!(r.bindings, vec![("1".to_string(), "alice".to_string())]);
        assert!(r.emitted, "capture-only interpolation must emit");
    }

    #[test]
    fn render_interpolation_escaped_dollar_template_is_double() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::EscapedDollar {
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let vars = VarScope::new();
        let env = LayeredEnv::root(Env::new());
        let caps: HashMap<String, String> = HashMap::new();
        let r = render_interpolation(&interp, &[&vars], &env, &caps);
        assert_eq!(r.result, "$");
        assert_eq!(r.template, "$$");
        assert_eq!(r.bindings, Vec::<(String, String)>::new());
        assert!(
            !r.emitted,
            "a literal-only string (escaped dollar) does not emit"
        );
    }

    #[test]
    fn render_interpolation_unresolved_var_is_empty_string() {
        let interp = IrInterpolation::new(
            vec![IrStringPart::Var {
                name: "missing".into(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let vars = VarScope::new();
        let env = LayeredEnv::root(Env::new());
        let caps: HashMap<String, String> = HashMap::new();
        let r = render_interpolation(&interp, &[&vars], &env, &caps);
        assert_eq!(r.result, "");
        assert_eq!(r.bindings, vec![("missing".to_string(), "".to_string())]);
        assert!(r.emitted);
    }

    #[test]
    fn qualified_var_resolves_via_flat_key() {
        let expr = IrPureExpr::QualifiedVar {
            qualifier: "Db".into(),
            name: "port".into(),
            span: IrSpan::synthetic(),
        };
        let mut vars = VarScope::new();
        vars.insert("Db.port".into(), "5432".into());
        let out = eval_pure_expr(
            &expr,
            &vars,
            &HashMap::new(),
            &empty_env(),
            &empty_fns(),
            &mut NoOpSink,
        )
        .unwrap();
        assert_eq!(out, "5432");
    }

    #[test]
    fn qualified_var_missing_is_empty_string() {
        let expr = IrPureExpr::QualifiedVar {
            qualifier: "Db".into(),
            name: "missing".into(),
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
        assert_eq!(out, "");
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
