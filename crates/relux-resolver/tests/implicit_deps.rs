// Integration tests for RFC R015 (Implicit Effect Dependencies).
// Task B4: wiring `enrich_start_dag` into effect lowering, and validating
// every `${Alias.var}` in an effect's shell bodies against its dependency
// map.
// Task B5: the symmetric wiring for test-level start-lists, plus (since a
// started effect's exposed vars are injected into the test's own
// shell/cleanup scope at runtime, same as an effect's own body) the same
// shell-body qualified-ref validation for a test's `shell { }` / `cleanup
// { }` blocks.
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

fn app_effect_id() -> EffectId {
    EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    }
}

#[test]
fn overlay_qualified_ref_in_effect_records_dep() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
effect Api {
  expect DB_PORT
  shell api {
    > start
  }
}
effect App {
  start Db as Db
  start Api { DB_PORT := Db.port }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = ctx.resolve_effect(&app_effect_id());
    assert!(
        result.is_ok(),
        "expected effect to lower cleanly, got {result:?}"
    );
    let eff = result.unwrap();
    assert_eq!(eff.starts().len(), 2);
    // Api (index 1) depends on Db (index 0) via its `DB_PORT := Db.port` overlay.
    assert_eq!(eff.starts()[1].deps(), &[0]);
    assert!(eff.starts()[0].deps().is_empty());
}

#[test]
fn effect_overlay_unknown_qualifier_is_error() {
    let source = r#"effect Api {
  expect X
  shell api {
    > start
  }
}
effect App {
  start Api { X := Nope.port }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = ctx.resolve_effect(&app_effect_id()).unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ),
        "expected UnknownQualifier, got {err:?}"
    );
}

#[test]
fn effect_overlay_non_exposed_var_is_error() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect Api {
  expect X
  shell api {
    > start
  }
}
effect App {
  start Db as Db
  start Api { X := Db.port }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = ctx.resolve_effect(&app_effect_id()).unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::VariableNotExposed { .. })
        ),
        "expected VariableNotExposed (Db does not expose port), got {err:?}"
    );
}

#[test]
fn shell_body_unknown_qualifier_is_error() {
    // The breaking tightening: `${Typo.port}` in an effect shell body,
    // where `Typo` is not a dependency of the effect at all, used to
    // silently resolve to "" and now must be a compile error.
    let source = r#"effect App {
  shell app {
    > connect ${Typo.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = ctx.resolve_effect(&app_effect_id()).unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ),
        "expected UnknownQualifier, got {err:?}"
    );
}

#[test]
fn shell_body_valid_qualified_ref_lowers() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
effect App {
  start Db as Db
  shell app {
    > connect ${Db.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = ctx.resolve_effect(&app_effect_id());
    assert!(
        result.is_ok(),
        "expected shell body to accept a valid dependency ref, got {result:?}"
    );
}

#[test]
fn cleanup_body_unknown_qualifier_is_error() {
    // Same breaking tightening as shell bodies, but for an effect's own
    // `cleanup { }` block: the runtime shares the same VarScope between a
    // shell block and the cleanup block, so a `${Typo.var}` naming a
    // non-dependency alias must be rejected at compile time here too.
    let source = r#"effect App {
  shell app {
    > start
  }
  cleanup {
    > disconnect ${Typo.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = ctx.resolve_effect(&app_effect_id()).unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ),
        "expected UnknownQualifier, got {err:?}"
    );
}

#[test]
fn cleanup_body_valid_qualified_ref_lowers() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
effect App {
  start Db as Db
  shell app {
    > start
  }
  cleanup {
    > disconnect ${Db.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = ctx.resolve_effect(&app_effect_id());
    assert!(
        result.is_ok(),
        "expected cleanup body to accept a valid dependency ref, got {result:?}"
    );
}

// --- Test-level start-list wiring (Task B5) ---------------

#[test]
fn test_overlay_qualified_ref_records_dep() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
effect Api {
  expect DB_PORT
  shell api {
    > start
  }
}
test "app" {
  start Db as Db
  start Api { DB_PORT := Db.port }
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(
        result.is_ok(),
        "expected test to lower cleanly, got {result:?}"
    );
    let test = result.unwrap();
    assert_eq!(test.starts().len(), 2);
    // Api (index 1) depends on Db (index 0) via its `DB_PORT := Db.port` overlay.
    assert_eq!(test.starts()[1].deps(), &[0]);
    assert!(test.starts()[0].deps().is_empty());
}

#[test]
fn test_overlay_forward_reference_is_legal() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
effect Api {
  expect DB_PORT
  shell api {
    > start
  }
}
test "app" {
  start Api { DB_PORT := Db.port }
  start Db as Db
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let test = lower_first_test(&mut ctx, "tests/a").unwrap();
    assert_eq!(test.starts()[0].deps(), &[1]);
    assert!(test.starts()[1].deps().is_empty());
}

#[test]
fn test_overlay_unknown_qualifier_is_error() {
    let source = r#"effect Api {
  expect X
  shell api {
    > start
  }
}
test "app" {
  start Api { X := Nope.port }
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = lower_first_test(&mut ctx, "tests/a").unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ),
        "expected UnknownQualifier, got {err:?}"
    );
}

#[test]
fn test_overlay_non_exposed_var_is_error() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect Api {
  expect X
  shell api {
    > start
  }
}
test "app" {
  start Db as Db
  start Api { X := Db.port }
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = lower_first_test(&mut ctx, "tests/a").unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::VariableNotExposed { .. })
        ),
        "expected VariableNotExposed (Db does not expose port), got {err:?}"
    );
}

#[test]
fn test_overlay_sibling_cycle_is_error() {
    let source = r#"effect A {
  expect X
  let out := "a"
  expose var out
  shell a {
    > start
  }
}
effect B {
  expect Y
  let out := "b"
  expose var out
  shell b {
    > start
  }
}
test "app" {
  start A as A { X := B.out }
  start B as B { Y := A.out }
  shell sh {
    > cmd
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = lower_first_test(&mut ctx, "tests/a").unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::Cycle(CycleReport::Effect { .. }))
        ),
        "expected a sibling cycle error, got {err:?}"
    );
}

// --- Test-level shell-body qualified refs (Task B5, Step 4) -----------
//
// A started effect's exposed vars are injected into the *test's own*
// shell/cleanup scope at runtime (see `run_test_body` in
// `relux-runtime/src/lib.rs`), mirroring the injection into an effect's
// own scope that the effect-level `shell_body_*` tests above guard. So
// the same unknown-qualifier / non-exposed-var footgun applies to a
// test's `shell { }` / `cleanup { }` bodies, and gets the same
// validation.

#[test]
fn test_shell_body_unknown_qualifier_is_error() {
    let source = r#"test "app" {
  shell sh {
    > connect ${Typo.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let err = lower_first_test(&mut ctx, "tests/a").unwrap_err();
    assert!(
        matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ),
        "expected UnknownQualifier, got {err:?}"
    );
}

#[test]
fn test_shell_body_valid_qualified_ref_lowers() {
    let source = r#"effect Db {
  let port := available_port()
  expose var port
  shell db {
    > start
  }
}
test "app" {
  start Db as Db
  shell sh {
    > connect ${Db.port}
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(
        result.is_ok(),
        "expected shell body to accept a valid dependency ref, got {result:?}"
    );
}
