use super::Env;
use super::VarScope;
use crate::dsl::resolver::ir::IrInterpolation;
use crate::dsl::resolver::ir::IrPureCallExpr;
use crate::dsl::resolver::ir::IrPureExpr;
use crate::dsl::resolver::ir::IrPureFn;
use crate::dsl::resolver::ir::IrPureStmt;
use crate::dsl::resolver::ir::IrStringPart;
use crate::dsl::resolver::ir::PureFnTable;

// ─── Public API ─────────────────────────────────────────────

/// Evaluate a pure expression to a string value.
///
/// Infallible — all failure modes (undefined functions, wrong arity,
/// cycles) are caught at lowering time. Missing variables evaluate
/// to empty string.
pub fn eval_pure_expr(expr: &IrPureExpr, vars: &VarScope, env: &Env, fns: &PureFnTable) -> String {
    match expr {
        IrPureExpr::String { value, .. } => eval_interpolation(value, vars, env),
        IrPureExpr::Var { name, .. } => resolve_var(name, vars, env),
        IrPureExpr::Call { call, .. } => eval_pure_call(call, vars, env, fns),
    }
}

/// Evaluate a resolved pure function with the given arguments.
///
/// Infallible — see `eval_pure_expr`.
pub fn eval_pure_fn(func: &IrPureFn, args: Vec<String>, env: &Env, fns: &PureFnTable) -> String {
    match func {
        IrPureFn::Builtin { name, .. } => super::bifs::dispatch(name, args),
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

fn eval_pure_call(call: &IrPureCallExpr, vars: &VarScope, env: &Env, fns: &PureFnTable) -> String {
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

fn eval_interpolation(interp: &IrInterpolation, vars: &VarScope, env: &Env) -> String {
    let mut result = String::new();
    for part in interp.parts() {
        match part {
            IrStringPart::Literal { value, .. } => result.push_str(value),
            IrStringPart::Var { name, .. } => result.push_str(&resolve_var(name, vars, env)),
            IrStringPart::EscapedDollar { .. } => result.push('$'),
            IrStringPart::CaptureRef { .. } => {
                unreachable!("CaptureRef in pure interpolation context")
            }
        }
    }
    result
}

fn resolve_var(name: &str, vars: &VarScope, env: &Env) -> String {
    vars.get(name)
        .or_else(|| env.get(name))
        .unwrap_or("")
        .to_string()
}

fn eval_body(body: &[IrPureStmt], scope: &mut VarScope, env: &Env, fns: &PureFnTable) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use crate::diagnostics::FnId as DiagFnId;
    use crate::diagnostics::IrSpan;
    use crate::diagnostics::ModulePath;
    use crate::dsl::resolver::ir::IrIdent;
    use crate::dsl::resolver::ir::IrPureAssignStmt;
    use crate::dsl::resolver::ir::IrPureLetStmt;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn test_file() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file(), crate::Span::new(0, 0))
    }

    fn test_env() -> Env {
        Env::from_map(HashMap::new())
    }

    fn empty_fns() -> PureFnTable {
        PureFnTable::new()
    }

    fn builtin_id(name: &str, arity: usize) -> DiagFnId {
        DiagFnId {
            module: ModulePath("@builtin".into()),
            name: name.into(),
            arity,
        }
    }

    fn test_ident(name: &str) -> IrIdent {
        IrIdent::new(name, test_span())
    }

    fn lit(s: &str) -> IrStringPart {
        IrStringPart::Literal {
            value: s.into(),
            span: test_span(),
        }
    }

    fn var_part(name: &str) -> IrStringPart {
        IrStringPart::Var {
            name: name.into(),
            span: test_span(),
        }
    }

    fn escaped_dollar() -> IrStringPart {
        IrStringPart::EscapedDollar { span: test_span() }
    }

    fn str_expr(parts: Vec<IrStringPart>) -> IrPureExpr {
        IrPureExpr::String {
            value: IrInterpolation::new(parts, test_span()),
            span: test_span(),
        }
    }

    fn var_expr(name: &str) -> IrPureExpr {
        IrPureExpr::Var {
            name: name.into(),
            span: test_span(),
        }
    }

    fn call_expr(name: &str, resolved: DiagFnId, args: Vec<IrPureExpr>) -> IrPureExpr {
        IrPureExpr::Call {
            call: IrPureCallExpr::new(test_ident(name), resolved, args, test_span()),
            span: test_span(),
        }
    }

    fn str_pure_expr(s: &str) -> IrPureExpr {
        IrPureExpr::String {
            value: IrInterpolation::new(vec![lit(s)], test_span()),
            span: test_span(),
        }
    }

    /// Register a builtin pure fn in the table and return it.
    fn register_builtin(fns: &PureFnTable, name: &str, arity: usize) {
        let id = builtin_id(name, arity);
        fns.insert(
            id,
            Ok(IrPureFn::Builtin {
                name: name.into(),
                arity,
            }),
        );
    }

    /// Build a PureFnTable with common builtins registered.
    fn fns_with_builtins(names: &[(&str, usize)]) -> PureFnTable {
        let fns = PureFnTable::new();
        for &(name, arity) in names {
            register_builtin(&fns, name, arity);
        }
        fns
    }

    // ─── Expression evaluation ──────────────────────────────

    #[test]
    fn eval_string_literal() {
        let expr = str_expr(vec![lit("hello")]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            "hello"
        );
    }

    #[test]
    fn eval_string_empty() {
        let expr = str_expr(vec![]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            ""
        );
    }

    #[test]
    fn eval_string_with_var() {
        let expr = str_expr(vec![lit("hello "), var_part("name")]);
        let mut vars = VarScope::new();
        vars.insert("name".into(), "world".into());
        assert_eq!(
            eval_pure_expr(&expr, &vars, &test_env(), &empty_fns()),
            "hello world"
        );
    }

    #[test]
    fn eval_string_missing_var() {
        let expr = str_expr(vec![lit("hello "), var_part("name")]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            "hello "
        );
    }

    #[test]
    fn eval_string_with_env_var() {
        let expr = str_expr(vec![var_part("MY_VAR")]);
        let env = Env::from_map(HashMap::from([("MY_VAR".into(), "from_env".into())]));
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &env, &empty_fns()),
            "from_env"
        );
    }

    #[test]
    fn eval_string_var_shadows_env() {
        let expr = str_expr(vec![var_part("X")]);
        let mut vars = VarScope::new();
        vars.insert("X".into(), "local".into());
        let env = Env::from_map(HashMap::from([("X".into(), "env".into())]));
        assert_eq!(eval_pure_expr(&expr, &vars, &env, &empty_fns()), "local");
    }

    #[test]
    fn eval_string_concatenation() {
        let expr = str_expr(vec![lit("a"), lit("b"), lit("c")]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            "abc"
        );
    }

    #[test]
    fn eval_string_escaped_dollar() {
        let expr = str_expr(vec![lit("cost: "), escaped_dollar(), lit("5")]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            "cost: $5"
        );
    }

    #[test]
    fn eval_string_only_var() {
        let expr = str_expr(vec![var_part("x")]);
        let mut vars = VarScope::new();
        vars.insert("x".into(), "val".into());
        assert_eq!(
            eval_pure_expr(&expr, &vars, &test_env(), &empty_fns()),
            "val"
        );
    }

    #[test]
    fn eval_string_adjacent_vars() {
        let expr = str_expr(vec![var_part("a"), var_part("b")]);
        let mut vars = VarScope::new();
        vars.insert("a".into(), "1".into());
        vars.insert("b".into(), "2".into());
        assert_eq!(
            eval_pure_expr(&expr, &vars, &test_env(), &empty_fns()),
            "12"
        );
    }

    #[test]
    fn eval_var_present() {
        let expr = var_expr("x");
        let mut vars = VarScope::new();
        vars.insert("x".into(), "val".into());
        assert_eq!(
            eval_pure_expr(&expr, &vars, &test_env(), &empty_fns()),
            "val"
        );
    }

    #[test]
    fn eval_var_missing() {
        let expr = var_expr("x");
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &empty_fns()),
            ""
        );
    }

    #[test]
    fn eval_var_empty_value() {
        let expr = var_expr("x");
        let mut vars = VarScope::new();
        vars.insert("x".into(), String::new());
        assert_eq!(eval_pure_expr(&expr, &vars, &test_env(), &empty_fns()), "");
    }

    #[test]
    fn eval_call_builtin() {
        let fns = fns_with_builtins(&[("trim", 1)]);
        let arg = str_expr(vec![lit("  hi  ")]);
        let expr = call_expr("trim", builtin_id("trim", 1), vec![arg]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &fns),
            "hi"
        );
    }

    #[test]
    fn eval_call_user_defined() {
        let fns = PureFnTable::new();
        let fn_id = DiagFnId {
            module: ModulePath("lib/utils".into()),
            name: "greet".into(),
            arity: 1,
        };
        fns.insert(
            fn_id.clone(),
            Ok(IrPureFn::UserDefined {
                name: test_ident("greet"),
                params: vec![test_ident("who")],
                body: vec![IrPureStmt::Expr {
                    expr: str_expr(vec![lit("hello "), var_part("who")]),
                    span: test_span(),
                }],
                span: test_span(),
            }),
        );
        let arg = str_expr(vec![lit("world")]);
        let expr = call_expr("greet", fn_id, vec![arg]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &fns),
            "hello world"
        );
    }

    #[test]
    fn eval_call_nested() {
        let fns = fns_with_builtins(&[("upper", 1), ("lower", 1)]);
        // lower(upper("ab"))
        let inner_arg = str_expr(vec![lit("ab")]);
        let inner = call_expr("upper", builtin_id("upper", 1), vec![inner_arg]);
        let expr = call_expr("lower", builtin_id("lower", 1), vec![inner]);
        assert_eq!(
            eval_pure_expr(&expr, &VarScope::new(), &test_env(), &fns),
            "ab"
        );
    }

    // ─── Function evaluation ────────────────────────────────

    #[test]
    fn eval_fn_identity() {
        let func = IrPureFn::UserDefined {
            name: test_ident("id"),
            params: vec![test_ident("x")],
            body: vec![IrPureStmt::Expr {
                expr: var_expr("x"),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(&func, vec!["hello".into()], &test_env(), &empty_fns()),
            "hello"
        );
    }

    #[test]
    fn eval_fn_with_let() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(
                        test_ident("x"),
                        Some(str_pure_expr("v")),
                        test_span(),
                    ),
                    span: test_span(),
                },
                IrPureStmt::Expr {
                    expr: var_expr("x"),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&func, vec![], &test_env(), &empty_fns()), "v");
    }

    #[test]
    fn eval_fn_with_assign() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(
                        test_ident("x"),
                        Some(str_pure_expr("a")),
                        test_span(),
                    ),
                    span: test_span(),
                },
                IrPureStmt::Assign {
                    stmt: IrPureAssignStmt::new(test_ident("x"), str_pure_expr("b"), test_span()),
                    span: test_span(),
                },
                IrPureStmt::Expr {
                    expr: var_expr("x"),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&func, vec![], &test_env(), &empty_fns()), "b");
    }

    #[test]
    fn eval_fn_with_multiple_lets() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(
                        test_ident("a"),
                        Some(str_pure_expr("1")),
                        test_span(),
                    ),
                    span: test_span(),
                },
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(
                        test_ident("b"),
                        Some(str_pure_expr("2")),
                        test_span(),
                    ),
                    span: test_span(),
                },
                IrPureStmt::Expr {
                    expr: str_expr(vec![var_part("a"), var_part("b")]),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&func, vec![], &test_env(), &empty_fns()), "12");
    }

    #[test]
    fn eval_fn_nested_call() {
        let fns = PureFnTable::new();
        let inner_id = DiagFnId {
            module: ModulePath("m".into()),
            name: "inner".into(),
            arity: 0,
        };
        fns.insert(
            inner_id.clone(),
            Ok(IrPureFn::UserDefined {
                name: test_ident("inner"),
                params: vec![],
                body: vec![IrPureStmt::Expr {
                    expr: str_expr(vec![lit("result")]),
                    span: test_span(),
                }],
                span: test_span(),
            }),
        );
        let outer = IrPureFn::UserDefined {
            name: test_ident("outer"),
            params: vec![],
            body: vec![IrPureStmt::Expr {
                expr: call_expr("inner", inner_id, vec![]),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&outer, vec![], &test_env(), &fns), "result");
    }

    #[test]
    fn eval_fn_deeply_nested_call() {
        let fns = PureFnTable::new();
        let c_id = DiagFnId {
            module: ModulePath("m".into()),
            name: "c".into(),
            arity: 0,
        };
        let b_id = DiagFnId {
            module: ModulePath("m".into()),
            name: "b".into(),
            arity: 0,
        };
        fns.insert(
            c_id.clone(),
            Ok(IrPureFn::UserDefined {
                name: test_ident("c"),
                params: vec![],
                body: vec![IrPureStmt::Expr {
                    expr: str_expr(vec![lit("deep")]),
                    span: test_span(),
                }],
                span: test_span(),
            }),
        );
        fns.insert(
            b_id.clone(),
            Ok(IrPureFn::UserDefined {
                name: test_ident("b"),
                params: vec![],
                body: vec![IrPureStmt::Expr {
                    expr: call_expr("c", c_id, vec![]),
                    span: test_span(),
                }],
                span: test_span(),
            }),
        );
        let a = IrPureFn::UserDefined {
            name: test_ident("a"),
            params: vec![],
            body: vec![IrPureStmt::Expr {
                expr: call_expr("b", b_id, vec![]),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&a, vec![], &test_env(), &fns), "deep");
    }

    #[test]
    fn eval_fn_params_shadow_outer() {
        let mut outer_vars = VarScope::new();
        outer_vars.insert("x".into(), "outer".into());
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![test_ident("x")],
            body: vec![IrPureStmt::Expr {
                expr: var_expr("x"),
                span: test_span(),
            }],
            span: test_span(),
        };
        // The function creates its own scope, so "x" = "inner"
        assert_eq!(
            eval_pure_fn(&func, vec!["inner".into()], &test_env(), &empty_fns()),
            "inner"
        );
    }

    #[test]
    fn eval_fn_params_not_visible_after_return() {
        let mut vars = VarScope::new();
        vars.insert("x".into(), "before".into());
        let fns = PureFnTable::new();
        let fn_id = DiagFnId {
            module: ModulePath("m".into()),
            name: "f".into(),
            arity: 1,
        };
        fns.insert(
            fn_id.clone(),
            Ok(IrPureFn::UserDefined {
                name: test_ident("f"),
                params: vec![test_ident("x")],
                body: vec![IrPureStmt::Expr {
                    expr: var_expr("x"),
                    span: test_span(),
                }],
                span: test_span(),
            }),
        );
        // Call f("inner"), then check outer x is still "before"
        let call_arg = str_expr(vec![lit("inner")]);
        let expr = call_expr("f", fn_id, vec![call_arg]);
        assert_eq!(eval_pure_expr(&expr, &vars, &test_env(), &fns), "inner");
        assert_eq!(vars.get("x"), Some("before"));
    }

    #[test]
    fn eval_fn_empty_body() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&func, vec![], &test_env(), &empty_fns()), "");
    }

    #[test]
    fn eval_fn_last_expr_is_return() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Expr {
                    expr: str_expr(vec![lit("ignored")]),
                    span: test_span(),
                },
                IrPureStmt::Expr {
                    expr: str_expr(vec![lit("returned")]),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(&func, vec![], &test_env(), &empty_fns()),
            "returned"
        );
    }

    #[test]
    fn eval_fn_multiple_params() {
        let fns = fns_with_builtins(&[("replace", 3)]);
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![test_ident("a"), test_ident("b")],
            body: vec![IrPureStmt::Expr {
                expr: str_expr(vec![var_part("a"), var_part("b")]),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(
                &func,
                vec!["hello".into(), "world".into()],
                &test_env(),
                &fns
            ),
            "helloworld"
        );
    }

    #[test]
    fn eval_fn_param_overrides_outer_var() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![test_ident("x")],
            body: vec![IrPureStmt::Expr {
                expr: var_expr("x"),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(&func, vec!["2".into()], &test_env(), &empty_fns()),
            "2"
        );
    }

    #[test]
    fn eval_fn_last_stmt_is_let_returns_value() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![IrPureStmt::Let {
                stmt: IrPureLetStmt::new(test_ident("x"), Some(str_pure_expr("val")), test_span()),
                span: test_span(),
            }],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(&func, vec![], &test_env(), &empty_fns()),
            "val"
        );
    }

    #[test]
    fn eval_fn_last_stmt_is_assign_returns_value() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(
                        test_ident("x"),
                        Some(str_pure_expr("old")),
                        test_span(),
                    ),
                    span: test_span(),
                },
                IrPureStmt::Assign {
                    stmt: IrPureAssignStmt::new(test_ident("x"), str_pure_expr("new"), test_span()),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(
            eval_pure_fn(&func, vec![], &test_env(), &empty_fns()),
            "new"
        );
    }

    #[test]
    fn eval_fn_let_without_value() {
        let func = IrPureFn::UserDefined {
            name: test_ident("f"),
            params: vec![],
            body: vec![
                IrPureStmt::Let {
                    stmt: IrPureLetStmt::new(test_ident("x"), None, test_span()),
                    span: test_span(),
                },
                IrPureStmt::Expr {
                    expr: var_expr("x"),
                    span: test_span(),
                },
            ],
            span: test_span(),
        };
        assert_eq!(eval_pure_fn(&func, vec![], &test_env(), &empty_fns()), "");
    }
}
