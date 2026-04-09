// Tests extracted from relux-ir/src/block.rs
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

fn test_ident(name: &str) -> IrIdent {
    IrIdent::new(name, test_span())
}

#[test]
fn ir_shell_block() {
    let s = test_span();
    let block = IrShellBlock::new(
        None,
        test_ident("sh"),
        vec![IrShellStmt::BufferReset { span: s.clone() }],
        s,
    );
    assert_eq!(block.name().name(), "sh");
    assert_eq!(block.body().len(), 1);
}

#[test]
fn ir_shell_block_empty_body() {
    let block = IrShellBlock::new(None, test_ident("sh"), vec![], test_span());
    assert!(block.body().is_empty());
}

#[test]
fn ir_cleanup_block() {
    let s = test_span();
    let block = IrCleanupBlock::new(vec![IrShellStmt::BufferReset { span: s.clone() }], s);
    assert_eq!(block.body().len(), 1);
}

#[test]
fn ir_cleanup_block_empty_body() {
    let block = IrCleanupBlock::new(vec![], test_span());
    assert!(block.body().is_empty());
}
