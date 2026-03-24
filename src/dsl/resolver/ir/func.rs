use crate::diagnostics::{IrSpan, LoweringBail};
use crate::dsl::parser::ast::{AstFnDef, AstPureFnDef};
use crate::table::FileId;

use super::ident::IrIdent;
use super::stmt::{IrPureStmt, IrShellStmt};
use super::{IrNodeLowering, LoweringContext};

/// IrFn is an enum because builtins have no AST source.
/// IrNode is NOT implemented — Builtin has no span.
#[derive(Debug, Clone)]
pub enum IrFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrShellStmt>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}

/// IrPureFn is an enum because builtins have no AST source.
/// IrNode is NOT implemented — Builtin has no span.
#[derive(Debug, Clone)]
pub enum IrPureFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrPureStmt>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}

impl IrNodeLowering for IrFn {
    type Ast = AstFnDef;
    fn lower(
        ast: &AstFnDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let params: Vec<IrIdent> = ast
            .params
            .iter()
            .map(|p| IrIdent::lower(&p.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        let body = ast
            .body
            .iter()
            .map(|s| IrShellStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrFn::UserDefined {
            name,
            params,
            body,
            span: IrSpan::new(file.clone(), ast.span),
        })
    }
}

impl IrNodeLowering for IrPureFn {
    type Ast = AstPureFnDef;
    fn lower(
        ast: &AstPureFnDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let params: Vec<IrIdent> = ast
            .params
            .iter()
            .map(|p| IrIdent::lower(&p.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        let body = ast
            .body
            .iter()
            .map(|s| IrPureStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrPureFn::UserDefined {
            name,
            params,
            body,
            span: IrSpan::new(file.clone(), ast.span),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::FileId;
    use std::path::PathBuf;

    fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(0, 10))
    }

    fn test_ident(name: &str) -> IrIdent {
        IrIdent::new(name, test_span())
    }

    #[test]
    fn ir_fn_user_defined() {
        let s = test_span();
        let f = IrFn::UserDefined {
            name: test_ident("greet"),
            params: vec![test_ident("name")],
            body: vec![IrShellStmt::BufferReset { span: s.clone() }],
            span: s,
        };
        assert!(matches!(f, IrFn::UserDefined { .. }));
    }

    #[test]
    fn ir_fn_user_defined_empty_body() {
        let f = IrFn::UserDefined {
            name: test_ident("noop"),
            params: vec![],
            body: vec![],
            span: test_span(),
        };
        if let IrFn::UserDefined { body, .. } = &f {
            assert!(body.is_empty());
        }
    }

    #[test]
    fn ir_fn_user_defined_zero_params() {
        let f = IrFn::UserDefined {
            name: test_ident("noop"),
            params: vec![],
            body: vec![],
            span: test_span(),
        };
        if let IrFn::UserDefined { params, .. } = &f {
            assert!(params.is_empty());
        }
    }

    #[test]
    fn ir_fn_builtin() {
        let f = IrFn::Builtin {
            name: "sleep".into(),
            arity: 1,
        };
        assert!(matches!(f, IrFn::Builtin { arity: 1, .. }));
    }

    #[test]
    fn ir_fn_builtin_arity_zero() {
        let f = IrFn::Builtin {
            name: "uuid".into(),
            arity: 0,
        };
        assert!(matches!(f, IrFn::Builtin { arity: 0, .. }));
    }

    #[test]
    fn ir_pure_fn_user_defined() {
        let f = IrPureFn::UserDefined {
            name: test_ident("concat"),
            params: vec![test_ident("a"), test_ident("b")],
            body: vec![],
            span: test_span(),
        };
        assert!(matches!(f, IrPureFn::UserDefined { .. }));
    }

    #[test]
    fn ir_pure_fn_builtin() {
        let f = IrPureFn::Builtin {
            name: "env".into(),
            arity: 1,
        };
        assert!(matches!(f, IrPureFn::Builtin { .. }));
    }

    // ─── Function lowering (cacheable) ────────────────────────

    use crate::diagnostics::{CycleReport, FnId, InvalidReport, LoweringBail, ModulePath};
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_fn_simple() {
        let source = r#"fn foo(x) {
  > ${x}
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "foo".into(),
            arity: 1,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_ok());
        if let Ok(IrFn::UserDefined {
            name, params, body, ..
        }) = &result
        {
            assert_eq!(name.name(), "foo");
            assert_eq!(params.len(), 1);
            assert!(!body.is_empty());
        } else {
            panic!("expected UserDefined, got {:?}", result);
        }
    }

    #[test]
    fn lower_fn_zero_params() {
        let source = r#"fn bar() {
  > hi
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "bar".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id).unwrap();
        if let IrFn::UserDefined { params, .. } = &result {
            assert!(params.is_empty());
        } else {
            panic!("expected UserDefined");
        }
    }

    #[test]
    fn lower_fn_multiple_params() {
        let source = r#"fn f(a, b, c) {
  > ${a}
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 3,
        };
        let result = ctx.resolve_fn(&fn_id).unwrap();
        if let IrFn::UserDefined { params, .. } = &result {
            assert_eq!(params.len(), 3);
            assert_eq!(params[0].name(), "a");
            assert_eq!(params[1].name(), "b");
            assert_eq!(params[2].name(), "c");
        } else {
            panic!("expected UserDefined");
        }
    }

    #[test]
    fn lower_fn_empty_body() {
        let source = "fn f() {\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id).unwrap();
        if let IrFn::UserDefined { body, .. } = &result {
            assert!(body.is_empty());
        } else {
            panic!("expected UserDefined");
        }
    }

    #[test]
    fn lower_fn_calls_other_fn() {
        let source = r#"fn helper() {
  > help
}
fn caller() {
  helper()
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_ok());
        let helper_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "helper".into(),
            arity: 0,
        };
        assert!(ctx.functions().get(&helper_id).is_some());
    }

    #[test]
    fn lower_fn_calls_bif() {
        let source = r#"fn f() {
  sleep("1")
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_fn_calls_pure_fn() {
        let source = r#"pure fn greet(name) {
  let v = name
}
fn caller() {
  let x = greet("world")
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_fn_memoized() {
        let source = r#"fn shared() {
  > s
}
fn a() {
  shared()
}
fn b() {
  shared()
}
"#;
        let mut ctx = ctx_with_source(source);
        let a_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "a".into(),
            arity: 0,
        };
        let b_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "b".into(),
            arity: 0,
        };
        ctx.resolve_fn(&a_id).unwrap();
        ctx.resolve_fn(&b_id).unwrap();
        let shared_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "shared".into(),
            arity: 0,
        };
        assert!(ctx.functions().get(&shared_id).is_some());
    }

    #[test]
    fn lower_fn_cycle_self() {
        let source = "fn f() {\n  f()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_err());
        if let Err(LoweringBail::Invalid(InvalidReport::Cycle(CycleReport::Function { chain }))) =
            &result
        {
            assert_eq!(chain.len(), 1);
            assert_eq!(chain[0].id.name, "f");
        } else {
            panic!("expected function cycle, got {:?}", result);
        }
    }

    #[test]
    fn lower_fn_cycle_mutual() {
        let source = r#"fn a() {
  b()
}
fn b() {
  a()
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "a".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_err());
        if let Err(LoweringBail::Invalid(InvalidReport::Cycle(CycleReport::Function { chain }))) =
            &result
        {
            assert_eq!(chain.len(), 2);
        } else {
            panic!("expected function cycle, got {:?}", result);
        }
    }

    #[test]
    fn lower_fn_cycle_deep() {
        let source = r#"fn a() {
  b()
}
fn b() {
  c()
}
fn c() {
  a()
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "a".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(result.is_err());
        if let Err(LoweringBail::Invalid(InvalidReport::Cycle(CycleReport::Function { chain }))) =
            &result
        {
            assert_eq!(chain.len(), 3);
        } else {
            panic!("expected function cycle, got {:?}", result);
        }
    }

    #[test]
    fn lower_fn_undefined_call() {
        let source = "fn caller() {\n  nonexistent()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(
                InvalidReport::UndefinedFunctionCall { .. }
            ))
        ));
    }

    #[test]
    fn lower_fn_wrong_arity() {
        let source = r#"fn foo(x) {
  > ${x}
}
fn caller() {
  foo("a", "b")
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_fn(&fn_id);
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::UndefinedFunctionCall {
                ref name,
                arity: 2,
                ..
            })) if name == "foo"
        ));
    }

    #[test]
    fn lower_fn_same_name_different_arity() {
        let source = r#"fn foo() {
  > zero
}
fn foo(a) {
  > ${a}
}
"#;
        let mut ctx = ctx_with_source(source);
        let id0 = FnId {
            module: ModulePath("tests/a".into()),
            name: "foo".into(),
            arity: 0,
        };
        let id1 = FnId {
            module: ModulePath("tests/a".into()),
            name: "foo".into(),
            arity: 1,
        };
        assert!(ctx.resolve_fn(&id0).is_ok());
        assert!(ctx.resolve_fn(&id1).is_ok());
    }

    #[test]
    fn lower_fn_error_cached() {
        let source = "fn f() {\n  nonexistent()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result1 = ctx.resolve_fn(&fn_id);
        assert!(result1.is_err());
        let result2 = ctx.resolve_fn(&fn_id);
        assert!(result2.is_err());
    }

    // ─── Pure function lowering (cacheable) ───────────────────

    #[test]
    fn lower_pure_fn_simple() {
        let source = "pure fn greet(name) {\n  let v = name\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "greet".into(),
            arity: 1,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(result.is_ok());
        if let Ok(IrPureFn::UserDefined {
            name, params, body, ..
        }) = &result
        {
            assert_eq!(name.name(), "greet");
            assert_eq!(params.len(), 1);
            assert!(!body.is_empty());
        } else {
            panic!("expected UserDefined, got {:?}", result);
        }
    }

    #[test]
    fn lower_pure_fn_calls_pure_fn() {
        let source = r#"pure fn helper() {
  let v = "h"
}
pure fn caller() {
  helper()
}
"#;
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_pure_fn_calls_pure_bif() {
        let source = "pure fn f() {\n  let v = trim(\"x\")\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_pure_fn_rejects_shell_op() {
        let source = "pure fn f() {\n  > cmd\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::PurityViolation { .. }))
        ));
    }

    #[test]
    fn lower_pure_fn_rejects_impure_bif() {
        let source = "pure fn f() {\n  ctrl_c()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::PurityViolation { .. }))
        ));
    }

    #[test]
    fn lower_pure_fn_cycle() {
        let source = "pure fn f() {\n  f()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::Cycle(
                CycleReport::Function { .. }
            )))
        ));
    }

    #[test]
    fn lower_pure_fn_memoized() {
        let source = r#"pure fn shared() {
  let v = "s"
}
pure fn a() {
  shared()
}
pure fn b() {
  shared()
}
"#;
        let mut ctx = ctx_with_source(source);
        let a_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "a".into(),
            arity: 0,
        };
        let b_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "b".into(),
            arity: 0,
        };
        ctx.resolve_pure_fn(&a_id).unwrap();
        ctx.resolve_pure_fn(&b_id).unwrap();
        let shared_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "shared".into(),
            arity: 0,
        };
        assert!(ctx.pure_functions().get(&shared_id).is_some());
    }

    #[test]
    fn lower_pure_fn_transitive_purity_violation() {
        let source = "pure fn bad() {\n  > cmd\n}\npure fn caller() {\n  bad()\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "caller".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id);
        assert!(result.is_err());
    }

    #[test]
    fn lower_pure_fn_empty_body() {
        let source = "pure fn f() {\n}\n";
        let mut ctx = ctx_with_source(source);
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "f".into(),
            arity: 0,
        };
        let result = ctx.resolve_pure_fn(&fn_id).unwrap();
        if let IrPureFn::UserDefined { body, .. } = &result {
            assert!(body.is_empty());
        } else {
            panic!("expected UserDefined");
        }
    }
}
