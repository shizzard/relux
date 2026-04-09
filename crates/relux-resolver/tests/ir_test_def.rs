// Tests extracted from relux-ir/src/test_def.rs
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

// ─── Test lowering ────────────────────────────────────────

#[test]
fn lower_test_simple() {
    let source = r#"test "basic" {
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "basic");
}

#[test]
fn lower_test_with_starts() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
test "with needs" {
  start Db
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert!(!result.starts().is_empty());
}

#[test]
fn lower_test_with_multiple_starts() {
    let source = r#"effect Db {
  shell db {
    > db
  }
}
effect Cache {
  shell cache {
    > cache
  }
}
test "multi" {
  start Db
  start Cache
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert_eq!(result.starts().len(), 2);
}

#[test]
fn lower_test_no_timeout() {
    let source = r#"test "t" {
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert_eq!(result.name(), "t");
}

#[test]
fn lower_test_calls_fn() {
    let source = r#"fn helper() {
  > help
}
test "t" {
  shell sh {
    helper()
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    let helper_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "helper".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&helper_id).is_some());
}

#[test]
fn lower_test_calls_bif() {
    let source = r#"test "t" {
  shell sh {
    sleep("1")
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
}

#[test]
fn lower_test_with_cleanup() {
    let source = r#"test "t" {
  shell sh {
    > cmd
  }
  cleanup {
    > clean
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert!(
        result
            .body()
            .iter()
            .any(|item| matches!(item, IrTestItem::Cleanup { .. }))
    );
}

#[test]
fn lower_test_comments_stripped() {
    let source = r#"test "t" {
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert!(
        result
            .body()
            .iter()
            .all(|item| !matches!(item, IrTestItem::Start { .. }))
    );
    assert!(!result.body().is_empty());
}

// ─── Purity enforcement tests ────────────────────────────

#[test]
fn lower_test_let_rejects_impure_fn_call() {
    let source = r#"fn impure_fn() {
  > cmd
}
test "t" {
  let x = impure_fn()
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_test_let_accepts_pure_fn_call() {
    let source = r#"test "t" {
  let x = trim("hi")
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    let test = result.unwrap();
    assert!(
        test.body()
            .iter()
            .any(|item| matches!(item, IrTestItem::Let { .. }))
    );
}

#[test]
fn lower_test_let_accepts_string_literal() {
    let source = r#"test "t" {
  let x = "hello"
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
}
