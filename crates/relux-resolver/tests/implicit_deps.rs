// Integration tests for RFC R015 (Implicit Effect Dependencies), Task B4:
// wiring `enrich_start_dag` into effect lowering, and validating every
// `${Alias.var}` in an effect's shell bodies against its dependency map.
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
