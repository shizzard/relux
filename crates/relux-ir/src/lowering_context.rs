use std::sync::Arc;

use crate::AstTable;
use crate::IrEffect;
use crate::IrFn;
use crate::IrNodeLowering;
use crate::IrPureFn;
use crate::LocalEffectKey;
use crate::LocalFnKey;
use crate::LocalTable;
use crate::LocalTables;
use crate::Plan;
use crate::Suite;
use crate::Tables;
use relux_ast::AstItem;
use relux_ast::AstModule;
use relux_core::diagnostics::Cause;
use relux_core::diagnostics::CauseId;
use relux_core::diagnostics::CauseTable;
use relux_core::diagnostics::CycleReport;
use relux_core::diagnostics::DefinitionRef;
use relux_core::diagnostics::EffectCycleEntry;
use relux_core::diagnostics::EffectId;
use relux_core::diagnostics::EffectName;
use relux_core::diagnostics::FnCycleEntry;
use relux_core::diagnostics::FnId;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::ModulePath;
use relux_core::diagnostics::Warning;
use relux_core::diagnostics::WarningId;
use relux_core::diagnostics::WarningTable;
use relux_core::pure::LayeredEnv;
use relux_core::table::FileId;
use relux_core::table::SourceTable;

// ─── LoweringScope ──────────────────────────────────────────

/// Per-definition scope holding local name resolution tables.
/// Pushed onto the scope stack when entering a definition's body,
/// popped when leaving.
pub struct LoweringScope {
    pub module_path: ModulePath,
    pub tables: LocalTables,
}

// ─── LoweringContext ─────────────────────────────────────────

pub struct LoweringContext {
    ast_table: AstTable,
    env: Arc<LayeredEnv>,
    tables: Tables,
    causes: CauseTable,
    warnings: WarningTable,
    multiplier: f64,
    fn_stack: Vec<(FnId, IrSpan)>,
    effect_stack: Vec<(EffectId, IrSpan)>,
    scope_stack: Vec<LoweringScope>,
    shallow_env: Option<Arc<crate::shallow_env::ShallowLayeredEnv>>,
}

impl std::fmt::Debug for LoweringContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoweringContext").finish_non_exhaustive()
    }
}

impl LoweringContext {
    pub fn new(
        ast_table: AstTable,
        source_table: SourceTable,
        env: Arc<LayeredEnv>,
        causes: CauseTable,
        warnings: WarningTable,
        multiplier: f64,
    ) -> Self {
        Self {
            ast_table,
            env,
            tables: Tables {
                sources: source_table,
                fns: relux_core::table::SharedTable::new(),
                pure_fns: relux_core::table::SharedTable::new(),
                effects: relux_core::table::SharedTable::new(),
            },
            causes,
            warnings,
            multiplier,
            fn_stack: Vec::new(),
            effect_stack: Vec::new(),
            scope_stack: Vec::new(),
            shallow_env: None,
        }
    }

    // ─── Accessors ───────────────────────────────────────────

    pub fn ast_table(&self) -> &AstTable {
        &self.ast_table
    }

    pub fn env(&self) -> &Arc<LayeredEnv> {
        &self.env
    }

    pub fn tables(&self) -> &Tables {
        &self.tables
    }

    pub fn functions(&self) -> &crate::FnTable {
        &self.tables.fns
    }

    pub fn pure_functions(&self) -> &crate::PureFnTable {
        &self.tables.pure_fns
    }

    pub fn effects(&self) -> &crate::EffectTable {
        &self.tables.effects
    }

    pub fn causes(&self) -> &CauseTable {
        &self.causes
    }

    pub fn warnings(&self) -> &WarningTable {
        &self.warnings
    }

    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    pub fn fn_stack(&self) -> &[(FnId, IrSpan)] {
        &self.fn_stack
    }

    pub fn effect_stack(&self) -> &[(EffectId, IrSpan)] {
        &self.effect_stack
    }

    pub fn set_shallow_env(&mut self, env: Arc<crate::shallow_env::ShallowLayeredEnv>) {
        self.shallow_env = Some(env);
    }

    pub fn shallow_env(&self) -> Option<&Arc<crate::shallow_env::ShallowLayeredEnv>> {
        self.shallow_env.as_ref()
    }

    // ─── BIF Registration ────────────────────────────────────

    /// Pre-register all built-in functions under the synthetic `@builtin` module.
    pub fn register_bifs(&self) {
        let builtin_mod = ModulePath("@builtin".into());

        // Pure BIFs — registered in both FnTable and PureFnTable.
        let pure_bifs: &[(&str, usize)] = &[
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
            ("default", 2),
        ];

        for &(name, arity) in pure_bifs {
            let fn_id = FnId {
                module: builtin_mod.clone(),
                name: name.into(),
                arity,
            };
            self.tables.fns.insert(
                fn_id.clone(),
                Ok(IrFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
            self.tables.pure_fns.insert(
                fn_id,
                Ok(IrPureFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
        }

        // Impure BIFs — registered in FnTable only.
        let impure_bifs: &[(&str, usize)] = &[
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

        for &(name, arity) in impure_bifs {
            let fn_id = FnId {
                module: builtin_mod.clone(),
                name: name.into(),
                arity,
            };
            self.tables.fns.insert(
                fn_id,
                Ok(IrFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
        }
    }

    // ─── Local Table Factories ───────────────────────────────

    pub fn local_tables(&self) -> LocalTables {
        LocalTables {
            fns: LocalTable::new(self.tables.fns.clone()),
            pure_fns: LocalTable::new(self.tables.pure_fns.clone()),
            effects: LocalTable::new(self.tables.effects.clone()),
        }
    }

    // ─── Local Table Population ──────────────────────────────

    /// Populate local tables from a module's own definitions and imports.
    ///
    /// Fails fast on the first error (missing module, missing name, name conflict).
    /// The caller wraps the error in `LoweringBail::Invalid` for caching.
    pub fn populate_local_tables(
        &self,
        module_path: &ModulePath,
        file_id: &FileId,
        tables: &mut LocalTables,
    ) -> Result<(), InvalidReport> {
        let module = self
            .ast_table
            .get(module_path)
            .expect("module must be in ast_table");

        let ast_module = &module.1;

        // 1. Insert own definitions as identity mappings.
        self.insert_own_definitions(module_path, file_id, ast_module, tables)?;

        // 2. Walk import declarations.
        for item in &ast_module.items {
            if let AstItem::Import { import, .. } = &item.node {
                let import_mod_path = ModulePath(format!("lib/{}", import.path.node));
                let import_span = IrSpan::new(file_id.clone(), import.span);

                // Look up target module in AstTable.
                let Some(target_entry) = self.ast_table.get(&import_mod_path) else {
                    return Err(InvalidReport::undefined_module_import(
                        import_mod_path,
                        import_span,
                    ));
                };

                let target_file_id = &target_entry.0;
                let target_module = &target_entry.1;

                match &import.names {
                    None => {
                        // Wildcard import — import all definitions from target.
                        self.import_wildcard(
                            &import_mod_path,
                            target_file_id,
                            target_module,
                            &import_span,
                            tables,
                        )?;
                    }
                    Some(names) => {
                        // Selective import.
                        self.import_selective(
                            &import_mod_path,
                            target_file_id,
                            target_module,
                            names,
                            file_id,
                            tables,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    fn insert_own_definitions(
        &self,
        module_path: &ModulePath,
        file_id: &FileId,
        ast_module: &AstModule,
        tables: &mut LocalTables,
    ) -> Result<(), InvalidReport> {
        for item in &ast_module.items {
            match &item.node {
                AstItem::Fn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    let global_key = FnId {
                        module: module_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    let span = IrSpan::new(file_id.clone(), def.name.node.span);
                    tables.fns.insert(local_key, global_key, span);
                }
                AstItem::PureFn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    let global_key = FnId {
                        module: module_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    let span = IrSpan::new(file_id.clone(), def.name.node.span);
                    // Pure fns go in both tables — pure fns are callable
                    // from impure contexts too.
                    tables
                        .fns
                        .insert(local_key.clone(), global_key.clone(), span.clone());
                    tables.pure_fns.insert(local_key, global_key, span);
                }
                AstItem::Effect { def, .. } => {
                    let local_key = LocalEffectKey::new(EffectName(def.name.node.name.clone()));
                    let global_key = EffectId {
                        module: module_path.clone(),
                        name: EffectName(def.name.node.name.clone()),
                    };
                    let span = IrSpan::new(file_id.clone(), def.name.node.span);
                    tables.effects.insert(local_key, global_key, span);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn import_wildcard(
        &self,
        import_mod_path: &ModulePath,
        _target_file_id: &FileId,
        target_module: &AstModule,
        import_span: &IrSpan,
        tables: &mut LocalTables,
    ) -> Result<(), InvalidReport> {
        for item in &target_module.items {
            match &item.node {
                AstItem::Fn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    if tables.fns.contains_local(&local_key) {
                        return Err(InvalidReport::name_conflict(
                            format!("{}/{}", def.name.node.name, def.params.len()),
                            tables.fns.get_span(&local_key).unwrap().clone(),
                            import_span.clone(),
                        ));
                    }
                    let global_key = FnId {
                        module: import_mod_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    tables
                        .fns
                        .insert(local_key, global_key, import_span.clone());
                }
                AstItem::PureFn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    if tables.fns.contains_local(&local_key) {
                        return Err(InvalidReport::name_conflict(
                            format!("{}/{}", def.name.node.name, def.params.len()),
                            tables.fns.get_span(&local_key).unwrap().clone(),
                            import_span.clone(),
                        ));
                    }
                    let global_key = FnId {
                        module: import_mod_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    tables
                        .fns
                        .insert(local_key.clone(), global_key.clone(), import_span.clone());
                    tables
                        .pure_fns
                        .insert(local_key, global_key, import_span.clone());
                }
                AstItem::Effect { def, .. } => {
                    let local_key = LocalEffectKey::new(EffectName(def.name.node.name.clone()));
                    if tables.effects.contains_local(&local_key) {
                        return Err(InvalidReport::name_conflict(
                            def.name.node.name.clone(),
                            tables.effects.get_span(&local_key).unwrap().clone(),
                            import_span.clone(),
                        ));
                    }
                    let global_key = EffectId {
                        module: import_mod_path.clone(),
                        name: EffectName(def.name.node.name.clone()),
                    };
                    tables
                        .effects
                        .insert(local_key, global_key, import_span.clone());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn import_selective(
        &self,
        import_mod_path: &ModulePath,
        _target_file_id: &FileId,
        target_module: &AstModule,
        names: &[relux_core::Spanned<relux_ast::AstImportName>],
        source_file_id: &FileId,
        tables: &mut LocalTables,
    ) -> Result<(), InvalidReport> {
        for import_name in names {
            let original_name = &import_name.node.name.node.name;
            let local_name = import_name
                .node
                .alias
                .as_ref()
                .map(|a| &a.node.name)
                .unwrap_or(original_name);
            let name_span = IrSpan::new(source_file_id.clone(), import_name.node.name.node.span);

            // Determine if this is an effect (CamelCase) or function.
            let is_effect = original_name
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase());

            if is_effect {
                // Look for an effect definition in the target module.
                let found = target_module.items.iter().any(|item| {
                    matches!(&item.node, AstItem::Effect { def, .. }
                        if def.name.node.name == *original_name)
                });
                if !found {
                    return Err(InvalidReport::undefined_effect_import(
                        original_name.clone(),
                        import_mod_path.clone(),
                        name_span,
                    ));
                }

                let local_key = LocalEffectKey::new(EffectName(local_name.clone()));
                if tables.effects.contains_local(&local_key) {
                    return Err(InvalidReport::name_conflict(
                        local_name.clone(),
                        tables.effects.get_span(&local_key).unwrap().clone(),
                        name_span,
                    ));
                }
                let global_key = EffectId {
                    module: import_mod_path.clone(),
                    name: EffectName(original_name.clone()),
                };
                tables
                    .effects
                    .insert(local_key, global_key, name_span.clone());
            } else {
                // Look for fn or pure fn definitions in the target module.
                // Collect all matching definitions (there may be multiple arities).
                let mut found_any = false;

                for item in &target_module.items {
                    match &item.node {
                        AstItem::Fn { def, .. } if def.name.node.name == *original_name => {
                            found_any = true;
                            let local_key = LocalFnKey::new(local_name, def.params.len());
                            if tables.fns.contains_local(&local_key) {
                                return Err(InvalidReport::name_conflict(
                                    format!("{}/{}", local_name, def.params.len()),
                                    tables.fns.get_span(&local_key).unwrap().clone(),
                                    name_span,
                                ));
                            }
                            let global_key = FnId {
                                module: import_mod_path.clone(),
                                name: original_name.clone(),
                                arity: def.params.len(),
                            };
                            tables.fns.insert(local_key, global_key, name_span.clone());
                        }
                        AstItem::PureFn { def, .. } if def.name.node.name == *original_name => {
                            found_any = true;
                            let local_key = LocalFnKey::new(local_name, def.params.len());
                            if tables.fns.contains_local(&local_key) {
                                return Err(InvalidReport::name_conflict(
                                    format!("{}/{}", local_name, def.params.len()),
                                    tables.fns.get_span(&local_key).unwrap().clone(),
                                    name_span,
                                ));
                            }
                            let global_key = FnId {
                                module: import_mod_path.clone(),
                                name: original_name.clone(),
                                arity: def.params.len(),
                            };
                            tables.fns.insert(
                                local_key.clone(),
                                global_key.clone(),
                                name_span.clone(),
                            );
                            tables
                                .pure_fns
                                .insert(local_key, global_key, name_span.clone());
                        }
                        _ => {}
                    }
                }

                if !found_any {
                    return Err(InvalidReport::undefined_function_import(
                        original_name.clone(),
                        import_mod_path.clone(),
                        name_span,
                    ));
                }
            }
        }
        Ok(())
    }

    // ─── Cause / Warning Registration ────────────────────────

    pub fn register_cause(&self, cause_id: CauseId, cause: relux_core::diagnostics::Cause) {
        self.causes.insert(cause_id, cause);
    }

    pub fn register_warning(&self, warning_id: WarningId, warning: Warning) {
        self.warnings.insert(warning_id, warning);
    }

    // ─── In-Progress Stack: Functions ────────────────────────

    pub fn push_fn(&mut self, id: FnId, span: IrSpan) {
        self.fn_stack.push((id, span));
    }

    pub fn pop_fn(&mut self) {
        self.fn_stack.pop();
    }

    pub fn find_fn_cycle(&self, id: &FnId) -> Option<CycleReport> {
        let pos = self.fn_stack.iter().position(|(fid, _)| fid == id)?;
        let chain = self.fn_stack[pos..]
            .iter()
            .map(|(fid, span)| FnCycleEntry {
                id: fid.clone(),
                call_span: span.clone(),
            })
            .collect();
        Some(CycleReport::Function { chain })
    }

    // ─── In-Progress Stack: Effects ──────────────────────────

    pub fn push_effect(&mut self, id: EffectId, span: IrSpan) {
        self.effect_stack.push((id, span));
    }

    pub fn pop_effect(&mut self) {
        self.effect_stack.pop();
    }

    pub fn find_effect_cycle(&self, id: &EffectId) -> Option<CycleReport> {
        let pos = self.effect_stack.iter().position(|(eid, _)| eid == id)?;
        let chain = self.effect_stack[pos..]
            .iter()
            .map(|(eid, span)| EffectCycleEntry {
                id: eid.clone(),
                start_span: span.clone(),
            })
            .collect();
        Some(CycleReport::Effect { chain })
    }

    // ─── Finalization ────────────────────────────────────────

    /// Print all diagnostics (causes and warnings) to stderr.
    ///
    /// Each cause is printed once with its mnemonic ID. At runtime, tests
    /// reference causes by ID rather than repeating the full diagnostic.
    pub fn print_diagnostics(&self, project_root: Option<&std::path::Path>) {
        use relux_core::diagnostics::Diagnostic;

        for (warning_id, warning) in self.warnings.as_vec() {
            let diagnostic = Diagnostic::from(warning);
            diagnostic.eprint_with_id(&warning_id, &self.tables.sources, project_root);
        }
        for (cause_id, cause) in self.causes.as_vec() {
            let diagnostic = Diagnostic::from(cause);
            diagnostic.eprint_with_id(&cause_id, &self.tables.sources, project_root);
        }
    }

    /// Consume the context and produce a Suite.
    pub fn into_suite(self, plans: Vec<Plan>) -> Suite {
        Suite {
            plans,
            env: self.env,
            causes: self.causes,
            warnings: self.warnings,
            tables: self.tables,
        }
    }

    // ─── Scope Stack ─────────────────────────────────────────

    pub fn push_scope(&mut self, scope: LoweringScope) {
        self.scope_stack.push(scope);
    }

    pub fn pop_scope(&mut self) -> LoweringScope {
        self.scope_stack.pop().expect("scope stack underflow")
    }

    pub fn current_scope(&self) -> &LoweringScope {
        self.scope_stack.last().expect("no current scope")
    }

    // ─── Resolve: Functions ──────────────────────────────────

    /// Resolve a function by its global FnId.
    /// Handles caching, cycle detection, local table creation, and lowering.
    pub fn resolve_fn(&mut self, fn_id: &FnId) -> Result<IrFn, LoweringBail> {
        // Check cache
        if let Some(result) = self.tables.fns.get(fn_id) {
            return result.clone();
        }

        // Check cycle
        if let Some(cycle) = self.find_fn_cycle(fn_id) {
            let bail = LoweringBail::invalid(InvalidReport::cycle(cycle));
            self.tables.fns.insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Find AST definition (clone ast_table Arc to avoid borrowing self)
        let ast_table = self.ast_table.clone();
        let entry = ast_table
            .get(&fn_id.module)
            .expect("module must be in ast_table");
        let file_id = entry.0.clone();
        let def = entry
            .1
            .items
            .iter()
            .find_map(|item| match &item.node {
                AstItem::Fn { def, .. }
                    if def.name.node.name == fn_id.name && def.params.len() == fn_id.arity =>
                {
                    Some(def)
                }
                _ => None,
            })
            .expect("fn must be in module");

        // Create and populate local tables
        let mut tables = self.local_tables();
        if let Err(e) = self.populate_local_tables(&fn_id.module, &file_id, &mut tables) {
            let bail = LoweringBail::invalid(e);
            self.tables.fns.insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_fn(fn_id.clone(), span);

        // Push scope
        self.push_scope(LoweringScope {
            module_path: fn_id.module.clone(),
            tables,
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Fn(fn_id.clone());
        let marker_recordings =
            match crate::marker::eval_marker(&def.markers, definition, &env, &file_id, self) {
                Ok(result) => {
                    if let Some(skip) = result.skip {
                        let bail = LoweringBail::skip(skip);
                        let cause_id = bail.cause_id();
                        self.register_cause(cause_id, Cause::from_bail(&bail));
                        self.pop_scope();
                        self.pop_fn();
                        self.tables.fns.insert(fn_id.clone(), Err(bail.clone()));
                        return Err(bail);
                    }
                    // Flaky on fns is ignored (only meaningful on tests)
                    result.recordings
                }
                Err(bail) => {
                    let cause_id = bail.cause_id();
                    self.register_cause(cause_id, Cause::from_bail(&bail));
                    self.pop_scope();
                    self.pop_fn();
                    self.tables.fns.insert(fn_id.clone(), Err(bail.clone()));
                    return Err(bail);
                }
            };

        // Lower body and attach the fn's marker recordings so the
        // runtime can replay them under the FnCall span at call time.
        let result = IrFn::lower(def, &file_id, self).map(|ir_fn| match ir_fn {
            IrFn::UserDefined {
                name,
                params,
                body,
                span,
                ..
            } => IrFn::UserDefined {
                name,
                params,
                body,
                marker_recordings,
                span,
            },
            other => other,
        });

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_fn();

        // Cache and return
        self.tables.fns.insert(fn_id.clone(), result.clone());
        result
    }

    // ─── Resolve: Pure Functions ─────────────────────────────

    pub fn resolve_pure_fn(&mut self, fn_id: &FnId) -> Result<IrPureFn, LoweringBail> {
        // Check cache
        if let Some(result) = self.tables.pure_fns.get(fn_id) {
            return result.clone();
        }

        // Check cycle
        if let Some(cycle) = self.find_fn_cycle(fn_id) {
            let bail = LoweringBail::invalid(InvalidReport::cycle(cycle));
            self.tables
                .pure_fns
                .insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Find AST definition
        let ast_table = self.ast_table.clone();
        let entry = ast_table
            .get(&fn_id.module)
            .expect("module must be in ast_table");
        let file_id = entry.0.clone();
        let def = entry
            .1
            .items
            .iter()
            .find_map(|item| match &item.node {
                AstItem::PureFn { def, .. }
                    if def.name.node.name == fn_id.name && def.params.len() == fn_id.arity =>
                {
                    Some(def)
                }
                _ => None,
            })
            .expect("pure fn must be in module");

        // Create and populate local tables
        let mut tables = self.local_tables();
        if let Err(e) = self.populate_local_tables(&fn_id.module, &file_id, &mut tables) {
            let bail = LoweringBail::invalid(e);
            self.tables
                .pure_fns
                .insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress (pure fns share fn_stack)
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_fn(fn_id.clone(), span);

        // Push scope
        self.push_scope(LoweringScope {
            module_path: fn_id.module.clone(),
            tables,
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Fn(fn_id.clone());
        let marker_recordings =
            match crate::marker::eval_marker(&def.markers, definition, &env, &file_id, self) {
                Ok(result) => {
                    if let Some(skip) = result.skip {
                        let bail = LoweringBail::skip(skip);
                        let cause_id = bail.cause_id();
                        self.register_cause(cause_id, Cause::from_bail(&bail));
                        self.pop_scope();
                        self.pop_fn();
                        self.tables
                            .pure_fns
                            .insert(fn_id.clone(), Err(bail.clone()));
                        return Err(bail);
                    }
                    result.recordings
                }
                Err(bail) => {
                    let cause_id = bail.cause_id();
                    self.register_cause(cause_id, Cause::from_bail(&bail));
                    self.pop_scope();
                    self.pop_fn();
                    self.tables
                        .pure_fns
                        .insert(fn_id.clone(), Err(bail.clone()));
                    return Err(bail);
                }
            };

        // Lower body and attach marker recordings to the pure fn so
        // the runtime can replay them under the FnCall span when the
        // function is invoked.
        let result = IrPureFn::lower(def, &file_id, self).map(|ir_fn| match ir_fn {
            IrPureFn::UserDefined {
                name,
                params,
                body,
                span,
                ..
            } => IrPureFn::UserDefined {
                name,
                params,
                body,
                marker_recordings,
                span,
            },
            other => other,
        });

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_fn();

        // Cache and return
        self.tables.pure_fns.insert(fn_id.clone(), result.clone());
        result
    }

    // ─── Resolve: Effects ────────────────────────────────────

    pub fn resolve_effect(&mut self, effect_id: &EffectId) -> Result<IrEffect, LoweringBail> {
        // Check cache
        if let Some(result) = self.tables.effects.get(effect_id) {
            return result.clone();
        }

        // Check cycle
        if let Some(cycle) = self.find_effect_cycle(effect_id) {
            let bail = LoweringBail::invalid(InvalidReport::cycle(cycle));
            self.tables
                .effects
                .insert(effect_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Find AST definition
        let ast_table = self.ast_table.clone();
        let entry = ast_table
            .get(&effect_id.module)
            .expect("module must be in ast_table");
        let file_id = entry.0.clone();
        let def = entry
            .1
            .items
            .iter()
            .find_map(|item| match &item.node {
                AstItem::Effect { def, .. } if def.name.node.name == effect_id.name.0 => Some(def),
                _ => None,
            })
            .expect("effect must be in module");

        // Create and populate local tables
        let mut tables = self.local_tables();
        if let Err(e) = self.populate_local_tables(&effect_id.module, &file_id, &mut tables) {
            let bail = LoweringBail::invalid(e);
            self.tables
                .effects
                .insert(effect_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_effect(effect_id.clone(), span);

        // Push scope
        self.push_scope(LoweringScope {
            module_path: effect_id.module.clone(),
            tables,
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Effect(effect_id.clone());
        let marker_recordings =
            match crate::marker::eval_marker(&def.markers, definition, &env, &file_id, self) {
                Ok(result) => {
                    if let Some(skip) = result.skip {
                        let bail = LoweringBail::skip(skip);
                        let cause_id = bail.cause_id();
                        self.register_cause(cause_id, Cause::from_bail(&bail));
                        self.pop_scope();
                        self.pop_effect();
                        self.tables
                            .effects
                            .insert(effect_id.clone(), Err(bail.clone()));
                        return Err(bail);
                    }
                    result.recordings
                }
                Err(bail) => {
                    let cause_id = bail.cause_id();
                    self.register_cause(cause_id, Cause::from_bail(&bail));
                    self.pop_scope();
                    self.pop_effect();
                    self.tables
                        .effects
                        .insert(effect_id.clone(), Err(bail.clone()));
                    return Err(bail);
                }
            };

        // Lower body
        let result = IrEffect::lower(def, &file_id, self).map(|mut ir_effect| {
            ir_effect.set_marker_recordings(marker_recordings);
            ir_effect
        });

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_effect();

        // Cache and return
        self.tables
            .effects
            .insert(effect_id.clone(), result.clone());
        result
    }
}
