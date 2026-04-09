// Tests extracted from relux-ir/src/regex_validate.rs
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

#[test]
fn lower_valid_regex() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? hello\\s+world\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    assert!(ir.is_ok());
}

#[test]
fn lower_invalid_regex_match() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_invalid_regex_fail() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  !? [unclosed\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_invalid_regex_timed_match() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <~5s? [unclosed\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_invalid_regex_includes_pattern() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    if let Err(LoweringBail::Invalid(inner)) = &ir {
        if let InvalidReport::InvalidRegex { pattern, .. } = inner.as_ref() {
            assert!(pattern.contains("[unclosed"));
        } else {
            panic!("expected InvalidRegex, got {:?}", ir);
        }
    } else {
        panic!("expected InvalidRegex, got {:?}", ir);
    }
}

#[test]
fn lower_invalid_regex_includes_error_message() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    if let Err(LoweringBail::Invalid(inner)) = &ir {
        if let InvalidReport::InvalidRegex { error, .. } = inner.as_ref() {
            assert!(!error.is_empty());
        } else {
            panic!("expected InvalidRegex, got {:?}", ir);
        }
    } else {
        panic!("expected InvalidRegex, got {:?}", ir);
    }
}

#[test]
fn lower_regex_with_interpolation_not_validated() {
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    push_test_scope(&mut ctx, "tests/a");
    let file = file_id_for(&ctx, "tests/a");
    let stmt = extract_first_stmt("fn t() {\n  <? ^${prefix}\n}\n");
    let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
    assert!(ir.is_ok());
}
