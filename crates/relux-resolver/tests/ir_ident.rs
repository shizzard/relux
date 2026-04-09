// Tests extracted from relux-ir/src/ident.rs
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

#[test]
fn ir_ident_name_and_span() {
    let ident = IrIdent::new("my_var", test_span());
    assert_eq!(ident.name(), "my_var");
    assert_eq!(ident.span().span(), &relux_core::Span::new(0, 10));
}

#[test]
fn ir_ident_empty_name() {
    let ident = IrIdent::new("", test_span());
    assert_eq!(ident.name(), "");
}

// ─── Lowering tests (moved from lower.rs) ───────────────

use relux_ast::AstIdent;

#[test]
fn lower_ident_name_and_span() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast_ident = AstIdent::new("foo", Span::new(5, 8));
    let ir = IrIdent::lower(&ast_ident, &file, &mut ctx).unwrap();
    assert_eq!(ir.name(), "foo");
    assert_eq!(ir.span().span(), &Span::new(5, 8));
}

#[test]
fn lower_ident_preserves_span_file() {
    let file = FileId::new(PathBuf::from("/other/file.relux"));
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast_ident = AstIdent::new("bar", Span::new(0, 3));
    let ir = IrIdent::lower(&ast_ident, &file, &mut ctx).unwrap();
    assert_eq!(ir.span().file(), &file);
}
