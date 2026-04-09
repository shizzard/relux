// Tests extracted from relux-ir/src/tables.rs
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

fn dummy_span() -> IrSpan {
    IrSpan::synthetic()
}

#[test]
fn local_table_insert_and_get() {
    let registry = SharedTable::new();
    registry.insert("global_a".to_string(), 42);
    let mut lt = LocalTable::new(registry);
    lt.insert("local_a".to_string(), "global_a".to_string(), dummy_span());
    assert_eq!(lt.get(&"local_a".to_string()), Some(&42));
}

#[test]
fn local_table_get_missing_local_returns_none() {
    let registry: SharedTable<String, i32> = SharedTable::new();
    let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
    assert_eq!(lt.get(&"missing".to_string()), None);
}

#[test]
fn local_table_get_missing_global_returns_none() {
    let registry: SharedTable<String, i32> = SharedTable::new();
    let mut lt = LocalTable::new(registry);
    lt.insert(
        "local".to_string(),
        "not_in_registry".to_string(),
        dummy_span(),
    );
    assert_eq!(lt.get(&"local".to_string()), None);
}

#[test]
fn local_table_multiple_locals_same_global() {
    let registry = SharedTable::new();
    registry.insert("g".to_string(), 99);
    let mut lt = LocalTable::new(registry);
    lt.insert("a".to_string(), "g".to_string(), dummy_span());
    lt.insert("b".to_string(), "g".to_string(), dummy_span());
    assert_eq!(lt.get(&"a".to_string()), Some(&99));
    assert_eq!(lt.get(&"b".to_string()), Some(&99));
}

#[test]
fn local_table_insert_overwrites() {
    let registry = SharedTable::new();
    registry.insert("g1".to_string(), 1);
    registry.insert("g2".to_string(), 2);
    let mut lt = LocalTable::new(registry);
    lt.insert("k".to_string(), "g1".to_string(), dummy_span());
    assert_eq!(lt.get(&"k".to_string()), Some(&1));
    lt.insert("k".to_string(), "g2".to_string(), dummy_span());
    assert_eq!(lt.get(&"k".to_string()), Some(&2));
}

#[test]
fn local_table_registry_updated_after_insert() {
    let registry = SharedTable::new();
    let mut lt = LocalTable::new(registry.clone());
    lt.insert("k".to_string(), "g".to_string(), dummy_span());
    assert_eq!(lt.get(&"k".to_string()), None);
    registry.insert("g".to_string(), 7);
    assert_eq!(lt.get(&"k".to_string()), Some(&7));
}

#[test]
fn local_table_empty() {
    let registry: SharedTable<String, i32> = SharedTable::new();
    let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
    assert_eq!(lt.get(&"anything".to_string()), None);
}

#[test]
fn local_table_get_span() {
    let registry: SharedTable<String, i32> = SharedTable::new();
    let file = FileId::new(std::path::PathBuf::from("test.relux"));
    let span = IrSpan::new(file.clone(), relux_core::Span::new(10, 20));
    let mut lt = LocalTable::new(registry);
    lt.insert("k".to_string(), "g".to_string(), span);
    let got = lt.get_span(&"k".to_string()).unwrap();
    assert_eq!(got.file(), &file);
}

#[test]
fn local_table_get_span_missing() {
    let registry: SharedTable<String, i32> = SharedTable::new();
    let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
    assert!(lt.get_span(&"missing".to_string()).is_none());
}
