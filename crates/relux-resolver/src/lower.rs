// Re-export LoweringContext and LoweringScope from relux-ir for convenience.
pub use relux_ir::lowering_context::LoweringContext;
pub use relux_ir::lowering_context::LoweringScope;

pub mod test_helpers {
    use relux_ast::*;
    use relux_core::Span;
    use relux_core::diagnostics::CauseTable;
    use relux_core::diagnostics::IrSpan;
    use relux_core::diagnostics::LoweringBail;
    use relux_core::diagnostics::ModulePath;
    use relux_core::diagnostics::WarningTable;
    use relux_core::pure::Env;
    use relux_core::pure::LayeredEnv;
    use relux_core::table::FileId;
    use relux_core::table::SharedTable;
    use relux_core::table::SourceTable;
    use relux_ir::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    pub fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("/test/file.relux"))
    }

    pub fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), Span::new(0, 10))
    }

    pub fn test_env() -> Arc<LayeredEnv> {
        Arc::new(LayeredEnv::from(Env::from_map(HashMap::new())))
    }

    pub fn empty_ast_table() -> AstTable {
        AstTable::new()
    }

    pub fn empty_source_table() -> SourceTable {
        SourceTable::new()
    }

    pub fn parse_module(source: &str) -> AstModule {
        relux_parser::parse(source).unwrap_or_else(|e| panic!("parse error: {e:?}"))
    }

    pub fn ctx_with_source(source: &str) -> super::LoweringContext {
        ctx_with_modules(vec![("tests/a", "/test/a.relux", source)])
    }

    pub fn ctx_with_modules(modules: Vec<(&str, &str, &str)>) -> super::LoweringContext {
        let ast_table: AstTable = SharedTable::new();
        for (mod_path, file_path, source) in &modules {
            let ast = parse_module(source);
            ast_table.insert(
                ModulePath((*mod_path).into()),
                (FileId::new(PathBuf::from(file_path)), ast),
            );
        }
        let ctx = super::LoweringContext::new(
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

    pub fn push_test_scope(ctx: &mut super::LoweringContext, mod_path: &str) {
        let module_path = ModulePath(mod_path.into());
        let ast_table = ctx.ast_table().clone();
        let file_id = ast_table
            .get(&module_path)
            .unwrap_or_else(|| panic!("module {mod_path} not in ast_table"))
            .0
            .clone();
        let mut tables = ctx.local_tables();
        ctx.populate_local_tables(&module_path, &file_id, &mut tables)
            .unwrap();
        ctx.push_scope(super::LoweringScope {
            module_path,
            tables,
        });
    }

    pub fn file_id_for(ctx: &super::LoweringContext, mod_path: &str) -> FileId {
        ctx.ast_table()
            .get(&ModulePath(mod_path.into()))
            .unwrap()
            .0
            .clone()
    }

    pub fn extract_first_stmt(source: &str) -> AstStmt {
        let module = parse_module(source);
        if let AstItem::Fn { def, .. } = &module.items[0].node {
            def.body[0].node.clone()
        } else {
            panic!("expected fn");
        }
    }

    pub fn extract_let_expr(source: &str) -> AstExpr {
        let module = parse_module(source);
        if let AstItem::Fn { def, .. } = &module.items[0].node {
            if let AstStmt::Let { stmt, .. } = &def.body[0].node {
                stmt.value.as_ref().unwrap().node.clone()
            } else {
                panic!("expected let stmt");
            }
        } else {
            panic!("expected fn");
        }
    }

    pub fn lower_first_test(
        ctx: &mut super::LoweringContext,
        mod_path_str: &str,
    ) -> Result<IrTest, LoweringBail> {
        let mod_path = ModulePath(mod_path_str.into());
        let file = file_id_for(ctx, mod_path_str);
        let ast_table = ctx.ast_table().clone();
        let entry = ast_table.get(&mod_path).unwrap();
        let def = entry
            .1
            .items
            .iter()
            .find_map(|item| match &item.node {
                AstItem::Test { def, .. } => Some(def),
                _ => None,
            })
            .unwrap();

        let mut tables = ctx.local_tables();
        ctx.populate_local_tables(&mod_path, &file, &mut tables)
            .map_err(LoweringBail::invalid)?;
        ctx.push_scope(super::LoweringScope {
            module_path: mod_path.clone(),
            tables,
        });
        let result = IrTest::lower(def, &file, ctx);
        ctx.pop_scope();
        result
    }

    pub fn resolve_source(sources: &[(&str, &str)], env: HashMap<String, String>) -> Suite {
        use crate::loader::InMemoryLoader;

        let mut loader = InMemoryLoader::new();
        let mut seeds = Vec::new();
        for (mod_path, source) in sources {
            loader.add(mod_path, source);
            seeds.push(ModulePath((*mod_path).into()));
        }

        let causes: CauseTable = SharedTable::new();
        let warnings: WarningTable = SharedTable::new();
        let (ast_table, source_table) = crate::load_modules(&loader, seeds, &causes, &warnings);
        let mut ctx = super::LoweringContext::new(
            ast_table,
            source_table,
            Arc::new(LayeredEnv::from(Env::from_map(env))),
            causes,
            warnings,
            1.0,
        );
        ctx.register_bifs();
        let plans = build_all_plans(&mut ctx);
        ctx.into_suite("test-suite-name-placeholder".to_string(), plans)
    }

    pub fn resolve_source_with_multiplier(sources: &[(&str, &str)], multiplier: f64) -> Suite {
        use crate::loader::InMemoryLoader;

        let mut loader = InMemoryLoader::new();
        let mut seeds = Vec::new();
        for (mod_path, source) in sources {
            loader.add(mod_path, source);
            seeds.push(ModulePath((*mod_path).into()));
        }

        let causes: CauseTable = SharedTable::new();
        let warnings: WarningTable = SharedTable::new();
        let (ast_table, source_table) = crate::load_modules(&loader, seeds, &causes, &warnings);
        let mut ctx = super::LoweringContext::new(
            ast_table,
            source_table,
            Arc::new(LayeredEnv::from(Env::from_map(HashMap::new()))),
            causes,
            warnings,
            multiplier,
        );
        ctx.register_bifs();
        let plans = build_all_plans(&mut ctx);
        ctx.into_suite("test-suite-name-placeholder".to_string(), plans)
    }

    pub fn resolve_source_no_env(sources: &[(&str, &str)]) -> Suite {
        resolve_source(sources, HashMap::new())
    }

    pub fn plan_name(plan: &Plan) -> &str {
        plan.meta().name()
    }

    pub fn is_runnable(plan: &Plan) -> bool {
        matches!(plan, Plan::Runnable { .. })
    }

    pub fn is_skipped(plan: &Plan) -> bool {
        matches!(plan, Plan::Skipped { .. })
    }

    pub fn is_invalid(plan: &Plan) -> bool {
        matches!(plan, Plan::Invalid { .. })
    }

    pub fn is_flaky(plan: &Plan) -> bool {
        plan.meta().flaky()
    }
}
