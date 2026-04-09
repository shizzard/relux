// Tests extracted from relux-ir/src/stmt.rs
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

use relux_ir::IrStringPart;
use relux_ir::IrTimeout;

use std::time::Duration;

fn test_file_id() -> FileId {
    FileId::new(PathBuf::from("test.relux"))
}

fn test_span() -> IrSpan {
    IrSpan::new(test_file_id(), relux_core::Span::new(0, 10))
}

fn test_ident(name: &str) -> IrIdent {
    IrIdent::new(name, test_span())
}

#[test]
fn ir_shell_stmt_send() {
    let s = test_span();
    let stmt = IrShellStmt::Send {
        payload: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::Send { .. }));
}

#[test]
fn ir_shell_stmt_send_raw() {
    let s = test_span();
    let stmt = IrShellStmt::SendRaw {
        payload: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::SendRaw { .. }));
}

#[test]
fn ir_shell_stmt_match_regex() {
    let s = test_span();
    let stmt = IrShellStmt::MatchRegex {
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::MatchRegex { .. }));
}

#[test]
fn ir_shell_stmt_match_literal() {
    let s = test_span();
    let stmt = IrShellStmt::MatchLiteral {
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::MatchLiteral { .. }));
}

#[test]
fn ir_shell_stmt_timed_match_regex() {
    let s = test_span();
    let timeout = IrTimeout::Tolerance {
        duration: Duration::from_secs(5),
        multiplier: 1.0,
        span: s.clone(),
    };
    let stmt = IrShellStmt::TimedMatchRegex {
        timeout,
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::TimedMatchRegex { .. }));
}

#[test]
fn ir_shell_stmt_timed_match_literal() {
    let s = test_span();
    let timeout = IrTimeout::Assertion {
        duration: Duration::from_secs(2),
        span: s.clone(),
    };
    let stmt = IrShellStmt::TimedMatchLiteral {
        timeout,
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::TimedMatchLiteral { .. }));
}

#[test]
fn ir_shell_stmt_timeout() {
    let s = test_span();
    let timeout = IrTimeout::Tolerance {
        duration: Duration::from_secs(10),
        multiplier: 1.0,
        span: s.clone(),
    };
    let stmt = IrShellStmt::Timeout { timeout, span: s };
    assert!(matches!(stmt, IrShellStmt::Timeout { .. }));
}

#[test]
fn ir_shell_stmt_fail_regex() {
    let s = test_span();
    let stmt = IrShellStmt::FailRegex {
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::FailRegex { .. }));
}

#[test]
fn ir_shell_stmt_fail_literal() {
    let s = test_span();
    let stmt = IrShellStmt::FailLiteral {
        pattern: IrInterpolation::new(vec![], s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::FailLiteral { .. }));
}

#[test]
fn ir_shell_stmt_clear_fail_pattern() {
    let stmt = IrShellStmt::ClearFailPattern { span: test_span() };
    assert!(matches!(stmt, IrShellStmt::ClearFailPattern { .. }));
}

#[test]
fn ir_shell_stmt_buffer_reset() {
    let stmt = IrShellStmt::BufferReset { span: test_span() };
    assert!(matches!(stmt, IrShellStmt::BufferReset { .. }));
}

#[test]
fn ir_shell_stmt_let() {
    let s = test_span();
    let stmt = IrShellStmt::Let {
        stmt: IrLetStmt::new(test_ident("x"), None, s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::Let { .. }));
}

#[test]
fn ir_shell_stmt_assign() {
    let s = test_span();
    let val = IrExpr::Var {
        name: "y".into(),
        span: s.clone(),
    };
    let stmt = IrShellStmt::Assign {
        stmt: IrAssignStmt::new(test_ident("x"), val, s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrShellStmt::Assign { .. }));
}

#[test]
fn ir_shell_stmt_expr() {
    let s = test_span();
    let expr = IrExpr::Var {
        name: "x".into(),
        span: s.clone(),
    };
    let stmt = IrShellStmt::Expr { expr, span: s };
    assert!(matches!(stmt, IrShellStmt::Expr { .. }));
}

#[test]
fn ir_pure_stmt_let() {
    let s = test_span();
    let stmt = IrPureStmt::Let {
        stmt: IrPureLetStmt::new(test_ident("x"), None, s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrPureStmt::Let { .. }));
}

#[test]
fn ir_pure_stmt_assign() {
    let s = test_span();
    let val = IrPureExpr::Var {
        name: "y".into(),
        span: s.clone(),
    };
    let stmt = IrPureStmt::Assign {
        stmt: IrPureAssignStmt::new(test_ident("x"), val, s.clone()),
        span: s,
    };
    assert!(matches!(stmt, IrPureStmt::Assign { .. }));
}

#[test]
fn ir_pure_stmt_expr() {
    let s = test_span();
    let expr = IrPureExpr::Var {
        name: "x".into(),
        span: s.clone(),
    };
    let stmt = IrPureStmt::Expr { expr, span: s };
    assert!(matches!(stmt, IrPureStmt::Expr { .. }));
}

// ─── Shell statement lowering ─────────────────────────────

#[test]
fn lower_let_stmt_with_value() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  let x = \"val\"\n}\n");
    if let relux_ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
        let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
        assert_eq!(ir.name().name(), "x");
        assert!(ir.value().is_some());
    } else {
        panic!("expected Let");
    }
}

#[test]
fn lower_let_stmt_no_value() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  let x\n}\n");
    if let relux_ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
        let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
        assert_eq!(ir.name().name(), "x");
        assert!(ir.value().is_none());
    } else {
        panic!("expected Let");
    }
}

#[test]
fn lower_let_stmt_with_call_value() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  let x = trim(\"v\")\n}\n");
    if let relux_ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
        let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir.value(), Some(IrExpr::Call { .. })));
    } else {
        panic!("expected Let");
    }
}

#[test]
fn lower_assign_stmt_basic() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  x = \"val\"\n}\n");
    if let relux_ast::AstStmt::Assign {
        stmt: assign_stmt, ..
    } = &stmt
    {
        let ir = IrAssignStmt::lower(assign_stmt, &file, &mut ctx).unwrap();
        assert_eq!(ir.name().name(), "x");
    } else {
        panic!("expected Assign");
    }
}

#[test]
fn lower_assign_stmt_with_call() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  x = trim(\"v\")\n}\n");
    if let relux_ast::AstStmt::Assign {
        stmt: assign_stmt, ..
    } = &stmt
    {
        let ir = IrAssignStmt::lower(assign_stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir.value(), IrExpr::Call { .. }));
    } else {
        panic!("expected Assign");
    }
}

#[test]
fn lower_send_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  > cmd\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::Send { .. }));
}

#[test]
fn lower_send_stmt_with_var() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  > echo ${x}\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::Send { payload, .. } = &ir {
        assert!(
            payload
                .parts()
                .iter()
                .any(|p| matches!(p, IrStringPart::Var { .. }))
        );
    } else {
        panic!("expected Send");
    }
}

#[test]
fn lower_send_raw_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  => raw\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::SendRaw { .. }));
}

#[test]
fn lower_match_regex_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? pattern\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::MatchRegex { .. }));
}

#[test]
fn lower_match_regex_with_var() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? ${prefix}.*\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::MatchRegex { pattern, .. } = &ir {
        assert!(
            pattern
                .parts()
                .iter()
                .any(|p| matches!(p, IrStringPart::Var { .. }))
        );
    } else {
        panic!("expected MatchRegex");
    }
}

#[test]
fn lower_match_literal_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <= text\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::MatchLiteral { .. }));
}

#[test]
fn lower_match_literal_with_var() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <= ${expected}\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::MatchLiteral { pattern, .. } = &ir {
        assert!(
            pattern
                .parts()
                .iter()
                .any(|p| matches!(p, IrStringPart::Var { .. }))
        );
    } else {
        panic!("expected MatchLiteral");
    }
}

#[test]
fn lower_timed_match_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <~5s? pattern\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::TimedMatchRegex { .. }));
}

#[test]
fn lower_timed_match_literal() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <~5s= text\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::TimedMatchLiteral { .. }));
}

#[test]
fn lower_timed_match_milliseconds() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <~500ms? pat\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::TimedMatchRegex { timeout, .. } = &ir {
        assert_eq!(
            timeout.raw_duration(),
            std::time::Duration::from_millis(500)
        );
    } else {
        panic!("expected TimedMatchRegex");
    }
}

#[test]
fn lower_timed_match_assertion() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <@2s? pat\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::TimedMatchRegex { timeout, .. } = &ir {
        assert!(timeout.is_assertion());
    } else {
        panic!("expected TimedMatchRegex");
    }
}

#[test]
fn lower_timeout_stmt_tolerance() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  ~10s\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::Timeout { timeout, .. } = &ir {
        assert!(!timeout.is_assertion());
        assert_eq!(timeout.raw_duration(), std::time::Duration::from_secs(10));
    } else {
        panic!("expected Timeout, got {:?}", ir);
    }
}

#[test]
fn lower_timeout_stmt_assertion() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  @5s\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    if let IrShellStmt::Timeout { timeout, .. } = &ir {
        assert!(timeout.is_assertion());
        assert_eq!(timeout.raw_duration(), std::time::Duration::from_secs(5));
    } else {
        panic!("expected Timeout, got {:?}", ir);
    }
}

#[test]
fn lower_fail_regex_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !? pattern\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::FailRegex { .. }));
}

#[test]
fn lower_fail_literal_stmt() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  != text\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::FailLiteral { .. }));
}

#[test]
fn lower_buffer_reset_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <?\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::BufferReset { .. }));
}

#[test]
fn lower_buffer_reset_literal() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <=\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::BufferReset { .. }));
}

#[test]
fn lower_clear_fail_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !?\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::ClearFailPattern { .. }));
}

#[test]
fn lower_clear_fail_literal() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !=\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrShellStmt::ClearFailPattern { .. }));
}

#[test]
fn lower_comments_pass_through() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let module = parse_module("fn t() {\n  // a comment\n  > cmd\n}\n");
    if let AstItem::Fn { def, .. } = &module.items[0].node {
        let stmts: Vec<IrShellStmt> = def
            .body
            .iter()
            .map(|s| IrShellStmt::lower(&s.node, &file, &mut ctx).unwrap())
            .collect();
        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0], IrShellStmt::Comment { .. }));
        assert!(matches!(stmts[1], IrShellStmt::Send { .. }));
    } else {
        panic!("expected fn");
    }
}

// ─── Pure statement lowering ──────────────────────────────

#[test]
fn lower_pure_stmt_let() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  let x = \"v\"\n}\n");
    let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrPureStmt::Let { .. }));
}

#[test]
fn lower_pure_stmt_assign() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  x = \"v\"\n}\n");
    let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrPureStmt::Assign { .. }));
}

#[test]
fn lower_pure_stmt_expr() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  trim(\"v\")\n}\n");
    let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrPureStmt::Expr { .. }));
}

#[test]
fn lower_pure_stmt_rejects_send() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  > cmd\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_send_raw() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  => cmd\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_match_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? pat\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_match_literal() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <= text\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_timed_match() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <~5s? pat\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_timeout() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  ~10s\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_fail_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !? pat\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_fail_literal() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  != text\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_buffer_reset() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <?\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_pure_stmt_rejects_clear_fail() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !?\n}\n");
    let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}
