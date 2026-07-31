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

// --- eval_marker: @skip ----------------------------------

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
fn marker_skip_if_eq_rhs_empty_is_contains_match() {
    // Contains semantics: every string contains the empty string, so an
    // unset RHS now makes `=` met (this is a deliberate consequence of the
    // contains rule, not exact-equality's "both empty or neither" behavior).
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
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn marker_skip_if_eq_is_contains_not_exact() {
    // `X = linux` with X = "ubuntu-linux" must now be met (contains),
    // where the old exact-equality semantics would have been unmet.
    let mut env = HashMap::new();
    env.insert("MY_VAR".into(), "ubuntu-linux".into());
    env.insert("SUBSTR".into(), "linux".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# skip if MY_VAR = SUBSTR
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
        )],
        env,
    );
    assert!(
        is_skipped(&suite.plans[0]),
        "expected `=` to match by containment"
    );
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

// --- eval_marker: @run -----------------------------------

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

// --- eval_marker: unless ---------------------------------

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

// --- eval_marker: with expressions -----------------------

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

// --- eval_marker: errors ---------------------------------

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

#[test]
fn later_marker_lowering_error_is_invalid_despite_earlier_skip() {
    // Marker lowering validity is env-INDEPENDENT: every marker is lowered to IR
    // before any is decided, so a later marker's lowering error (here an
    // undefined pure fn) surfaces as `Plan::Invalid` even though the earlier
    // unconditional `# skip` would otherwise short-circuit the decision. This
    // pins the behavior introduced by the lower/decide split.
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
# skip if nonexistent()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

// --- eval_marker: multiple markers -----------------------

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

// --- Marker on fn/effect ---------------------------------

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
    // fn is skipped -> calling it from test body propagates skip
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
    // CI not set -> run condition unmet -> fn skipped -> test skipped
    assert!(is_skipped(&suite.plans[0]));
}

// --- Plan::Skipped retains marker recordings -------------

#[test]
fn skipped_plan_retains_unconditional_skip_recording() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
test "skipped" {
  shell sh {
    > echo hi
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
    let stack = StackHash(suite.env.stack_hash());
    let recs = &suite
        .tables
        .marker_decisions
        .get(&(suite.plans[0].meta().definition().clone(), stack))
        .expect("decision present for skipped test")
        .recordings;
    assert_eq!(recs.len(), 1, "expected one recording for `# skip`");
    assert_eq!(recs[0].kind, MarkerEvalKind::Skip);
    assert_eq!(recs[0].decision, MarkerEvalDecision::Mark);
}

#[test]
fn skipped_plan_retains_run_if_falsy_recording() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# run if MY_VAR
test "skipped" {
  shell sh {
    > echo hi
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
    let stack = StackHash(suite.env.stack_hash());
    let recs = &suite
        .tables
        .marker_decisions
        .get(&(suite.plans[0].meta().definition().clone(), stack))
        .expect("decision present for skipped test")
        .recordings;
    assert_eq!(
        recs.len(),
        1,
        "expected one recording for `# run if MY_VAR`"
    );
    assert_eq!(recs[0].kind, MarkerEvalKind::Run);
    // `# run if MY_VAR` with MY_VAR unset -> condition not met -> Run-marker
    // does NOT apply (decision = Pass) -> the test is skipped because no
    // run-mark fired.
    assert_eq!(recs[0].decision, MarkerEvalDecision::Pass);
}

#[test]
fn skipped_plan_retains_full_marker_chain() {
    let mut env = HashMap::new();
    env.insert("TRIGGER".into(), "yes".into());
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"# flaky
# skip if TRIGGER
test "skipped" {
  shell sh {
    > echo hi
  }
}
"#,
        )],
        env,
    );
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
    let stack = StackHash(suite.env.stack_hash());
    let recs = &suite
        .tables
        .marker_decisions
        .get(&(suite.plans[0].meta().definition().clone(), stack))
        .expect("decision present for skipped test")
        .recordings;
    assert_eq!(recs.len(), 2, "expected two recordings (flaky + skip-if)");
    // Order matches source order: flaky first, skip-if second (the triggering one).
    assert_eq!(recs[0].kind, MarkerEvalKind::Flaky);
    assert_eq!(recs[1].kind, MarkerEvalKind::Skip);
    assert_eq!(recs[1].decision, MarkerEvalDecision::Mark);
}

#[test]
fn propagated_skip_from_fn_lands_under_originating_def() {
    // CI not set -> `# run if CI` on helper() unmet -> helper bails skip ->
    // test depending on helper inherits the skip. The originating recordings
    // live under DefinitionRef::Fn(helper), not under the test's own def.
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
    assert!(is_skipped(&suite.plans[0]));

    let stack = StackHash(suite.env.stack_hash());

    // Test's own definition has no markers (no `#` lines), so its entry is
    // either absent or an empty vec - the propagating recording lives under
    // the fn's definition, not the test's.
    let test_decision = suite
        .tables
        .marker_decisions
        .get(&(suite.plans[0].meta().definition().clone(), stack));
    assert!(
        test_decision
            .map(|d| d.recordings.is_empty())
            .unwrap_or(true),
        "test definition should have no recordings on propagated skip, got: {test_decision:?}",
    );

    // Find the fn's definition in the decision table.
    let fn_def = relux_core::diagnostics::DefinitionRef::Fn(relux_core::diagnostics::FnId {
        module: relux_core::diagnostics::ModulePath("tests/a".into()),
        name: "helper".into(),
        arity: 0,
    });
    let recs = &suite
        .tables
        .marker_decisions
        .get(&(fn_def, stack))
        .expect("originating fn decision present")
        .recordings;
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].kind, MarkerEvalKind::Run);
    assert_eq!(recs[0].decision, MarkerEvalDecision::Pass);
}

// --- M4b: skip decoupled from IR production ---------------

#[test]
fn skip_gated_shared_fn_still_resolves_to_ok_ir() {
    // A `# skip`-marked shared `fn` reached by a test must still lower to
    // `Ok(IR)` in `tables.fns` - the skip lives in the decision table now,
    // not baked into the fn's cached lowering result. The *test* is still
    // `Plan::Skipped` (the skip propagates via the reachability walk), but
    // the fn's IR itself is no longer poisoned by the marker.
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
    assert!(is_skipped(&suite.plans[0]));

    let fn_id = relux_core::diagnostics::FnId {
        module: relux_core::diagnostics::ModulePath("tests/a".into()),
        name: "helper".into(),
        arity: 0,
    };
    let result = suite
        .tables
        .fns
        .get(&fn_id)
        .expect("helper must have been resolved");
    assert!(
        result.is_ok(),
        "skip-gated fn must still resolve to Ok(IR), got: {result:?}"
    );
}

/// Resolve the `DefinitionRef` a skipped plan's skip-cause points at.
fn skipped_plan_skip_definition(suite: &Suite, plan: &Plan) -> DefinitionRef {
    let causes = match plan {
        Plan::Skipped { causes, .. } => causes,
        other => panic!("expected Plan::Skipped, got {other:?}"),
    };
    causes
        .iter()
        .find_map(|id| match suite.causes.get(id) {
            Some(Cause::Skip(report)) => Some(report.definition.clone()),
            _ => None,
        })
        .expect("skipped plan must carry a Cause::Skip")
}

#[test]
fn aggregation_first_skip_wins_across_two_reachable_defs() {
    // A test reaches TWO skip-triggering defs: it `start`s a `# skip`-marked
    // effect and calls a `# skip`-marked fn. `collect_test_decision` visits
    // in pre-order (test def, then `start`ed effects, then body fn calls), so
    // the effect is seen before the body fn. The resulting Plan::Skipped's
    // cause must therefore point at the PRE-ORDER-FIRST def: the effect.
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
effect Setup {
  shell sh {
    > echo setup
  }
}

# skip
fn helper() {
  > echo hello
}

test "t" {
  start Setup
  shell sh {
    helper()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));

    let def = skipped_plan_skip_definition(&suite, &suite.plans[0]);
    let expected = DefinitionRef::Effect(EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Setup".into()),
    });
    assert_eq!(
        def, expected,
        "pre-order-first skip (the effect) must win over the body fn's skip"
    );
}

#[test]
fn aggregation_flaky_or_across_defs_marks_unmarked_test_flaky() {
    // The test itself carries no marker, but it reaches a `# flaky`-marked fn.
    // Flaky is OR'd across every reachable def, so the test is flaky even
    // though it has no marker of its own.
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# flaky
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
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
    assert!(
        is_flaky(&suite.plans[0]),
        "a test reaching a flaky-marked fn must itself be flaky"
    );
}
