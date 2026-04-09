// Tests extracted from relux-ir/src/expr.rs
#![allow(unused_imports)]
use relux_ast::*;
use relux_core::Span;
use relux_core::Spanned;
use relux_core::diagnostics::*;
use relux_core::pure::*;
use relux_core::table::FileId;
use relux_core::table::SharedTable;
use relux_core::table::SourceTable;
use relux_ir::evaluator::*;
use relux_ir::lowering_context::*;
use relux_ir::marker::*;
use relux_ir::regex_validate::*;
use relux_ir::shallow_env::*;
use relux_ir::*;
use relux_resolver::lower::test_helpers::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn test_file_id() -> FileId {
    FileId::new(PathBuf::from("test.relux"))
}

fn test_span() -> IrSpan {
    IrSpan::new(test_file_id(), relux_core::Span::new(0, 10))
}

fn test_span2() -> IrSpan {
    IrSpan::new(test_file_id(), relux_core::Span::new(20, 30))
}

#[test]
fn ir_expr_string_variant() {
    let s = test_span();
    let e = IrExpr::String {
        value: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(e, IrExpr::String { .. }));
}

#[test]
fn ir_expr_var_variant() {
    let e = IrExpr::Var {
        name: "x".into(),
        span: test_span(),
    };
    assert!(matches!(e, IrExpr::Var { .. }));
    let _ = e.span();
}

#[test]
fn ir_expr_call_variant() {
    let s = test_span();
    let name = IrIdent::new("foo", s.clone());
    let resolved = FnId {
        module: ModulePath("test".into()),
        name: "foo".into(),
        arity: 0,
    };
    let call = IrCallExpr::new(name, resolved, vec![], s.clone());
    let e = IrExpr::Call { call, span: s };
    assert!(matches!(e, IrExpr::Call { .. }));
}

#[test]
fn ir_expr_capture_ref_variant() {
    let e = IrExpr::CaptureRef {
        index: 1,
        span: test_span(),
    };
    assert!(matches!(e, IrExpr::CaptureRef { index: 1, .. }));
}

#[test]
fn ir_expr_capture_ref_index_zero() {
    let e = IrExpr::CaptureRef {
        index: 0,
        span: test_span(),
    };
    assert!(matches!(e, IrExpr::CaptureRef { index: 0, .. }));
}

#[test]
fn ir_pure_expr_all_three_variants() {
    let s = test_span();
    let _ = IrPureExpr::String {
        value: IrInterpolation::new(vec![], s.clone()),
        span: s.clone(),
    };
    let _ = IrPureExpr::Var {
        name: "x".into(),
        span: s.clone(),
    };
    let name = IrIdent::new("f", s.clone());
    let resolved = FnId {
        module: ModulePath("test".into()),
        name: "f".into(),
        arity: 0,
    };
    let call = IrPureCallExpr::new(name, resolved, vec![], s.clone());
    let _ = IrPureExpr::Call { call, span: s };
}

#[test]
fn ir_node_enum_span_each_variant() {
    let s = test_span();
    let expr_str = IrExpr::String {
        value: IrInterpolation::new(vec![], s.clone()),
        span: s.clone(),
    };
    let expr_var = IrExpr::Var {
        name: "x".into(),
        span: s.clone(),
    };
    let _ = expr_str.span();
    let _ = expr_var.span();
}

#[test]
fn ir_node_enum_span_different_values() {
    let s1 = test_span();
    let s2 = test_span2();
    let e1 = IrExpr::Var {
        name: "a".into(),
        span: s1,
    };
    let e2 = IrExpr::Var {
        name: "b".into(),
        span: s2,
    };
    assert_eq!(e1.span().span(), &relux_core::Span::new(0, 10));
    assert_eq!(e2.span().span(), &relux_core::Span::new(20, 30));
}

// ─── Expression lowering ──────────────────────────────────

#[test]
fn lower_expr_string() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let expr = extract_let_expr("fn x() {\n  let v = \"hello\"\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), IrExpr::String { .. }));
}

#[test]
fn lower_expr_var() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let expr = extract_let_expr("fn x() {\n  let v = name\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), IrExpr::Var { .. }));
}

#[test]
fn lower_expr_call_resolved() {
    let mut ctx = ctx_with_source(
        r#"fn foo(x) {
  > ${x}
}
fn bar() {
  let v = foo("a")
}
"#,
    );
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let expr = extract_let_expr("fn t() {\n  let v = foo(\"a\")\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), IrExpr::Call { .. }));
    let fn_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "foo".into(),
        arity: 1,
    };
    assert!(ctx.functions().get(&fn_id).is_some());
}

#[test]
fn lower_expr_call_bif() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let expr = extract_let_expr("fn t() {\n  let v = trim(\"hello\")\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    if let IrExpr::Call { call, .. } = result.unwrap() {
        assert_eq!(call.name().name(), "trim");
    } else {
        panic!("expected Call");
    }
}

#[test]
fn lower_expr_call_multi_arg() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let expr = extract_let_expr("fn t() {\n  let v = replace(\"s\", \"a\", \"b\")\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    if let IrExpr::Call { call, .. } = result.unwrap() {
        assert_eq!(call.args().len(), 3);
    } else {
        panic!("expected Call");
    }
}

#[test]
fn lower_expr_call_nested() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let expr = extract_let_expr("fn t() {\n  let v = trim(upper(\"x\"))\n}\n");
    let result = IrExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    if let IrExpr::Call { call, .. } = result.unwrap() {
        assert_eq!(call.name().name(), "trim");
        assert!(matches!(&call.args()[0], IrExpr::Call { .. }));
    } else {
        panic!("expected Call");
    }
}

#[test]
fn lower_expr_capture_ref() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::CaptureRef {
        index: 1,
        span: Span::new(0, 2),
    };
    let result = IrExpr::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(result, IrExpr::CaptureRef { index: 1, .. }));
}

#[test]
fn lower_expr_capture_ref_multi_digit() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::CaptureRef {
        index: 12,
        span: Span::new(0, 3),
    };
    let result = IrExpr::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(result, IrExpr::CaptureRef { index: 12, .. }));
}

#[test]
fn lower_pure_expr_string() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::String {
        interp: AstInterpolation {
            parts: vec![AstStringPart::Literal {
                value: "hi".into(),
                span: Span::new(1, 3),
            }],
            span: Span::new(0, 4),
        },
        span: Span::new(0, 4),
    };
    let result = IrPureExpr::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(result, IrPureExpr::String { .. }));
}

#[test]
fn lower_pure_expr_var() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::Var {
        name: "x".into(),
        span: Span::new(0, 1),
    };
    let result = IrPureExpr::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(result, IrPureExpr::Var { .. }));
}

#[test]
fn lower_pure_expr_call() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let expr = extract_let_expr("fn t() {\n  let v = trim(\"x\")\n}\n");
    let result = IrPureExpr::lower(&expr, &file, &mut ctx);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), IrPureExpr::Call { .. }));
}

#[test]
fn lower_pure_expr_rejects_capture_ref() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::CaptureRef {
        index: 1,
        span: Span::new(0, 2),
    };
    let result = IrPureExpr::lower(&ast, &file, &mut ctx);
    assert!(result.is_err());
    if let Err(LoweringBail::Invalid(_)) = result {
        // OK
    } else {
        panic!("expected PurityViolation, got {:?}", result);
    }
}

#[test]
fn lower_pure_expr_string_rejects_capture_ref_in_parts() {
    let mut ctx = ctx_with_source("fn dummy() {}");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");

    let ast = AstExpr::String {
        interp: AstInterpolation {
            parts: vec![AstStringPart::CaptureRef {
                index: 1,
                span: Span::new(1, 5),
            }],
            span: Span::new(0, 6),
        },
        span: Span::new(0, 6),
    };
    let result = IrPureExpr::lower(&ast, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}
