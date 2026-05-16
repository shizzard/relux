// Tests extracted from relux-ir/src/plan.rs
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

use crate::IrTestItem;
use std::time::Duration;

use relux_ir::IrTimeout;

fn test_file_id() -> FileId {
    FileId::new(PathBuf::from("test.relux"))
}

fn test_span() -> IrSpan {
    IrSpan::new(test_file_id(), relux_core::Span::new(0, 10))
}

fn test_def() -> DefinitionRef {
    DefinitionRef::Test {
        name: "test1".into(),
        module: ModulePath("tests".into()),
    }
}

#[test]
fn plan_runnable_variant() {
    let s = test_span();
    let meta = TestMeta::new("test1", None, None, test_def(), s.clone());
    let test = IrTest::new("test1", vec![], vec![], s);
    let plan = Plan::Runnable {
        meta,
        test,
        warnings: vec![],
    };
    assert!(matches!(plan, Plan::Runnable { .. }));
}

#[test]
fn plan_runnable_with_warnings() {
    let s = test_span();
    let meta = TestMeta::new("test1", None, None, test_def(), s.clone());
    let test = IrTest::new("test1", vec![], vec![], s);
    let w = WarningId {
        id: "test-warn-0001".into(),
    };
    let plan = Plan::Runnable {
        meta,
        test,
        warnings: vec![w],
    };
    if let Plan::Runnable { warnings, .. } = &plan {
        assert_eq!(warnings.len(), 1);
    }
}

#[test]
fn plan_skipped_variant() {
    let meta = TestMeta::new("test1", None, None, test_def(), test_span());
    let cause = CauseId::generate("test", "skip", 0, "skip");
    let plan = Plan::Skipped {
        meta,
        causes: vec![cause],
        warnings: vec![],
    };
    assert!(matches!(plan, Plan::Skipped { .. }));
}

#[test]
fn plan_skipped_multiple_causes() {
    let meta = TestMeta::new("test1", None, None, test_def(), test_span());
    let c1 = CauseId::generate("test", "a", 0, "skip");
    let c2 = CauseId::generate("test", "b", 1, "skip");
    let plan = Plan::Skipped {
        meta,
        causes: vec![c1, c2],
        warnings: vec![],
    };
    if let Plan::Skipped { causes, .. } = &plan {
        assert_eq!(causes.len(), 2);
    }
}

#[test]
fn plan_invalid_variant() {
    let meta = TestMeta::new("test1", None, None, test_def(), test_span());
    let cause = CauseId::generate("test", "err", 0, "invalid");
    let plan = Plan::Invalid {
        meta,
        causes: vec![cause],
        warnings: vec![],
    };
    assert!(matches!(plan, Plan::Invalid { .. }));
}

#[test]
fn plan_invalid_multiple_causes() {
    let meta = TestMeta::new("test1", None, None, test_def(), test_span());
    let c1 = CauseId::generate("test", "a", 0, "err1");
    let c2 = CauseId::generate("test", "b", 1, "err2");
    let plan = Plan::Invalid {
        meta,
        causes: vec![c1, c2],
        warnings: vec![],
    };
    if let Plan::Invalid { causes, .. } = &plan {
        assert_eq!(causes.len(), 2);
    }
}

#[test]
fn test_meta_with_all_fields() {
    let s = test_span();
    let timeout = IrTimeout::Tolerance {
        duration: Duration::from_secs(5),
        multiplier: 1.0,
        span: s.clone(),
    };
    let meta = TestMeta::new("test1", Some("docs".into()), Some(timeout), test_def(), s);
    assert_eq!(meta.name(), "test1");
    assert_eq!(meta.docstring(), Some("docs"));
    assert!(meta.timeout().is_some());
}

#[test]
fn test_meta_minimal() {
    let meta = TestMeta::new("test1", None, None, test_def(), test_span());
    assert_eq!(meta.name(), "test1");
    assert_eq!(meta.docstring(), None);
    assert!(meta.timeout().is_none());
}

// ─── Plan building: happy paths ────────────────────────────

#[test]
fn plan_simple_test() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "basic" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
    assert_eq!(plan_name(&suite.plans[0]), "basic");
}

#[test]
fn plan_test_with_fn_call() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn greet() {
  > echo hello
}

test "with fn" {
  shell sh {
    greet()
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn plan_test_with_pure_fn() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"pure fn greeting() {
  "hello"
}

test "with pure" {
  let g = greeting()
  shell sh {
    > echo ${g}
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn plan_test_with_bif() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "with bif" {
  let v = trim("  hello  ")
  shell sh {
    > echo ${v}
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn plan_test_with_effect() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Setup {
  shell sh {
    > echo setup
  }
}

test "with effect" {
  start Setup
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn plan_test_with_docstring() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "documented" {
  """This test does things"""
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert_eq!(
        suite.plans[0].meta().docstring(),
        Some("This test does things")
    );
}

#[test]
fn plan_test_without_docstring() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "no doc" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert_eq!(suite.plans[0].meta().docstring(), None);
}

#[test]
fn plan_test_with_timeout() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "timed" ~10s {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert!(suite.plans[0].meta().timeout().is_some());
}

#[test]
fn plan_test_without_timeout() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "no timeout" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(suite.plans[0].meta().timeout().is_none());
}

#[test]
fn plan_test_with_cleanup() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "with cleanup" {
  shell sh {
    > echo hello
  }
  cleanup {
    > echo bye
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    if let Plan::Runnable { test, .. } = &suite.plans[0] {
        let has_cleanup = test
            .body()
            .iter()
            .any(|item| matches!(item, IrTestItem::Cleanup { .. }));
        assert!(has_cleanup);
    }
}

#[test]
fn plan_test_with_let() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "with let" {
  let x = "hello"
  shell sh {
    > echo ${x}
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn plan_multiple_tests() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "first" {
  shell sh {
    > echo 1
  }
}

test "second" {
  shell sh {
    > echo 2
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_runnable(&suite.plans[1]));
    assert_eq!(plan_name(&suite.plans[0]), "first");
    assert_eq!(plan_name(&suite.plans[1]), "second");
}

#[test]
fn plan_multiple_effects() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  shell db_sh {
    > echo db setup
  }
}

effect Cache {
  shell cache_sh {
    > echo cache setup
  }
}

test "multi effects" {
  start Db
  start Cache
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    if let Plan::Runnable { test, .. } = &suite.plans[0] {
        assert_eq!(test.starts().len(), 2);
    }
}

// ─── Plan building: skip paths ─────────────────────────────

#[test]
fn plan_skip_unconditional() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn plan_skip_bare_condition() {
    let mut env = HashMap::new();
    env.insert("SKIP_ME".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if SKIP_ME
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn plan_skip_has_cause_id() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    if let Plan::Skipped { causes, .. } = &suite.plans[0] {
        assert!(!causes.is_empty());
    } else {
        panic!("expected Skipped plan");
    }
}

#[test]
fn plan_skip_fn_dep_propagates() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
fn helper() {
  > echo hello
}

test "t" {
  shell sh {
    helper()
  }
}
"#,
    )]);
    // Skipped fn dep → test is also skipped (propagation)
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn plan_skip_effect_dep_propagates() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
effect Setup {
  shell sh {
    > echo setup
  }
}

test "t" {
  start Setup
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    // Skipped effect dep → test is also skipped
    assert!(is_skipped(&suite.plans[0]));
}

// ─── Plan building: invalid paths ──────────────────────────

#[test]
fn plan_invalid_undefined_fn() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn plan_invalid_undefined_effect() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  start NonExistent
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn plan_invalid_fn_cycle() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn a() {
  b()
}

fn b() {
  a()
}

test "t" {
  shell sh {
    a()
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn plan_invalid_has_cause_id() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
    )]);
    if let Plan::Invalid { causes, .. } = &suite.plans[0] {
        assert!(!causes.is_empty());
    } else {
        panic!("expected Invalid plan");
    }
}

#[test]
fn plan_invalid_purity_violation() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"pure fn bad() {
  > echo side-effect
}

test "t" {
  let v = bad()
  shell sh {
    > echo ${v}
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

// ─── Plan building: precedence ─────────────────────────────

#[test]
fn plan_own_skip_skips_body_lowering() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

// ─── Suite assembly ────────────────────────────────────────

#[test]
fn suite_has_all_plans() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t1" {
  shell sh {
    > echo 1
  }
}

test "t2" {
  shell sh {
    > echo 2
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
}

#[test]
fn suite_has_source_table() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    let has_entries = !suite.tables.sources.is_empty();
    assert!(has_entries);
}

#[test]
fn suite_has_env() {
    let mut env = HashMap::new();
    env.insert("TEST_KEY".into(), "test_val".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert_eq!(suite.env.get("TEST_KEY"), Some("test_val"));
}

#[test]
fn suite_empty() {
    let suite = resolve_source_no_env(&[(
        "lib/helpers",
        r#"fn greet() {
  > echo hello
}
"#,
    )]);
    assert!(suite.plans.is_empty());
}

#[test]
fn suite_mixed_variants() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "good" {
  shell sh {
    > echo hello
  }
}

# skip
test "skipped" {
  shell sh {
    > echo skip
  }
}

test "bad" {
  shell sh {
    nonexistent()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 3);
    let good = suite.plans.iter().find(|p| plan_name(p) == "good").unwrap();
    let skipped = suite
        .plans
        .iter()
        .find(|p| plan_name(p) == "skipped")
        .unwrap();
    let bad = suite.plans.iter().find(|p| plan_name(p) == "bad").unwrap();
    assert!(is_runnable(good));
    assert!(is_skipped(skipped));
    assert!(is_invalid(bad));
}

// ─── Effect deduplication ──────────────────────────────────

#[test]
fn effect_start_no_overlay_same_as_empty_overlay() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  shell db_sh {
    > echo db
  }
}

test "t1" {
  start Db
  shell sh {
    > echo 1
  }
}

test "t2" {
  start Db {}
  shell sh {
    > echo 2
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(suite.plans.iter().all(is_runnable));
}

// ─── build_all_plans ordering ──────────────────────────────

#[test]
fn build_all_plans_tests_within_module_in_order() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "first" {
  shell sh {
    > echo 1
  }
}

test "second" {
  shell sh {
    > echo 2
  }
}

test "third" {
  shell sh {
    > echo 3
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 3);
    assert_eq!(plan_name(&suite.plans[0]), "first");
    assert_eq!(plan_name(&suite.plans[1]), "second");
    assert_eq!(plan_name(&suite.plans[2]), "third");
}

// ─── Purity enforcement (end-to-end plan building) ───────

#[test]
fn plan_test_let_impure_fn_invalidates() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn impure_fn() {
  > cmd
}
test "t" {
  let x = impure_fn()
  shell sh {
    > cmd
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn plan_effect_let_impure_fn_invalidates() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn impure_fn() {
  > cmd
}
effect E {
  let x = impure_fn()
  shell sh {
    > start
  }
}
test "t" {
  start E
  shell sh {
    > cmd
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}
