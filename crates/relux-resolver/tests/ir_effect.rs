// Tests extracted from relux-ir/src/effect.rs
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

fn test_effect_id() -> EffectId {
    EffectId {
        module: ModulePath("test".into()),
        name: EffectName("Db".into()),
    }
}

#[test]
fn ir_effect_with_starts() {
    let s = test_span();
    let start = IrEffectStart::new(test_effect_id(), vec![], None, s.clone());
    let eff = IrEffect::new(test_ident("Db"), vec![], vec![], vec![start], vec![], s);
    assert_eq!(eff.starts().len(), 1);
}

#[test]
fn ir_effect_empty_starts() {
    let eff = IrEffect::new(
        test_ident("Standalone"),
        vec![],
        vec![],
        vec![],
        vec![],
        test_span(),
    );
    assert!(eff.starts().is_empty());
}

#[test]
fn ir_effect_start_no_overlay() {
    let start = IrEffectStart::new(test_effect_id(), vec![], None, test_span());
    assert!(start.overlay().is_empty());
}

#[test]
fn ir_effect_start_with_alias() {
    let start = IrEffectStart::new(test_effect_id(), vec![], Some("my_db".into()), test_span());
    assert_eq!(start.alias(), Some("my_db"));
}

#[test]
fn ir_effect_start_without_alias() {
    let start = IrEffectStart::new(test_effect_id(), vec![], None, test_span());
    assert_eq!(start.alias(), None);
}

#[test]
fn ir_overlay_entry() {
    let s = test_span();
    let val = IrPureExpr::Var {
        name: "port_var".into(),
        span: s.clone(),
    };
    let entry = IrOverlayEntry::new(test_ident("PORT"), val, s);
    assert_eq!(entry.key().name(), "PORT");
    assert!(matches!(entry.value(), IrPureExpr::Var { .. }));
}

// ─── Effect lowering (cacheable) ──────────────────────────

#[test]
fn lower_effect_simple() {
    let source = r#"effect Db {
  shell db {
    > start_db
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    assert_eq!(eff.name().name(), "Db");
}

#[test]
fn lower_effect_with_start() {
    let source = r#"effect Base {
  shell base {
    > base
  }
}
effect App {
  start Base
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    assert!(!result.unwrap().starts().is_empty());
}

#[test]
fn lower_effect_recursive_start() {
    let source = r#"effect A {
  shell a {
    > a
  }
}
effect B {
  start A
  shell b {
    > b
  }
}
effect C {
  start B
  shell c {
    > c
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("C".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let a_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let b_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("B".into()),
    };
    assert!(ctx.effects().get(&a_id).is_some());
    assert!(ctx.effects().get(&b_id).is_some());
}

#[test]
fn lower_effect_cycle_mutual() {
    let source = r#"effect A {
  start B
  shell a {
    > a
  }
}
effect B {
  start A
  shell b {
    > b
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_cycle_self() {
    let source = r#"effect A {
  start A
  shell a {
    > a
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_cycle_deep() {
    let source = r#"effect A {
  start B
  shell a {
    > a
  }
}
effect B {
  start C
  shell b {
    > b
  }
}
effect C {
  start A
  shell c {
    > c
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_err());
    if let Err(LoweringBail::Invalid(inner)) = &result {
        if let InvalidReport::Cycle(CycleReport::Effect { chain }) = inner.as_ref() {
            assert_eq!(chain.len(), 3);
        } else {
            panic!("expected effect cycle, got {:?}", result);
        }
    } else {
        panic!("expected effect cycle, got {:?}", result);
    }
}

#[test]
fn lower_effect_memoized() {
    let source = r#"effect Shared {
  shell sh {
    > s
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Shared".into()),
    };
    ctx.resolve_effect(&effect_id).unwrap();
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
}

#[test]
fn lower_effect_with_cleanup() {
    let source = r#"effect Db {
  shell db {
    > start
  }
  cleanup {
    > stop
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id).unwrap();
    assert!(
        result
            .body()
            .iter()
            .any(|item| matches!(item, IrEffectItem::Cleanup { .. }))
    );
}

#[test]
fn lower_effect_undefined_start() {
    let source = r#"effect A {
  start Nonexistent
  shell a {
    > a
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_with_overlay() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = "5432" }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id).unwrap();
    let start = &result.starts()[0];
    assert!(!start.overlay().is_empty());
}

#[test]
fn lower_effect_with_let_vars() {
    let source = r#"effect Db {
  let port = "5432"
  shell db {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id).unwrap();
    assert!(
        result
            .body()
            .iter()
            .any(|item| matches!(item, IrEffectItem::Let { .. }))
    );
}

#[test]
fn lower_effect_with_fn_calls() {
    let source = r#"fn setup() {
  > setup
}
effect Db {
  shell db {
    setup()
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let setup_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "setup".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&setup_id).is_some());
}

#[test]
fn lower_effect_start_with_alias() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db as MyDb
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id).unwrap();
    let start = &result.starts()[0];
    assert_eq!(start.alias(), Some("MyDb"));
}

#[test]
fn lower_effect_start_without_alias() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id).unwrap();
    let start = &result.starts()[0];
    assert!(start.alias().is_none());
}

#[test]
fn lower_effect_error_cached() {
    let source = r#"effect A {
  start Nonexistent
  shell a {
    > a
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("A".into()),
    };
    let result1 = ctx.resolve_effect(&effect_id);
    assert!(result1.is_err());
    let result2 = ctx.resolve_effect(&effect_id);
    assert!(result2.is_err());
}

// ─── Purity enforcement tests ────────────────────────────

#[test]
fn lower_effect_let_rejects_impure_fn_call() {
    let source = r#"fn impure_fn() {
  > cmd
}
effect E {
  let x = impure_fn()
  shell sh {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("E".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_let_accepts_pure_fn_call() {
    let source = r#"effect E {
  let x = trim("hi")
  shell sh {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("E".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    assert!(
        eff.body()
            .iter()
            .any(|item| matches!(item, IrEffectItem::Let { .. }))
    );
}

#[test]
fn lower_effect_let_accepts_string_literal() {
    let source = r#"effect E {
  let x = "hello"
  shell sh {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("E".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
}

#[test]
fn lower_effect_let_accepts_var_ref() {
    let source = r#"effect E {
  let x = "val"
  let y = x
  shell sh {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("E".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
}

#[test]
fn lower_overlay_accepts_pure_fn_call() {
    let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = available_port() }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    let start = &eff.starts()[0];
    assert!(!start.overlay().is_empty());
}

#[test]
fn lower_overlay_rejects_impure_fn_call() {
    let source = r#"fn impure_fn() {
  > cmd
}
effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = impure_fn() }
  shell app {
    > app
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("App".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

// ─── Expose validation ──────────────────────────────────

#[test]
fn lower_effect_expose_valid_local_shell() {
    let source = r#"effect Db {
  expose shell db
  shell db {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().exposes().len(), 1);
}

#[test]
fn lower_effect_expose_invalid_shell() {
    let source = r#"effect Db {
  expose shell nonexistent
  shell db {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_expose_invalid_var() {
    let source = r#"effect Db {
  expose var nonexistent
  shell db {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_expose_qualified_valid() {
    let source = r#"effect Base {
  expose shell sh
  shell sh {
    > base
  }
}
effect Wrapper {
  start Base as B
  expose shell B.sh as base_shell
  shell wrapper {
    > wrapper
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Wrapper".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    assert_eq!(eff.exposes().len(), 1);
    assert_eq!(eff.exposes()[0].exposed_name(), "base_shell");
}

#[test]
fn lower_effect_expose_qualified_invalid_alias() {
    let source = r#"effect Base {
  expose shell sh
  shell sh {
    > base
  }
}
effect Wrapper {
  start Base as B
  expose shell Nonexistent.sh
  shell wrapper {
    > wrapper
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Wrapper".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(matches!(result, Err(LoweringBail::Invalid(_))));
}

#[test]
fn lower_effect_expose_qualified_unexposed_shell() {
    // Base exposes `sh` but NOT `internal`.
    // Wrapper tries to re-expose `b.internal` — this should fail
    // because Base does not expose `internal` to callers.
    let source = r#"effect Base {
  expose shell sh
  shell sh {
    > base
  }
  shell internal {
    > secret
  }
}
effect Wrapper {
  start Base as B
  expose shell B.internal as leaked
  shell wrapper {
    > wrapper
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Wrapper".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(
        matches!(result, Err(LoweringBail::Invalid(_))),
        "expose should reject referencing a shell not exposed by the dependency"
    );
}

#[test]
fn lower_effect_expose_rejects_qualified_shell_name() {
    // `shell b.sh { ... }` is a qualified block that operates on a dependency's
    // shell — it does NOT create a local shell. `expose sh` (unqualified) should
    // fail because no local shell named `sh` exists.
    let source = r#"effect Base {
  expose shell sh
  shell sh {
    > base
  }
}
effect Wrapper {
  start Base as B
  expose shell sh
  shell B.sh {
    > use dep shell
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Wrapper".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(
        matches!(result, Err(LoweringBail::Invalid(_))),
        "expose should reject a qualified shell block's name as a local shell"
    );
}

#[test]
fn lower_effect_expect_vars() {
    let source = r#"effect Db {
  expect DB_PORT, DB_NAME
  expose shell db
  shell db {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Db".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    assert_eq!(eff.expects().len(), 2);
    assert_eq!(eff.expects()[0].name(), "DB_PORT");
    assert_eq!(eff.expects()[1].name(), "DB_NAME");
}

#[test]
fn lower_effect_no_expose_is_valid() {
    let source = r#"effect SideEffect {
  shell setup {
    > side effect
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("SideEffect".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    assert!(result.unwrap().exposes().is_empty());
}

#[test]
fn lower_effect_expose_local_with_alias() {
    let source = r#"effect Auth {
  expose shell auth as svc
  shell auth {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Auth".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    let eff = result.unwrap();
    assert_eq!(eff.exposes().len(), 1);
    assert_eq!(eff.exposes()[0].exposed_name(), "svc");
}

#[test]
fn lower_effect_no_expect_is_valid() {
    let source = r#"effect Simple {
  expose shell sh
  shell sh {
    > start
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("Simple".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_ok());
    assert!(result.unwrap().expects().is_empty());
}
