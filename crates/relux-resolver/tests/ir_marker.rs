// Tests extracted from relux-ir/src/marker.rs
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

// ─── eval_marker: @skip ────────────────────────────────────

#[test]
fn marker_skip_unconditional() {
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
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
    assert_eq!(plan_name(&suite.plans[0]), "skipped");
}

#[test]
fn marker_skip_if_bare_truthy() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_skip_if_bare_falsy() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if MY_VAR
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_if_bare_whitespace_is_truthy() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "  ".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR
test "ws" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_skip_if_eq_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "expected".into());
    env.insert("EXPECTED".into(), "expected".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR = EXPECTED
test "eq" {
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
fn marker_skip_if_eq_no_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "other".into());
    env.insert("EXPECTED".into(), "expected".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR = EXPECTED
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_if_eq_both_empty() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if UNSET_A = UNSET_B
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_skip_if_eq_one_empty() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "val".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR = UNSET_B
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_if_regex_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "abc123".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR ? \d+
test "rx" {
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
fn marker_skip_if_regex_no_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "abc".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR ? ^\d+$
test "rx" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_if_regex_empty_value() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if UNSET ? .*
test "rx" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

// ─── eval_marker: @run ─────────────────────────────────────

#[test]
fn marker_run_unconditional() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# run
test "always" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_run_if_bare_truthy() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if MY_VAR
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_run_if_bare_falsy() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# run if MY_VAR
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_run_if_eq_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "expected".into());
    env.insert("EXPECTED".into(), "expected".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if MY_VAR = EXPECTED
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_run_if_eq_no_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "other".into());
    env.insert("EXPECTED".into(), "expected".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if MY_VAR = EXPECTED
test "run" {
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
fn marker_run_if_regex_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "123".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if MY_VAR ? ^\d+$
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_run_if_regex_no_match() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "abc".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if MY_VAR ? ^\d+$
test "run" {
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

// ─── eval_marker: unless ───────────────────────────────────

#[test]
fn marker_skip_unless_truthy() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_unless_falsy() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_run_unless_truthy() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run unless MY_VAR
test "t" {
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
fn marker_run_unless_falsy() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# run unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

// ─── eval_marker: with expressions ─────────────────────────

#[test]
fn marker_skip_if_env_var() {
    let mut env = HashMap::new();
    env.insert("CI".into(), "true".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if CI
test "ci" {
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
fn marker_skip_if_missing_env() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if CI
test "ci" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_skip_if_pure_fn_call() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"pure fn always_true() {
  "yes"
}

# skip if always_true()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_run_if_pure_fn_call_returns_empty() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"pure fn always_empty() {
  ""
}

# run if always_empty()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

// ─── eval_marker: errors ───────────────────────────────────

#[test]
fn marker_invalid_regex_in_condition() {
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "test".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR ? [invalid
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn marker_undefined_fn_in_condition() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if nonexistent()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

// ─── eval_marker: multiple markers ─────────────────────────

#[test]
fn marker_first_skip_triggers() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
# run
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_second_skip_triggers() {
    let mut env = HashMap::new();
    env.insert("CI".into(), "true".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if UNSET
# skip if CI
test "t" {
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
fn marker_none_trigger() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip if UNSET_A
# skip if UNSET_B
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_flaky_unconditional_sets_flag() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# flaky
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_flaky(&suite.plans[0]));
}

#[test]
fn marker_flaky_if_truthy_sets_flag() {
    let mut env = HashMap::new();
    env.insert("CI".into(), "true".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# flaky if CI
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_flaky(&suite.plans[0]));
}

#[test]
fn marker_flaky_if_falsy_not_flaky() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# flaky if CI
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert!(!is_flaky(&suite.plans[0]));
}

#[test]
fn marker_flaky_unless_empty_is_flaky() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# flaky unless UNSET
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_flaky(&suite.plans[0]));
}

#[test]
fn marker_flaky_unless_truthy_not_flaky() {
    let mut env = HashMap::new();
    env.insert("STABLE".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# flaky unless STABLE
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
    assert!(!is_flaky(&suite.plans[0]));
}

#[test]
fn marker_flaky_with_skip_skip_wins() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# flaky
# skip
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_no_flaky_by_default() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
    assert!(!is_flaky(&suite.plans[0]));
}

// ─── Marker on fn/effect ───────────────────────────────────

#[test]
fn marker_skip_on_fn_propagates_to_test() {
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
    // fn is skipped → calling it from test body propagates skip
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_skip_on_effect_propagates_to_test() {
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
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_run_met_on_fn_allows_test() {
    let mut env = HashMap::new();
    env.insert("CI".into(), "true".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# run if CI
fn helper() {
  > echo hello
}

test "t" {
  shell sh {
    helper()
  }
}
"#,
        )],
        env,
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn marker_run_unmet_on_fn_propagates_skip_to_test() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# run if CI
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
    // CI not set → run condition unmet → fn skipped → test skipped
    assert!(is_skipped(&suite.plans[0]));
}
