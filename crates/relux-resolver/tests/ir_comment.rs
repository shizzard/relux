// Tests extracted from relux-ir/src/comment.rs
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
fn ir_comment_stub() {
    let file = FileId::new(PathBuf::from("test.relux"));
    let span = IrSpan::new(file, relux_core::Span::new(0, 10));
    let comment = IrComment::new(span.clone());
    assert_eq!(comment.span().file(), span.file());
}
