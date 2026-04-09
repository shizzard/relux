// Tests extracted from relux-ir/src/shallow_env.rs
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

fn make_env(keys: &[&str]) -> LayeredEnv {
    let map: HashMap<String, String> = keys
        .iter()
        .map(|k| (k.to_string(), String::new()))
        .collect();
    LayeredEnv::from(Env::from_map(map))
}

#[test]
fn root_contains_env_keys() {
    let env = make_env(&["HOME", "PATH"]);
    let root = ShallowLayeredEnv::root(&env);
    assert!(root.contains("HOME"));
    assert!(root.contains("PATH"));
    assert!(!root.contains("MISSING"));
}

#[test]
fn child_sees_own_and_parent() {
    let env = make_env(&["BASE"]);
    let root = Arc::new(ShallowLayeredEnv::root(&env));
    let child = ShallowLayeredEnv::child(root, ["OVERLAY".to_string()]);
    assert!(child.contains("BASE"));
    assert!(child.contains("OVERLAY"));
    assert!(!child.contains("MISSING"));
}

#[test]
fn with_name_adds_single_binding() {
    let env = make_env(&["BASE"]);
    let root = Arc::new(ShallowLayeredEnv::root(&env));
    let extended = ShallowLayeredEnv::with_name(&root, "FOO".to_string());
    assert!(extended.contains("BASE"));
    assert!(extended.contains("FOO"));
    assert!(!extended.contains("BAR"));
}

#[test]
fn three_level_chain() {
    let env = make_env(&["L0"]);
    let l0 = Arc::new(ShallowLayeredEnv::root(&env));
    let l1 = Arc::new(ShallowLayeredEnv::child(l0, ["L1".to_string()]));
    let l2 = ShallowLayeredEnv::child(l1, ["L2".to_string()]);
    assert!(l2.contains("L0"));
    assert!(l2.contains("L1"));
    assert!(l2.contains("L2"));
    assert!(!l2.contains("L3"));
}
