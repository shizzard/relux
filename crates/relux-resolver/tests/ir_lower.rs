// Tests extracted from src/dsl/resolver/lower.rs mod tests
#![allow(unused_imports)]
use relux_ast::*;
use relux_core::Span;
use relux_core::Spanned;
use relux_core::diagnostics::*;
use relux_core::pure::*;
use relux_core::table::FileId;
use relux_core::table::SharedTable;
use relux_core::table::SourceFile;
use relux_core::table::SourceTable;
use relux_ir::lowering_context::*;
use relux_ir::*;
use relux_resolver::lower::test_helpers::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use relux_resolver::lower::test_helpers::*;

use relux_core::diagnostics::Cause;
use relux_core::diagnostics::CauseId;
use relux_core::diagnostics::CauseTable;
use relux_core::diagnostics::DefinitionRef;
use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::EffectName;
use relux_core::diagnostics::FnId;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::ModulePath;
use relux_core::diagnostics::SkipEvaluation;
use relux_core::diagnostics::SkipReport;
use relux_core::diagnostics::WarningTable;
use relux_ir::*;

use relux_ast::*;

// ─── Local Test Helpers (not shared) ─────────────────────

fn test_span_at(start: usize, end: usize) -> IrSpan {
    IrSpan::new(test_file_id(), Span::new(start, end))
}

fn builtin_mod() -> ModulePath {
    ModulePath("@builtin".into())
}

fn make_context() -> LoweringContext {
    LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        test_env(),
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    )
}

fn make_context_with_bifs() -> LoweringContext {
    let ctx = make_context();
    ctx.register_bifs();
    ctx
}

/// Build an AstTable with the given modules.
fn make_ast_table(modules: Vec<(ModulePath, FileId, AstModule)>) -> AstTable {
    let table: AstTable = SharedTable::new();
    for (path, file_id, module) in modules {
        table.insert(path, (file_id, module));
    }
    table
}

fn make_context_with_ast(modules: Vec<(ModulePath, FileId, AstModule)>) -> LoweringContext {
    let ast_table = make_ast_table(modules);
    let ctx = LoweringContext::new(
        ast_table,
        empty_source_table(),
        test_env(),
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    );
    ctx.register_bifs();
    ctx
}

/// Create a minimal AstModule with the given items.
fn make_module(items: Vec<AstItem>) -> AstModule {
    let spanned_items = items
        .into_iter()
        .map(|item| {
            let span = *item.span();
            crate::Spanned::new(item, span)
        })
        .collect();
    AstModule {
        items: spanned_items,
        span: Span::new(0, 100),
    }
}

fn make_fn_def(name: &str, param_count: usize) -> AstItem {
    let params: Vec<Spanned<AstIdent>> = (0..param_count)
        .map(|i| {
            let ident = AstIdent::new(format!("p{i}"), Span::new(0, 1));
            Spanned::new(ident.clone(), ident.span)
        })
        .collect();
    let def = AstFnDef {
        name: Spanned::new(
            AstIdent::new(name, Span::new(0, name.len())),
            Span::new(0, name.len()),
        ),
        params,
        markers: vec![],
        body: vec![],
        span: Span::new(0, 50),
    };
    AstItem::Fn {
        def,
        span: Span::new(0, 50),
    }
}

fn make_pure_fn_def(name: &str, param_count: usize) -> AstItem {
    let params: Vec<Spanned<AstIdent>> = (0..param_count)
        .map(|i| {
            let ident = AstIdent::new(format!("p{i}"), Span::new(0, 1));
            Spanned::new(ident.clone(), ident.span)
        })
        .collect();
    let def = AstPureFnDef {
        name: Spanned::new(
            AstIdent::new(name, Span::new(0, name.len())),
            Span::new(0, name.len()),
        ),
        params,
        markers: vec![],
        body: vec![],
        span: Span::new(0, 50),
    };
    AstItem::PureFn {
        def,
        span: Span::new(0, 50),
    }
}

fn make_effect_def(name: &str) -> AstItem {
    let def = AstEffectDef {
        name: Spanned::new(
            AstIdent::new(name, Span::new(0, name.len())),
            Span::new(0, name.len()),
        ),
        markers: vec![],
        body: vec![],
        span: Span::new(0, 50),
    };
    AstItem::Effect {
        def,
        span: Span::new(0, 50),
    }
}

fn make_import(path: &str, names: Option<Vec<(&str, Option<&str>)>>) -> AstItem {
    let import_names = names.map(|ns| {
        ns.into_iter()
            .map(|(name, alias)| {
                let import_name = AstImportName {
                    name: Spanned::new(
                        AstIdent::new(name, Span::new(0, name.len())),
                        Span::new(0, name.len()),
                    ),
                    alias: alias.map(|a| {
                        Spanned::new(
                            AstIdent::new(a, Span::new(0, a.len())),
                            Span::new(0, a.len()),
                        )
                    }),
                    span: Span::new(0, 20),
                };
                Spanned::new(import_name, Span::new(0, 20))
            })
            .collect()
    });
    let import = AstImport {
        path: Spanned::new(path.into(), Span::new(0, path.len())),
        names: import_names,
        span: Span::new(0, 30),
    };
    AstItem::Import {
        import,
        span: Span::new(0, 30),
    }
}

// ═══════════════════════════════════════════════════════════
// LoweringContext construction
// ═══════════════════════════════════════════════════════════

#[test]
fn context_new_has_empty_ir_registries() {
    let ctx = make_context();
    let fn_key = FnId {
        module: ModulePath("m".into()),
        name: "f".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&fn_key).is_none());
    assert!(ctx.pure_functions().get(&fn_key).is_none());
    let eff_key = EffectId {
        module: ModulePath("m".into()),
        name: EffectName("E".into()),
    };
    assert!(ctx.effects().get(&eff_key).is_none());
}

#[test]
fn context_new_has_empty_stacks() {
    let ctx = make_context();
    assert!(ctx.fn_stack().is_empty());
    assert!(ctx.effect_stack().is_empty());
}

#[test]
fn context_new_preserves_env() {
    let mut m = HashMap::new();
    m.insert("KEY".into(), "val".into());
    let env = Arc::new(LayeredEnv::from(Env::from_map(m)));
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        env.clone(),
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    );
    assert_eq!(ctx.env().get("KEY"), Some("val"));
}

#[test]
fn context_new_preserves_ast_table() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![]);
    let ast_table = make_ast_table(vec![(mod_path.clone(), file_id, module)]);
    let ctx = LoweringContext::new(
        ast_table,
        empty_source_table(),
        test_env(),
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    );
    assert!(ctx.ast_table().get(&mod_path).is_some());
}

#[test]
fn context_new_preserves_cause_table() {
    let causes: CauseTable = SharedTable::new();
    let id = CauseId::generate("m", "f", 0, "err");
    causes.insert(
        id.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        test_env(),
        causes,
        WarningTable::default(),
        1.0,
    );
    assert!(ctx.causes().get(&id).is_some());
}

#[test]
fn context_new_preserves_multiplier() {
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        test_env(),
        CauseTable::default(),
        WarningTable::default(),
        2.5,
    );
    assert_eq!(ctx.multiplier(), 2.5);
}

#[test]
fn context_new_default_multiplier() {
    let ctx = make_context();
    assert_eq!(ctx.multiplier(), 1.0);
}

#[test]
fn context_new_preserves_warning_table() {
    let warnings: WarningTable = SharedTable::new();
    // No warning variants yet, but table itself should be preserved.
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        test_env(),
        CauseTable::default(),
        warnings,
        1.0,
    );
    let _ = ctx.warnings();
}

// ═══════════════════════════════════════════════════════════
// BIF registration
// ═══════════════════════════════════════════════════════════

#[test]
fn bifs_registered_in_fn_table() {
    let ctx = make_context_with_bifs();
    // All 24 BIFs should be in fn_table (14 pure + 10 impure).
    let all_bifs: Vec<(&str, usize)> = vec![
        ("sleep", 1),
        ("annotate", 1),
        ("log", 1),
        ("trim", 1),
        ("upper", 1),
        ("lower", 1),
        ("replace", 3),
        ("split", 3),
        ("len", 1),
        ("uuid", 0),
        ("rand", 1),
        ("rand", 2),
        ("available_port", 0),
        ("which", 1),
        ("match_prompt", 0),
        ("match_exit_code", 1),
        ("match_ok", 0),
        ("match_not_ok", 0),
        ("match_not_ok", 1),
        ("ctrl_c", 0),
        ("ctrl_d", 0),
        ("ctrl_z", 0),
        ("ctrl_l", 0),
        ("ctrl_backslash", 0),
    ];
    for (name, arity) in all_bifs {
        let key = FnId {
            module: builtin_mod(),
            name: name.into(),
            arity,
        };
        assert!(
            ctx.functions().get(&key).is_some(),
            "BIF {name}/{arity} not in fn_table"
        );
    }
}

#[test]
fn pure_bifs_registered_in_pure_fn_table() {
    let ctx = make_context_with_bifs();
    let pure_bifs: Vec<(&str, usize)> = vec![
        ("trim", 1),
        ("upper", 1),
        ("lower", 1),
        ("replace", 3),
        ("split", 3),
        ("len", 1),
        ("uuid", 0),
        ("rand", 1),
        ("rand", 2),
        ("available_port", 0),
        ("which", 1),
    ];
    for (name, arity) in pure_bifs {
        let key = FnId {
            module: builtin_mod(),
            name: name.into(),
            arity,
        };
        assert!(
            ctx.pure_functions().get(&key).is_some(),
            "Pure BIF {name}/{arity} not in pure_fn_table"
        );
    }
}

#[test]
fn impure_bifs_not_in_pure_fn_table() {
    let ctx = make_context_with_bifs();
    let impure_bifs: Vec<(&str, usize)> = vec![
        ("sleep", 1),
        ("annotate", 1),
        ("log", 1),
        ("match_prompt", 0),
        ("match_exit_code", 1),
        ("match_ok", 0),
        ("match_not_ok", 0),
        ("match_not_ok", 1),
        ("ctrl_c", 0),
        ("ctrl_d", 0),
        ("ctrl_z", 0),
        ("ctrl_l", 0),
        ("ctrl_backslash", 0),
    ];
    for (name, arity) in impure_bifs {
        let key = FnId {
            module: builtin_mod(),
            name: name.into(),
            arity,
        };
        assert!(
            ctx.pure_functions().get(&key).is_none(),
            "Impure BIF {name}/{arity} should NOT be in pure_fn_table"
        );
    }
}

#[test]
fn impure_bifs_in_fn_table() {
    let ctx = make_context_with_bifs();
    let impure_bifs: Vec<(&str, usize)> = vec![("match_prompt", 0), ("ctrl_c", 0), ("ctrl_d", 0)];
    for (name, arity) in impure_bifs {
        let key = FnId {
            module: builtin_mod(),
            name: name.into(),
            arity,
        };
        assert!(ctx.functions().get(&key).is_some());
    }
}

#[test]
fn bif_module_path_is_builtin() {
    let ctx = make_context_with_bifs();
    let key = FnId {
        module: builtin_mod(),
        name: "trim".into(),
        arity: 1,
    };
    assert!(ctx.functions().get(&key).is_some());
}

#[test]
fn bif_entries_are_ok_builtin() {
    let ctx = make_context_with_bifs();
    let key = FnId {
        module: builtin_mod(),
        name: "uuid".into(),
        arity: 0,
    };
    let entry = ctx.functions().get(&key).unwrap();
    assert!(matches!(entry, Ok(IrFn::Builtin { arity: 0, .. })));
}

#[test]
fn bif_arity_matches_definition() {
    let ctx = make_context_with_bifs();
    let key = FnId {
        module: builtin_mod(),
        name: "replace".into(),
        arity: 3,
    };
    let entry = ctx.functions().get(&key).unwrap();
    if let Ok(IrFn::Builtin { arity, .. }) = entry {
        assert_eq!(*arity, 3);
    } else {
        panic!("expected Ok(Builtin)");
    }
}

#[test]
fn bif_no_user_module_collides_with_builtin() {
    // @builtin cannot be a filesystem path (paths are like lib/... or tests/...).
    assert!(builtin_mod().0.starts_with('@'));
}

// ═══════════════════════════════════════════════════════════
// Local table factory
// ═══════════════════════════════════════════════════════════

#[test]
fn local_fn_table_sees_registered_bifs() {
    let ctx = make_context_with_bifs();
    let mut tables = ctx.local_tables();
    let local_key = LocalFnKey::new("trim", 1);
    let global_key = FnId {
        module: builtin_mod(),
        name: "trim".into(),
        arity: 1,
    };
    tables
        .fns
        .insert(local_key.clone(), global_key, IrSpan::synthetic());
    assert!(tables.fns.get(&local_key).is_some());
}

#[test]
fn local_pure_fn_table_shares_registry() {
    let ctx = make_context_with_bifs();
    let mut tables1 = ctx.local_tables();
    let mut tables2 = ctx.local_tables();
    // Both should see the same registry.
    let key = LocalFnKey::new("uuid", 0);
    let gk = FnId {
        module: builtin_mod(),
        name: "uuid".into(),
        arity: 0,
    };
    tables1
        .pure_fns
        .insert(key.clone(), gk.clone(), IrSpan::synthetic());
    tables2
        .pure_fns
        .insert(key.clone(), gk, IrSpan::synthetic());
    assert!(tables1.pure_fns.get(&key).is_some());
    assert!(tables2.pure_fns.get(&key).is_some());
}

#[test]
fn local_effect_table_initially_empty() {
    let ctx = make_context();
    let tables = ctx.local_tables();
    let key = LocalEffectKey::new(EffectName("Db".into()));
    assert!(tables.effects.get(&key).is_none());
}

#[test]
fn local_fn_table_independent_locals() {
    let ctx = make_context_with_bifs();
    let mut tables1 = ctx.local_tables();
    let mut tables2 = ctx.local_tables();
    let gk = FnId {
        module: builtin_mod(),
        name: "trim".into(),
        arity: 1,
    };
    tables1.fns.insert(
        LocalFnKey::new("my_trim", 1),
        gk.clone(),
        IrSpan::synthetic(),
    );
    tables2
        .fns
        .insert(LocalFnKey::new("your_trim", 1), gk, IrSpan::synthetic());
    // tables1 doesn't see tables2's local mapping.
    assert!(tables1.fns.get(&LocalFnKey::new("your_trim", 1)).is_none());
    assert!(tables2.fns.get(&LocalFnKey::new("my_trim", 1)).is_none());
}

// ═══════════════════════════════════════════════════════════
// Local table population — own definitions
// ═══════════════════════════════════════════════════════════

#[test]
fn populate_own_fn_definitions() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![make_fn_def("foo", 1)]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    let result = ctx.populate_local_tables(&mod_path, &file_id, &mut tables);
    assert!(result.is_ok());
    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 1)));
}

#[test]
fn populate_own_fn_multiple_arities() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![make_fn_def("foo", 0), make_fn_def("foo", 1)]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&mod_path, &file_id, &mut tables)
        .unwrap();
    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 0)));
    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 1)));
}

#[test]
fn populate_own_effect_definitions() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![make_effect_def("Db")]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&mod_path, &file_id, &mut tables)
        .unwrap();
    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("Db".into())))
    );
}

#[test]
fn populate_own_pure_fn_definitions() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![make_pure_fn_def("bar", 0)]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&mod_path, &file_id, &mut tables)
        .unwrap();
    // Pure fns go in both tables.
    assert!(tables.fns.contains_local(&LocalFnKey::new("bar", 0)));
    assert!(tables.pure_fns.contains_local(&LocalFnKey::new("bar", 0)));
}

#[test]
fn populate_own_mixed_definitions() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![
        make_fn_def("impure_fn", 1),
        make_pure_fn_def("pure_fn", 0),
        make_effect_def("Setup"),
    ]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&mod_path, &file_id, &mut tables)
        .unwrap();
    assert!(tables.fns.contains_local(&LocalFnKey::new("impure_fn", 1)));
    assert!(tables.fns.contains_local(&LocalFnKey::new("pure_fn", 0)));
    assert!(
        tables
            .pure_fns
            .contains_local(&LocalFnKey::new("pure_fn", 0))
    );
    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("Setup".into())))
    );
}

#[test]
fn populate_empty_module() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&mod_path, &file_id, &mut tables)
        .unwrap();
    // No local entries (BIFs are in the registry, not local mappings).
    assert!(!tables.fns.contains_local(&LocalFnKey::new("anything", 0)));
}

// ═══════════════════════════════════════════════════════════
// Local table population — imports
// ═══════════════════════════════════════════════════════════

#[test]
fn populate_wildcard_import() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![
        make_fn_def("foo", 1),
        make_pure_fn_def("bar", 0),
        make_effect_def("StartDb"),
    ]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    // import helpers (wildcard — no names)
    let test_mod = make_module(vec![make_import("helpers", None)]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 1)));
    assert!(tables.fns.contains_local(&LocalFnKey::new("bar", 0)));
    assert!(tables.pure_fns.contains_local(&LocalFnKey::new("bar", 0)));
    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("StartDb".into())))
    );
}

#[test]
fn populate_selective_import_fn() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 1), make_fn_def("bar", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("helpers", Some(vec![("foo", None)]))]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 1)));
    // bar was not selectively imported.
    assert!(!tables.fns.contains_local(&LocalFnKey::new("bar", 0)));
}

#[test]
fn populate_selective_import_effect() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_effect_def("StartDb")]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("helpers", Some(vec![("StartDb", None)]))]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("StartDb".into())))
    );
}

#[test]
fn populate_selective_import_multiple() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![
        make_fn_def("foo", 0),
        make_fn_def("bar", 1),
        make_effect_def("StartDb"),
    ]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import(
        "helpers",
        Some(vec![("foo", None), ("bar", None), ("StartDb", None)]),
    )]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(tables.fns.contains_local(&LocalFnKey::new("foo", 0)));
    assert!(tables.fns.contains_local(&LocalFnKey::new("bar", 1)));
    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("StartDb".into())))
    );
}

#[test]
fn populate_aliased_fn_import() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 1)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import(
        "helpers",
        Some(vec![("foo", Some("bar"))]),
    )]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    // "bar" is the local alias for "foo".
    assert!(tables.fns.contains_local(&LocalFnKey::new("bar", 1)));
    // "foo" should NOT be in the local table — only the alias.
    assert!(!tables.fns.contains_local(&LocalFnKey::new("foo", 1)));
}

#[test]
fn populate_aliased_effect_import() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_effect_def("StartDb")]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import(
        "helpers",
        Some(vec![("StartDb", Some("Db"))]),
    )]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("Db".into())))
    );
    assert!(
        !tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("StartDb".into())))
    );
}

#[test]
fn populate_multiple_imports() {
    let h1_path = ModulePath("lib/h1".into());
    let h1_fid = FileId::new(PathBuf::from("/proj/lib/h1.relux"));
    let h1_mod = make_module(vec![make_fn_def("alpha", 0)]);

    let h2_path = ModulePath("lib/h2".into());
    let h2_fid = FileId::new(PathBuf::from("/proj/lib/h2.relux"));
    let h2_mod = make_module(vec![make_fn_def("beta", 1)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("h1", None), make_import("h2", None)]);

    let ctx = make_context_with_ast(vec![
        (h1_path, h1_fid, h1_mod),
        (h2_path, h2_fid, h2_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    assert!(tables.fns.contains_local(&LocalFnKey::new("alpha", 0)));
    assert!(tables.fns.contains_local(&LocalFnKey::new("beta", 1)));
}

#[test]
fn populate_wildcard_does_not_import_bifs() {
    // Wildcard import from a module only imports that module's definitions,
    // not BIFs that happen to be in the registry.
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("my_fn", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("helpers", None)]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    ctx.populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap();

    // my_fn is imported, but BIFs like "trim" are not in local mappings.
    assert!(tables.fns.contains_local(&LocalFnKey::new("my_fn", 0)));
    assert!(!tables.fns.contains_local(&LocalFnKey::new("trim", 1)));
}

// ═══════════════════════════════════════════════════════════
// Local table population — error cases
// ═══════════════════════════════════════════════════════════

#[test]
fn populate_import_missing_module() {
    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("nonexistent", None)]);

    let ctx = make_context_with_ast(vec![(test_path.clone(), test_fid.clone(), test_mod)]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::UndefinedModuleImport { .. }));
}

#[test]
fn populate_import_missing_fn_name() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import(
        "helpers",
        Some(vec![("nonexistent", None)]),
    )]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::UndefinedFunctionImport { .. }));
}

#[test]
fn populate_import_missing_effect_name() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import(
        "helpers",
        Some(vec![("MissingEffect", None)]),
    )]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::UndefinedEffectImport { .. }));
}

#[test]
fn populate_name_conflict_two_imports() {
    let h1_path = ModulePath("lib/h1".into());
    let h1_fid = FileId::new(PathBuf::from("/proj/lib/h1.relux"));
    let h1_mod = make_module(vec![make_fn_def("foo", 0)]);

    let h2_path = ModulePath("lib/h2".into());
    let h2_fid = FileId::new(PathBuf::from("/proj/lib/h2.relux"));
    let h2_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("h1", None), make_import("h2", None)]);

    let ctx = make_context_with_ast(vec![
        (h1_path, h1_fid, h1_mod),
        (h2_path, h2_fid, h2_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::NameConflict { .. }));
}

#[test]
fn populate_name_conflict_own_and_import() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_fn_def("foo", 0), make_import("helpers", None)]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::NameConflict { .. }));
}

#[test]
fn populate_name_conflict_wildcard() {
    let helpers_path = ModulePath("lib/helpers".into());
    let helpers_fid = FileId::new(PathBuf::from("/proj/lib/helpers.relux"));
    let helpers_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_fn_def("foo", 0), make_import("helpers", None)]);

    let ctx = make_context_with_ast(vec![
        (helpers_path, helpers_fid, helpers_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    assert!(matches!(err, InvalidReport::NameConflict { .. }));
}

#[test]
fn populate_fn_and_effect_same_name_ok() {
    let mod_path = ModulePath("tests/a".into());
    let file_id = test_file_id();
    let module = make_module(vec![make_fn_def("setup", 0), make_effect_def("Setup")]);
    let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

    let mut tables = ctx.local_tables();
    let result = ctx.populate_local_tables(&mod_path, &file_id, &mut tables);
    assert!(result.is_ok());
    assert!(tables.fns.contains_local(&LocalFnKey::new("setup", 0)));
    assert!(
        tables
            .effects
            .contains_local(&LocalEffectKey::new(EffectName("Setup".into())))
    );
}

#[test]
fn populate_name_conflict_reports_both_spans() {
    let h1_path = ModulePath("lib/h1".into());
    let h1_fid = FileId::new(PathBuf::from("/proj/lib/h1.relux"));
    let h1_mod = make_module(vec![make_fn_def("foo", 0)]);

    let h2_path = ModulePath("lib/h2".into());
    let h2_fid = FileId::new(PathBuf::from("/proj/lib/h2.relux"));
    let h2_mod = make_module(vec![make_fn_def("foo", 0)]);

    let test_path = ModulePath("tests/a".into());
    let test_fid = test_file_id();
    let test_mod = make_module(vec![make_import("h1", None), make_import("h2", None)]);

    let ctx = make_context_with_ast(vec![
        (h1_path, h1_fid, h1_mod),
        (h2_path, h2_fid, h2_mod),
        (test_path.clone(), test_fid.clone(), test_mod),
    ]);

    let mut tables = ctx.local_tables();
    let err = ctx
        .populate_local_tables(&test_path, &test_fid, &mut tables)
        .unwrap_err();
    if let InvalidReport::NameConflict { first, second, .. } = &err {
        // Both spans should be present.
        let _ = first.file();
        let _ = second.file();
    } else {
        panic!("expected NameConflict");
    }
}

// ═══════════════════════════════════════════════════════════
// Cause registration
// ═══════════════════════════════════════════════════════════

#[test]
fn register_cause_returns_id() {
    let ctx = make_context();
    let id = CauseId::generate("m", "f", 0, "err");
    ctx.register_cause(
        id.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    assert!(ctx.causes().get(&id).is_some());
}

#[test]
fn register_cause_retrievable() {
    let ctx = make_context();
    let id = CauseId::generate("m", "f", 0, "err");
    ctx.register_cause(
        id.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    let cause = ctx.causes().get(&id).unwrap();
    assert!(matches!(cause, Cause::Invalid(_)));
}

#[test]
fn register_multiple_causes() {
    let ctx = make_context();
    let id1 = CauseId::generate("m", "f", 0, "err1");
    let id2 = CauseId::generate("m", "g", 1, "err2");
    ctx.register_cause(
        id1.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    ctx.register_cause(
        id2.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    assert!(ctx.causes().get(&id1).is_some());
    assert!(ctx.causes().get(&id2).is_some());
}

#[test]
fn register_cause_skip_variant() {
    let ctx = make_context();
    let id = CauseId::generate("m", "f", 0, "skip");
    let skip = SkipReport {
        definition: DefinitionRef::Fn(FnId {
            module: ModulePath("m".into()),
            name: "f".into(),
            arity: 0,
        }),
        marker_span: test_span(),
        evaluation: SkipEvaluation::Unconditional,
    };
    ctx.register_cause(id.clone(), Cause::skip(skip));
    assert!(matches!(ctx.causes().get(&id).unwrap(), Cause::Skip(_)));
}

#[test]
fn register_cause_invalid_variant() {
    let ctx = make_context();
    let id = CauseId::generate("m", "f", 0, "invalid");
    ctx.register_cause(
        id.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    assert!(matches!(ctx.causes().get(&id).unwrap(), Cause::Invalid(_)));
}

// ═══════════════════════════════════════════════════════════
// In-progress stacks — function
// ═══════════════════════════════════════════════════════════

fn make_fn_id(name: &str, arity: usize) -> FnId {
    FnId {
        module: ModulePath("m".into()),
        name: name.into(),
        arity,
    }
}

#[test]
fn push_pop_fn_stack() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span());
    assert_eq!(ctx.fn_stack().len(), 1);
    ctx.pop_fn();
    assert!(ctx.fn_stack().is_empty());
}

#[test]
fn push_pop_fn_stack_nested() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span());
    ctx.push_fn(make_fn_id("b", 0), test_span());
    assert_eq!(ctx.fn_stack().len(), 2);
    ctx.pop_fn();
    assert_eq!(ctx.fn_stack().len(), 1);
    assert_eq!(ctx.fn_stack()[0].0.name, "a");
}

#[test]
fn find_fn_cycle_empty_stack() {
    let ctx = make_context();
    assert!(ctx.find_fn_cycle(&make_fn_id("a", 0)).is_none());
}

#[test]
fn find_fn_cycle_self() {
    let mut ctx = make_context();
    let id = make_fn_id("a", 0);
    ctx.push_fn(id.clone(), test_span());
    let cycle = ctx.find_fn_cycle(&id).unwrap();
    if let CycleReport::Function { chain } = &cycle {
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].id.name, "a");
    } else {
        panic!("expected Function cycle");
    }
}

#[test]
fn find_fn_cycle_chain() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span());
    ctx.push_fn(make_fn_id("b", 0), test_span());
    let cycle = ctx.find_fn_cycle(&make_fn_id("a", 0)).unwrap();
    if let CycleReport::Function { chain } = &cycle {
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].id.name, "a");
        assert_eq!(chain[1].id.name, "b");
    } else {
        panic!("expected Function cycle");
    }
}

#[test]
fn find_fn_cycle_deep() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span());
    ctx.push_fn(make_fn_id("b", 0), test_span());
    ctx.push_fn(make_fn_id("c", 0), test_span());
    let cycle = ctx.find_fn_cycle(&make_fn_id("a", 0)).unwrap();
    if let CycleReport::Function { chain } = &cycle {
        assert_eq!(chain.len(), 3);
    } else {
        panic!("expected Function cycle");
    }
}

#[test]
fn find_fn_cycle_not_on_stack() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span());
    assert!(ctx.find_fn_cycle(&make_fn_id("b", 0)).is_none());
}

#[test]
fn find_fn_cycle_chain_preserves_spans() {
    let mut ctx = make_context();
    ctx.push_fn(make_fn_id("a", 0), test_span_at(10, 20));
    ctx.push_fn(make_fn_id("b", 0), test_span_at(30, 40));
    let cycle = ctx.find_fn_cycle(&make_fn_id("a", 0)).unwrap();
    if let CycleReport::Function { chain } = &cycle {
        assert_eq!(chain[0].call_span.span(), &Span::new(10, 20));
        assert_eq!(chain[1].call_span.span(), &Span::new(30, 40));
    } else {
        panic!("expected Function cycle");
    }
}

// ═══════════════════════════════════════════════════════════
// In-progress stacks — effect
// ═══════════════════════════════════════════════════════════

fn make_effect_id(name: &str) -> EffectId {
    EffectId {
        module: ModulePath("m".into()),
        name: EffectName(name.into()),
    }
}

#[test]
fn push_pop_effect_stack() {
    let mut ctx = make_context();
    ctx.push_effect(make_effect_id("A"), test_span());
    assert_eq!(ctx.effect_stack().len(), 1);
    ctx.pop_effect();
    assert!(ctx.effect_stack().is_empty());
}

#[test]
fn find_effect_cycle_self() {
    let mut ctx = make_context();
    let id = make_effect_id("A");
    ctx.push_effect(id.clone(), test_span());
    let cycle = ctx.find_effect_cycle(&id).unwrap();
    assert!(matches!(cycle, CycleReport::Effect { .. }));
}

#[test]
fn find_effect_cycle_chain() {
    let mut ctx = make_context();
    ctx.push_effect(make_effect_id("A"), test_span());
    ctx.push_effect(make_effect_id("B"), test_span());
    let cycle = ctx.find_effect_cycle(&make_effect_id("A")).unwrap();
    if let CycleReport::Effect { chain } = &cycle {
        assert_eq!(chain.len(), 2);
    } else {
        panic!("expected Effect cycle");
    }
}

#[test]
fn find_effect_cycle_not_on_stack() {
    let mut ctx = make_context();
    ctx.push_effect(make_effect_id("A"), test_span());
    assert!(ctx.find_effect_cycle(&make_effect_id("B")).is_none());
}

// ═══════════════════════════════════════════════════════════
// Stack independence
// ═══════════════════════════════════════════════════════════

#[test]
fn fn_and_effect_stacks_independent() {
    let mut ctx = make_context();
    // Push a fn with name "a" and an effect with name "a".
    ctx.push_fn(make_fn_id("a", 0), test_span());
    ctx.push_effect(make_effect_id("a"), test_span());
    // fn cycle check for effect ID type — different stacks.
    assert!(ctx.find_fn_cycle(&make_fn_id("a", 0)).is_some());
    assert!(ctx.find_effect_cycle(&make_effect_id("a")).is_some());
    // Cross-check: fn "a" is not an effect cycle trigger.
    assert!(ctx.find_effect_cycle(&make_effect_id("b")).is_none());
    assert!(ctx.find_fn_cycle(&make_fn_id("b", 0)).is_none());
}

#[test]
fn effect_lowering_uses_both_stacks() {
    let mut ctx = make_context();
    // Simulate: effect A is being lowered, and within it fn B is called.
    ctx.push_effect(make_effect_id("A"), test_span());
    ctx.push_fn(make_fn_id("b", 0), test_span());
    // Both stacks have entries.
    assert_eq!(ctx.effect_stack().len(), 1);
    assert_eq!(ctx.fn_stack().len(), 1);
    // Fn cycle for "b" found, effect cycle for "A" found.
    assert!(ctx.find_fn_cycle(&make_fn_id("b", 0)).is_some());
    assert!(ctx.find_effect_cycle(&make_effect_id("A")).is_some());
}

// ═══════════════════════════════════════════════════════════
// into_suite
// ═══════════════════════════════════════════════════════════

#[test]
fn into_suite_transfers_plans() {
    let ctx = make_context();
    let meta = TestMeta::new("test1", None, None, test_span());
    let plan = Plan::Invalid {
        meta,
        causes: vec![],
        warnings: vec![],
    };
    let suite = ctx.into_suite(vec![plan]);
    assert_eq!(suite.plans.len(), 1);
}

#[test]
fn into_suite_transfers_source_table() {
    let source_table: SourceTable = SharedTable::new();
    let fid = test_file_id();
    source_table.insert(
        fid.clone(),
        SourceFile::new(PathBuf::from("/test/file.relux"), "// test".into()),
    );

    let ctx = LoweringContext::new(
        empty_ast_table(),
        source_table,
        test_env(),
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    );
    let suite = ctx.into_suite(vec![]);
    assert!(suite.tables.sources.get(&fid).is_some());
}

#[test]
fn into_suite_transfers_env() {
    let mut m = HashMap::new();
    m.insert("MY_VAR".into(), "my_val".into());
    let env = Arc::new(LayeredEnv::from(Env::from_map(m)));
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        env,
        CauseTable::default(),
        WarningTable::default(),
        1.0,
    );
    let suite = ctx.into_suite(vec![]);
    assert_eq!(suite.env.get("MY_VAR"), Some("my_val"));
}

#[test]
fn into_suite_transfers_causes() {
    let causes: CauseTable = SharedTable::new();
    let id = CauseId::generate("m", "f", 0, "err");
    causes.insert(
        id.clone(),
        Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
    );
    let ctx = LoweringContext::new(
        empty_ast_table(),
        empty_source_table(),
        test_env(),
        causes,
        WarningTable::default(),
        1.0,
    );
    let suite = ctx.into_suite(vec![]);
    assert!(suite.causes.get(&id).is_some());
}

#[test]
fn into_suite_empty() {
    let ctx = make_context();
    let suite = ctx.into_suite(vec![]);
    assert!(suite.plans.is_empty());
}

// ─── Cross-module lowering ────────────────────────────────

#[test]
fn lower_imported_fn_call() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import helpers\ntest \"t\" {\n  shell sh {\n    greet()\n  }\n}\n",
        ),
        (
            "lib/helpers",
            "/lib/helpers.relux",
            "fn greet() {\n  > hello\n}\n",
        ),
    ]);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    let greet_id = FnId {
        module: ModulePath("lib/helpers".into()),
        name: "greet".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&greet_id).is_some());
}

#[test]
fn lower_transitive_fn_call() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import helpers\ntest \"t\" {\n  shell sh {\n    caller()\n  }\n}\n",
        ),
        (
            "lib/helpers",
            "/lib/helpers.relux",
            "fn helper() {\n  > help\n}\nfn caller() {\n  helper()\n}\n",
        ),
    ]);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    let helper_id = FnId {
        module: ModulePath("lib/helpers".into()),
        name: "helper".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&helper_id).is_some());
}

#[test]
fn lower_deep_transitive_fn_call() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import top\ntest \"t\" {\n  shell sh {\n    top()\n  }\n}\n",
        ),
        (
            "lib/top",
            "/lib/top.relux",
            "import mid\nfn top() {\n  mid()\n}\n",
        ),
        (
            "lib/mid",
            "/lib/mid.relux",
            "import deep\nfn mid() {\n  deep()\n}\n",
        ),
        ("lib/deep", "/lib/deep.relux", "fn deep() {\n  > deep\n}\n"),
    ]);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    assert!(
        ctx.functions()
            .get(&FnId {
                module: ModulePath("lib/deep".into()),
                name: "deep".into(),
                arity: 0,
            })
            .is_some()
    );
}

#[test]
fn lower_fn_from_different_modules_distinct() {
    let mut ctx = ctx_with_modules(vec![
        ("tests/a", "/test/a.relux", "fn foo() {\n  > a\n}\n"),
        ("tests/b", "/test/b.relux", "fn foo() {\n  > b\n}\n"),
    ]);
    let a_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "foo".into(),
        arity: 0,
    };
    let b_id = FnId {
        module: ModulePath("tests/b".into()),
        name: "foo".into(),
        arity: 0,
    };
    assert_ne!(a_id, b_id);
    ctx.resolve_fn(&a_id).unwrap();
    ctx.resolve_fn(&b_id).unwrap();
    assert!(ctx.functions().get(&a_id).is_some());
    assert!(ctx.functions().get(&b_id).is_some());
}

#[test]
fn lower_diamond_dependency_fn() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import shared\nfn a() {\n  shared()\n}\n",
        ),
        (
            "tests/b",
            "/test/b.relux",
            "import shared\nfn b() {\n  shared()\n}\n",
        ),
        (
            "lib/shared",
            "/lib/shared.relux",
            "fn shared() {\n  > s\n}\n",
        ),
    ]);
    let a_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "a".into(),
        arity: 0,
    };
    let b_id = FnId {
        module: ModulePath("tests/b".into()),
        name: "b".into(),
        arity: 0,
    };
    ctx.resolve_fn(&a_id).unwrap();
    ctx.resolve_fn(&b_id).unwrap();
    let shared_id = FnId {
        module: ModulePath("lib/shared".into()),
        name: "shared".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&shared_id).is_some());
}

#[test]
fn lower_imported_effect_with_sub_starts() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import effects\ntest \"t\" {\n  start App\n  shell sh {\n    > cmd\n  }\n}\n",
        ),
        (
            "lib/effects",
            "/lib/effects.relux",
            "effect Db {\n  shell db {\n    > db\n  }\n}\neffect App {\n  start Db\n  shell app {\n    > app\n  }\n}\n",
        ),
    ]);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_ok());
    let db_id = EffectId {
        module: ModulePath("lib/effects".into()),
        name: EffectName("Db".into()),
    };
    assert!(ctx.effects().get(&db_id).is_some());
}

// ─── Skip and invalid propagation ─────────────────────────

#[test]
fn lower_fn_invalid_propagates_to_caller() {
    let source = r#"fn bad() {
  nonexistent()
}
fn caller() {
  bad()
}
"#;
    let mut ctx = ctx_with_source(source);
    let fn_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "caller".into(),
        arity: 0,
    };
    let result = ctx.resolve_fn(&fn_id);
    assert!(result.is_err());
}

#[test]
fn lower_effect_invalid_propagates() {
    let source = r#"effect Bad {
  start Nonexistent
  shell bad {
    > x
  }
}
effect User {
  start Bad
  shell user {
    > y
  }
}
"#;
    let mut ctx = ctx_with_source(source);
    let effect_id = EffectId {
        module: ModulePath("tests/a".into()),
        name: EffectName("User".into()),
    };
    let result = ctx.resolve_effect(&effect_id);
    assert!(result.is_err());
}

#[test]
fn lower_transitive_invalid_three_levels() {
    let source = r#"fn bad() {
  nonexistent()
}
fn mid() {
  bad()
}
fn top() {
  mid()
}
"#;
    let mut ctx = ctx_with_source(source);
    let fn_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "top".into(),
        arity: 0,
    };
    let result = ctx.resolve_fn(&fn_id);
    assert!(result.is_err());
    let mid_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "mid".into(),
        arity: 0,
    };
    let bad_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "bad".into(),
        arity: 0,
    };
    assert!(ctx.functions().get(&mid_id).unwrap().is_err());
    assert!(ctx.functions().get(&bad_id).unwrap().is_err());
}

// ─── Span accuracy ───────────────────────────────────────

#[test]
fn lower_span_points_to_correct_file() {
    let mut ctx = ctx_with_modules(vec![
        (
            "tests/a",
            "/test/a.relux",
            "import helpers\ntest \"t\" {\n  shell sh {\n    broken()\n  }\n}\n",
        ),
        (
            "lib/helpers",
            "/lib/helpers.relux",
            "fn broken() {\n  nonexistent()\n}\n",
        ),
    ]);
    let result = lower_first_test(&mut ctx, "tests/a");
    assert!(result.is_err());
    if let Err(LoweringBail::Invalid(inner)) = &result {
        if let InvalidReport::UndefinedFunctionCall { span, .. } = inner.as_ref() {
            assert_eq!(
                span.file(),
                &FileId::new(PathBuf::from("/lib/helpers.relux"))
            );
        } else {
            panic!("expected UndefinedFunctionCall, got {:?}", result);
        }
    } else {
        panic!("expected UndefinedFunctionCall, got {:?}", result);
    }
}

#[test]
fn lower_undefined_call_span_covers_name() {
    let source = "fn caller() {\n  nonexistent()\n}\n";
    let mut ctx = ctx_with_source(source);
    let fn_id = FnId {
        module: ModulePath("tests/a".into()),
        name: "caller".into(),
        arity: 0,
    };
    let result = ctx.resolve_fn(&fn_id);
    if let Err(LoweringBail::Invalid(inner)) = &result {
        if let InvalidReport::UndefinedFunctionCall { name, span, .. } = inner.as_ref() {
            assert_eq!(name, "nonexistent");
            let s = span.span();
            assert!(s.end() > s.start());
        } else {
            panic!("expected UndefinedFunctionCall, got {:?}", result);
        }
    } else {
        panic!("expected UndefinedFunctionCall, got {:?}", result);
    }
}

// ─── Plan building: invalid paths ──────────────────────────

// ─── Plan building: precedence ─────────────────────────────

// ─── Memoization ──────────────────────────────────────────

#[test]
fn memoization_shared_fn_lowered_once() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn shared() {
  > echo hello
}

test "t1" {
  shell sh {
    shared()
  }
}

test "t2" {
  shell sh {
    shared()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_runnable(&suite.plans[1]));
}

#[test]
fn memoization_shared_error_propagates() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn broken() {
  nonexistent()
}

test "t1" {
  shell sh {
    broken()
  }
}

test "t2" {
  shell sh {
    broken()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(is_invalid(&suite.plans[0]));
    assert!(is_invalid(&suite.plans[1]));
}

#[test]
fn memoization_shared_effect_lowered_once() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Setup {
  shell sh {
    > echo setup
  }
}

test "t1" {
  start Setup
  shell sh {
    > echo 1
  }
}

test "t2" {
  start Setup
  shell sh {
    > echo 2
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(is_runnable(&suite.plans[0]));
    assert!(is_runnable(&suite.plans[1]));
}

#[test]
fn memoization_fn_ok_and_error_independent() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn good() {
  > echo hello
}

fn bad() {
  nonexistent()
}

test "uses good" {
  shell sh {
    good()
  }
}

test "uses bad" {
  shell sh {
    bad()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    let good_plan = suite
        .plans
        .iter()
        .find(|p| plan_name(p) == "uses good")
        .unwrap();
    let bad_plan = suite
        .plans
        .iter()
        .find(|p| plan_name(p) == "uses bad")
        .unwrap();
    assert!(is_runnable(good_plan));
    assert!(is_invalid(bad_plan));
}

// ─── Cross-module ──────────────────────────────────────────

#[test]
fn cross_module_fn_import() {
    let suite = resolve_source_no_env(&[
        (
            "lib/helpers",
            r#"fn greet() {
  > echo hello
}
"#,
        ),
        (
            "tests/a",
            r#"import helpers

test "uses import" {
  shell sh {
    greet()
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn cross_module_effect_import() {
    let suite = resolve_source_no_env(&[
        (
            "lib/effects",
            r#"effect Db {
  shell db_sh {
    > echo db
  }
}
"#,
        ),
        (
            "tests/a",
            r#"import effects

test "uses effect" {
  start Db
  shell sh {
    > echo hello
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn cross_module_transitive_fn() {
    let suite = resolve_source_no_env(&[
        (
            "lib/base",
            r#"fn base_fn() {
  > echo base
}
"#,
        ),
        (
            "lib/mid",
            r#"import base

fn mid_fn() {
  base_fn()
}
"#,
        ),
        (
            "tests/a",
            r#"import mid

test "transitive" {
  shell sh {
    mid_fn()
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn cross_module_diamond_import() {
    let suite = resolve_source_no_env(&[
        (
            "lib/base",
            r#"fn shared() {
  > echo shared
}
"#,
        ),
        (
            "lib/a",
            r#"import base

fn use_a() {
  shared()
}
"#,
        ),
        (
            "lib/b",
            r#"import base

fn use_b() {
  shared()
}
"#,
        ),
        (
            "tests/a",
            r#"import a
import b

test "diamond" {
  shell sh {
    use_a()
    use_b()
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn cross_module_multiple_tests_across_modules() {
    let suite = resolve_source_no_env(&[
        (
            "tests/a",
            r#"test "test a" {
  shell sh {
    > echo a
  }
}
"#,
        ),
        (
            "tests/b",
            r#"test "test b" {
  shell sh {
    > echo b
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 2);
    assert!(suite.plans.iter().all(is_runnable));
}

#[test]
fn cross_module_plans_sorted_by_module_path() {
    let suite = resolve_source_no_env(&[
        (
            "tests/z_last",
            r#"test "z test" {
  shell sh {
    > echo z
  }
}
"#,
        ),
        (
            "tests/a_first",
            r#"test "a test" {
  shell sh {
    > echo a
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 2);
    assert_eq!(plan_name(&suite.plans[0]), "a test");
    assert_eq!(plan_name(&suite.plans[1]), "z test");
}

// ─── Integration: cycle detection ─────────────────────────

#[test]
fn fn_cycle_self_recursive() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn a() {
  a()
}

test "t" {
  shell sh {
    a()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn fn_cycle_mutual_two() {
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
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn fn_cycle_three_way() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn a() {
  b()
}

fn b() {
  c()
}

fn c() {
  a()
}

test "t" {
  shell sh {
    a()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn effect_cycle_via_start() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect A {
  start B
  shell sh {
    > echo a
  }
}

effect B {
  start A
  shell sh {
    > echo b
  }
}

test "t" {
  start A
  shell sh {
    > echo t
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn fn_cycle_cross_module() {
    let suite = resolve_source_no_env(&[
        (
            "lib/a",
            r#"import b

fn fa() {
  fb()
}
"#,
        ),
        (
            "lib/b",
            r#"import a

fn fb() {
  fa()
}
"#,
        ),
        (
            "tests/t",
            r#"import a

test "t" {
  shell sh {
    fa()
  }
}
"#,
        ),
    ]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

// ─── Integration: error propagation ───────────────────────

#[test]
fn invalid_dependency_propagates_to_caller() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"fn broken() {
  nonexistent()
}

test "direct" {
  shell sh {
    broken()
  }
}

test "also broken" {
  shell sh {
    broken()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 2);
    assert!(suite.plans.iter().all(is_invalid));
}

#[test]
fn skip_dependency_propagates_transitively() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"# skip
fn skipped_fn() {
  > echo hello
}

test "calls skipped" {
  shell sh {
    skipped_fn()
  }
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_skipped(&suite.plans[0]));
}

#[test]
fn undefined_effect_start() {
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
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn empty_test_body_is_invalid() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"test "t" {
  shell sh {}
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn test_with_no_shell_blocks_is_invalid() {
    // A test with needs but no shell blocks should be invalid.
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  shell db {
    > echo db
  }
}

test "t" {
  start Db
}
"#,
    )]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn completely_empty_test_is_invalid() {
    let suite = resolve_source_no_env(&[("tests/a", "test \"t\" {}\n")]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn test_with_only_comment_is_invalid() {
    let suite = resolve_source_no_env(&[("tests/a", "test \"t\" {\n  // just a comment\n}\n")]);
    assert_eq!(suite.plans.len(), 1);
    assert!(is_invalid(&suite.plans[0]));
}

// ═══════════════════════════════════════════════════════════
// Timeout multiplier propagation
// ═══════════════════════════════════════════════════════════

use std::time::Duration;

/// Extract the first shell block's statements from a runnable plan.
fn first_shell_stmts(plan: &Plan) -> &[IrShellStmt] {
    let Plan::Runnable { test, .. } = plan else {
        panic!("expected Runnable plan");
    };
    for item in test.body() {
        if let IrTestItem::Shell { block, .. } = item {
            return block.body();
        }
    }
    panic!("no shell block found");
}

#[test]
fn multiplier_scales_scoped_tolerance_timeout() {
    let suite = resolve_source_with_multiplier(
        &[(
            "tests/a",
            r#"test "t" {
  """
  Doc.
  """
  shell s {
    ~10s
    > echo hi
  }
}
"#,
        )],
        2.0,
    );
    let stmts = first_shell_stmts(&suite.plans[0]);
    let IrShellStmt::Timeout { timeout, .. } = &stmts[0] else {
        panic!("expected Timeout stmt, got {:?}", stmts[0]);
    };
    assert_eq!(timeout.raw_duration(), Duration::from_secs(10));
    assert_eq!(timeout.adjusted_duration(), Duration::from_secs(20));
}

#[test]
fn multiplier_scales_inline_timed_regex_match() {
    let suite = resolve_source_with_multiplier(
        &[(
            "tests/a",
            r#"test "t" {
  """
  Doc.
  """
  shell s {
    > echo hi
    <~5s? ^hi$
  }
}
"#,
        )],
        3.0,
    );
    let stmts = first_shell_stmts(&suite.plans[0]);
    let timed = stmts
        .iter()
        .find(|s| matches!(s, IrShellStmt::TimedMatchRegex { .. }));
    let IrShellStmt::TimedMatchRegex { timeout, .. } = timed.unwrap() else {
        unreachable!();
    };
    assert_eq!(timeout.raw_duration(), Duration::from_secs(5));
    assert_eq!(timeout.adjusted_duration(), Duration::from_secs(15));
}

#[test]
fn multiplier_scales_inline_timed_literal_match() {
    let suite = resolve_source_with_multiplier(
        &[(
            "tests/a",
            r#"test "t" {
  """
  Doc.
  """
  shell s {
    > echo hi
    <~5s= hi
  }
}
"#,
        )],
        0.5,
    );
    let stmts = first_shell_stmts(&suite.plans[0]);
    let timed = stmts
        .iter()
        .find(|s| matches!(s, IrShellStmt::TimedMatchLiteral { .. }));
    let IrShellStmt::TimedMatchLiteral { timeout, .. } = timed.unwrap() else {
        unreachable!();
    };
    assert_eq!(timeout.raw_duration(), Duration::from_secs(5));
    assert_eq!(timeout.adjusted_duration(), Duration::from_millis(2500));
}

#[test]
fn multiplier_does_not_scale_assertion_timeout() {
    let suite = resolve_source_with_multiplier(
        &[(
            "tests/a",
            r#"test "t" {
  """
  Doc.
  """
  shell s {
    @5s
    > echo hi
  }
}
"#,
        )],
        3.0,
    );
    let stmts = first_shell_stmts(&suite.plans[0]);
    let IrShellStmt::Timeout { timeout, .. } = &stmts[0] else {
        panic!("expected Timeout stmt");
    };
    assert!(timeout.is_assertion());
    assert_eq!(timeout.adjusted_duration(), Duration::from_secs(5));
}

#[test]
fn default_multiplier_leaves_tolerance_unscaled() {
    let suite = resolve_source_with_multiplier(
        &[(
            "tests/a",
            r#"test "t" {
  """
  Doc.
  """
  shell s {
    ~10s
    > echo hi
  }
}
"#,
        )],
        1.0,
    );
    let stmts = first_shell_stmts(&suite.plans[0]);
    let IrShellStmt::Timeout { timeout, .. } = &stmts[0] else {
        panic!("expected Timeout stmt");
    };
    assert_eq!(timeout.adjusted_duration(), Duration::from_secs(10));
}

// ─── Expect satisfiability ──────────────────────────────

#[test]
fn expect_satisfied_by_overlay() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  expect PORT
  shell db {
    > start --port ${PORT}
  }
}
test "t" {
  start Db { PORT = "5432" }
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn expect_satisfied_by_base_env() {
    let suite = resolve_source(
        &[(
            "tests/a",
            r#"effect Db {
  expect HOME
  shell db {
    > start
  }
}
test "t" {
  start Db
  shell sh {
    > echo ok
  }
}
"#,
        )],
        HashMap::from([("HOME".into(), "/home/user".into())]),
    );
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn expect_satisfied_by_let_binding() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  expect PORT
  shell db {
    > start
  }
}
test "t" {
  let PORT = "5432"
  start Db
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn expect_unsatisfied_produces_invalid() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Db {
  expect PORT
  shell db {
    > start
  }
}
test "t" {
  start Db
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn expect_nested_effect_satisfied() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Inner {
  expect BAR
  shell s {
    > inner
  }
}
effect Outer {
  expect FOO
  start Inner { BAR = FOO }
  shell s {
    > outer
  }
}
test "t" {
  start Outer { FOO = "x" }
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}

#[test]
fn expect_nested_unsatisfied_produces_invalid() {
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect Inner {
  expect BAR
  shell s {
    > inner
  }
}
effect Outer {
  start Inner
  shell s {
    > outer
  }
}
test "t" {
  start Outer
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_invalid(&suite.plans[0]));
}

#[test]
fn expect_shallow_env_not_corrupted_by_sibling_start() {
    // Regression: the shallow env must be correctly restored after
    // resolving each start, so sibling starts see the caller's env.
    let suite = resolve_source_no_env(&[(
        "tests/a",
        r#"effect A {
  expect X
  shell a {
    > a
  }
}
effect B {
  expect Y
  shell b {
    > b
  }
}
test "t" {
  start A { X = "1" }
  start B { Y = "2" }
  shell sh {
    > echo ok
  }
}
"#,
    )]);
    assert!(is_runnable(&suite.plans[0]));
}
