use std::sync::Arc;

use crate::diagnostics::{
    Cause, CauseId, CauseTable, CycleReport, DefinitionRef, EffectCycleEntry, EffectId, EffectName,
    FnCycleEntry, FnId, InvalidReport, IrSpan, LoweringBail, ModulePath, Warning, WarningId,
    WarningTable,
};
use crate::dsl::parser::ast::{AstItem, AstModule};
use crate::dsl::resolver::ir::{
    AstTable, EffectTable, FnTable, IrEffect, IrFn, IrNodeLowering, IrPureFn, LocalEffectKey,
    LocalFnKey, NewPlan, PureFnTable, SourceTable, Suite,
};
use crate::stack::Env;
use crate::table::{FileId, LocalTable, SharedTable};

// ─── Type aliases for local tables ───────────────────────────

pub type LocalFnTable = LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>;
pub type LocalPureFnTable = LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>>;
pub type LocalEffectTable = LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>>;

// ─── LoweringScope ──────────────────────────────────────────

/// Per-definition scope holding local name resolution tables.
/// Pushed onto the scope stack when entering a definition's body,
/// popped when leaving.
pub struct LoweringScope {
    pub module_path: ModulePath,
    pub fn_table: LocalFnTable,
    pub pure_fn_table: LocalPureFnTable,
    pub effect_table: Option<LocalEffectTable>,
}

// ─── LoweringContext ─────────────────────────────────────────

pub struct LoweringContext {
    ast_table: AstTable,
    source_map: SourceTable,
    env: Arc<Env>,
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
    causes: CauseTable,
    warnings: WarningTable,
    fn_stack: Vec<(FnId, IrSpan)>,
    effect_stack: Vec<(EffectId, IrSpan)>,
    scope_stack: Vec<LoweringScope>,
}

impl std::fmt::Debug for LoweringContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoweringContext").finish_non_exhaustive()
    }
}

impl LoweringContext {
    pub fn new(
        ast_table: AstTable,
        source_map: SourceTable,
        env: Arc<Env>,
        causes: CauseTable,
        warnings: WarningTable,
    ) -> Self {
        Self {
            ast_table,
            source_map,
            env,
            functions: SharedTable::new(),
            pure_functions: SharedTable::new(),
            effects: SharedTable::new(),
            causes,
            warnings,
            fn_stack: Vec::new(),
            effect_stack: Vec::new(),
            scope_stack: Vec::new(),
        }
    }

    // ─── Accessors ───────────────────────────────────────────

    pub fn ast_table(&self) -> &AstTable {
        &self.ast_table
    }

    pub fn env(&self) -> &Arc<Env> {
        &self.env
    }

    pub fn functions(&self) -> &FnTable {
        &self.functions
    }

    pub fn pure_functions(&self) -> &PureFnTable {
        &self.pure_functions
    }

    pub fn effects(&self) -> &EffectTable {
        &self.effects
    }

    pub fn causes(&self) -> &CauseTable {
        &self.causes
    }

    pub fn warnings(&self) -> &WarningTable {
        &self.warnings
    }

    pub fn fn_stack(&self) -> &[(FnId, IrSpan)] {
        &self.fn_stack
    }

    pub fn effect_stack(&self) -> &[(EffectId, IrSpan)] {
        &self.effect_stack
    }

    // ─── BIF Registration ────────────────────────────────────

    /// Pre-register all built-in functions under the synthetic `@builtin` module.
    pub fn register_bifs(&self) {
        let builtin_mod = ModulePath("@builtin".into());

        // Pure BIFs — registered in both FnTable and PureFnTable.
        let pure_bifs: &[(&str, usize)] = &[
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
        ];

        for &(name, arity) in pure_bifs {
            let fn_id = FnId {
                module: builtin_mod.clone(),
                name: name.into(),
                arity,
            };
            self.functions.insert(
                fn_id.clone(),
                Ok(IrFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
            self.pure_functions.insert(
                fn_id,
                Ok(IrPureFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
        }

        // Impure BIFs — registered in FnTable only.
        let impure_bifs: &[(&str, usize)] = &[
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
            self.functions.insert(
                fn_id,
                Ok(IrFn::Builtin {
                    name: name.into(),
                    arity,
                }),
            );
        }
    }

    // ─── Local Table Factories ───────────────────────────────

    pub fn local_fn_table(&self) -> LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>> {
        LocalTable::new(self.functions.clone())
    }

    pub fn local_pure_fn_table(
        &self,
    ) -> LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>> {
        LocalTable::new(self.pure_functions.clone())
    }

    pub fn local_effect_table(
        &self,
    ) -> LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>> {
        LocalTable::new(self.effects.clone())
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
        fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>,
        pure_fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>>,
        mut effect_table: Option<
            &mut LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>>,
        >,
    ) -> Result<(), InvalidReport> {
        let module = self
            .ast_table
            .get(module_path)
            .expect("module must be in ast_table");

        let ast_module = &module.1;

        // 1. Insert own definitions as identity mappings.
        self.insert_own_definitions(
            module_path,
            file_id,
            ast_module,
            fn_table,
            pure_fn_table,
            effect_table.as_deref_mut(),
        )?;

        // 2. Walk import declarations.
        for item in &ast_module.items {
            if let AstItem::Import { import, .. } = &item.node {
                let import_mod_path = ModulePath(format!("lib/{}", import.path.node));
                let import_span = IrSpan::new(file_id.clone(), import.span);

                // Look up target module in AstTable.
                let Some(target_entry) = self.ast_table.get(&import_mod_path) else {
                    return Err(InvalidReport::UndefinedModuleImport {
                        module_path: import_mod_path,
                        span: import_span,
                    });
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
                            fn_table,
                            pure_fn_table,
                            effect_table.as_deref_mut(),
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
                            fn_table,
                            pure_fn_table,
                            effect_table.as_deref_mut(),
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
        _file_id: &FileId,
        ast_module: &AstModule,
        fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>,
        pure_fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>>,
        mut effect_table: Option<
            &mut LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>>,
        >,
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
                    fn_table.insert(local_key, global_key);
                }
                AstItem::PureFn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    let global_key = FnId {
                        module: module_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    // Pure fns go in both tables — pure fns are callable
                    // from impure contexts too.
                    fn_table.insert(local_key.clone(), global_key.clone());
                    pure_fn_table.insert(local_key, global_key);
                }
                AstItem::Effect { def, .. } => {
                    if let Some(et) = effect_table.as_deref_mut() {
                        let local_key = LocalEffectKey::new(EffectName(def.name.node.name.clone()));
                        let global_key = EffectId {
                            module: module_path.clone(),
                            name: EffectName(def.name.node.name.clone()),
                        };
                        et.insert(local_key, global_key);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // TODO: bundle tables into a struct to reduce argument count
    #[allow(clippy::too_many_arguments)]
    fn import_wildcard(
        &self,
        import_mod_path: &ModulePath,
        _target_file_id: &FileId,
        target_module: &AstModule,
        import_span: &IrSpan,
        fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>,
        pure_fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>>,
        mut effect_table: Option<
            &mut LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>>,
        >,
    ) -> Result<(), InvalidReport> {
        for item in &target_module.items {
            match &item.node {
                AstItem::Fn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    if fn_table.contains_local(&local_key) {
                        return Err(InvalidReport::NameConflict {
                            name: format!("{}/{}", def.name.node.name, def.params.len()),
                            first: self.span_of_existing_fn(&local_key, fn_table),
                            second: import_span.clone(),
                        });
                    }
                    let global_key = FnId {
                        module: import_mod_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    fn_table.insert(local_key, global_key);
                }
                AstItem::PureFn { def, .. } => {
                    let local_key = LocalFnKey::new(&def.name.node.name, def.params.len());
                    if fn_table.contains_local(&local_key) {
                        return Err(InvalidReport::NameConflict {
                            name: format!("{}/{}", def.name.node.name, def.params.len()),
                            first: self.span_of_existing_fn(&local_key, fn_table),
                            second: import_span.clone(),
                        });
                    }
                    let global_key = FnId {
                        module: import_mod_path.clone(),
                        name: def.name.node.name.clone(),
                        arity: def.params.len(),
                    };
                    fn_table.insert(local_key.clone(), global_key.clone());
                    pure_fn_table.insert(local_key, global_key);
                }
                AstItem::Effect { def, .. } => {
                    if let Some(et) = effect_table.as_deref_mut() {
                        let local_key = LocalEffectKey::new(EffectName(def.name.node.name.clone()));
                        if et.contains_local(&local_key) {
                            return Err(InvalidReport::NameConflict {
                                name: def.name.node.name.clone(),
                                first: import_span.clone(),
                                second: import_span.clone(),
                            });
                        }
                        let global_key = EffectId {
                            module: import_mod_path.clone(),
                            name: EffectName(def.name.node.name.clone()),
                        };
                        et.insert(local_key, global_key);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // TODO: bundle tables into a struct to reduce argument count
    #[allow(clippy::too_many_arguments)]
    fn import_selective(
        &self,
        import_mod_path: &ModulePath,
        _target_file_id: &FileId,
        target_module: &AstModule,
        names: &[crate::Spanned<crate::dsl::parser::ast::AstImportName>],
        source_file_id: &FileId,
        fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>,
        pure_fn_table: &mut LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>>,
        mut effect_table: Option<
            &mut LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>>,
        >,
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
                    return Err(InvalidReport::UndefinedEffectImport {
                        name: original_name.clone(),
                        module_path: import_mod_path.clone(),
                        span: name_span,
                    });
                }

                if let Some(et) = effect_table.as_deref_mut() {
                    let local_key = LocalEffectKey::new(EffectName(local_name.clone()));
                    if et.contains_local(&local_key) {
                        return Err(InvalidReport::NameConflict {
                            name: local_name.clone(),
                            first: name_span.clone(),
                            second: name_span,
                        });
                    }
                    let global_key = EffectId {
                        module: import_mod_path.clone(),
                        name: EffectName(original_name.clone()),
                    };
                    et.insert(local_key, global_key);
                }
            } else {
                // Look for fn or pure fn definitions in the target module.
                // Collect all matching definitions (there may be multiple arities).
                let mut found_any = false;

                for item in &target_module.items {
                    match &item.node {
                        AstItem::Fn { def, .. } if def.name.node.name == *original_name => {
                            found_any = true;
                            let local_key = LocalFnKey::new(local_name, def.params.len());
                            if fn_table.contains_local(&local_key) {
                                return Err(InvalidReport::NameConflict {
                                    name: format!("{}/{}", local_name, def.params.len()),
                                    first: name_span.clone(),
                                    second: name_span,
                                });
                            }
                            let global_key = FnId {
                                module: import_mod_path.clone(),
                                name: original_name.clone(),
                                arity: def.params.len(),
                            };
                            fn_table.insert(local_key, global_key);
                        }
                        AstItem::PureFn { def, .. } if def.name.node.name == *original_name => {
                            found_any = true;
                            let local_key = LocalFnKey::new(local_name, def.params.len());
                            if fn_table.contains_local(&local_key) {
                                return Err(InvalidReport::NameConflict {
                                    name: format!("{}/{}", local_name, def.params.len()),
                                    first: name_span.clone(),
                                    second: name_span,
                                });
                            }
                            let global_key = FnId {
                                module: import_mod_path.clone(),
                                name: original_name.clone(),
                                arity: def.params.len(),
                            };
                            fn_table.insert(local_key.clone(), global_key.clone());
                            pure_fn_table.insert(local_key, global_key);
                        }
                        _ => {}
                    }
                }

                if !found_any {
                    return Err(InvalidReport::UndefinedFunctionImport {
                        name: original_name.clone(),
                        module_path: import_mod_path.clone(),
                        span: name_span,
                    });
                }
            }
        }
        Ok(())
    }

    /// Helper: get the IrSpan for an existing fn entry. Used for NameConflict first span.
    /// Since we don't store spans in LocalTable, we use a synthetic span for now.
    fn span_of_existing_fn(
        &self,
        _local_key: &LocalFnKey,
        _fn_table: &LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>>,
    ) -> IrSpan {
        // TODO(R004): Proper span tracking for conflict reporting requires
        // storing spans in LocalTable. For now return a zero-span placeholder.
        // The second span (from the import) is always accurate.
        IrSpan::new(
            FileId::new(std::path::PathBuf::from("<conflict>")),
            crate::Span::new(0, 0),
        )
    }

    // ─── Cause / Warning Registration ────────────────────────

    pub fn register_cause(&self, cause_id: CauseId, cause: crate::diagnostics::Cause) {
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
                need_span: span.clone(),
            })
            .collect();
        Some(CycleReport::Effect { chain })
    }

    // ─── Finalization ────────────────────────────────────────

    /// Print all diagnostics (invalid causes only — skips are reported at runtime) to stderr.
    pub fn print_diagnostics(&self) {
        use crate::diagnostics::Diagnostic;

        for (_cause_id, cause) in self.causes.iter() {
            if matches!(&cause, Cause::Invalid(_)) {
                let diagnostic = Diagnostic::from(&cause);
                diagnostic.eprint(&self.source_map);
            }
        }
    }

    /// Consume the context and produce a Suite.
    pub fn into_suite(self, plans: Vec<NewPlan>) -> Suite {
        Suite {
            plans,
            source_map: self.source_map,
            env: self.env,
            causes: self.causes,
            warnings: self.warnings,
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
    // TODO: box LoweringBail to reduce Result size
    #[allow(clippy::result_large_err)]
    pub fn resolve_fn(&mut self, fn_id: &FnId) -> Result<IrFn, LoweringBail> {
        // Check cache
        if let Some(result) = self.functions.get(fn_id) {
            return result;
        }

        // Check cycle
        if let Some(cycle) = self.find_fn_cycle(fn_id) {
            let bail = LoweringBail::Invalid(InvalidReport::Cycle(cycle));
            self.functions.insert(fn_id.clone(), Err(bail.clone()));
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
        let mut fn_table = self.local_fn_table();
        let mut pure_fn_table = self.local_pure_fn_table();
        if let Err(e) = self.populate_local_tables(
            &fn_id.module,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        ) {
            let bail = LoweringBail::Invalid(e);
            self.functions.insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_fn(fn_id.clone(), span);

        // Push scope
        self.push_scope(LoweringScope {
            module_path: fn_id.module.clone(),
            fn_table,
            pure_fn_table,
            effect_table: None,
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Fn(fn_id.clone());
        match crate::dsl::resolver::ir::marker::eval_marker(
            &def.markers,
            definition,
            &env,
            &file_id,
            self,
        ) {
            Ok(Some(skip)) => {
                let cause_id = skip.cause_id();
                self.register_cause(cause_id, Cause::Skip(skip.clone()));
                let bail = LoweringBail::Skip(skip);
                self.pop_scope();
                self.pop_fn();
                self.functions.insert(fn_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
            Ok(None) => {}
            Err(bail) => {
                let cause_id = bail.cause_id();
                self.register_cause(cause_id, Cause::from_bail(&bail));
                self.pop_scope();
                self.pop_fn();
                self.functions.insert(fn_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
        }

        // Lower body
        let result = IrFn::lower(def, &file_id, self);

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_fn();

        // Cache and return
        self.functions.insert(fn_id.clone(), result.clone());
        result
    }

    // ─── Resolve: Pure Functions ─────────────────────────────

    // TODO: box LoweringBail to reduce Result size
    #[allow(clippy::result_large_err)]
    pub fn resolve_pure_fn(&mut self, fn_id: &FnId) -> Result<IrPureFn, LoweringBail> {
        // Check cache
        if let Some(result) = self.pure_functions.get(fn_id) {
            return result;
        }

        // Check cycle
        if let Some(cycle) = self.find_fn_cycle(fn_id) {
            let bail = LoweringBail::Invalid(InvalidReport::Cycle(cycle));
            self.pure_functions.insert(fn_id.clone(), Err(bail.clone()));
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

        // Create and populate local tables (pure fn only gets pure fn table)
        let mut fn_table = self.local_fn_table();
        let mut pure_fn_table = self.local_pure_fn_table();
        if let Err(e) = self.populate_local_tables(
            &fn_id.module,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        ) {
            let bail = LoweringBail::Invalid(e);
            self.pure_functions.insert(fn_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress (pure fns share fn_stack)
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_fn(fn_id.clone(), span);

        // Push scope (no effect table for pure fns)
        self.push_scope(LoweringScope {
            module_path: fn_id.module.clone(),
            fn_table,
            pure_fn_table,
            effect_table: None,
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Fn(fn_id.clone());
        match crate::dsl::resolver::ir::marker::eval_marker(
            &def.markers,
            definition,
            &env,
            &file_id,
            self,
        ) {
            Ok(Some(skip)) => {
                let cause_id = skip.cause_id();
                self.register_cause(cause_id, Cause::Skip(skip.clone()));
                let bail = LoweringBail::Skip(skip);
                self.pop_scope();
                self.pop_fn();
                self.pure_functions.insert(fn_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
            Ok(None) => {}
            Err(bail) => {
                let cause_id = bail.cause_id();
                self.register_cause(cause_id, Cause::from_bail(&bail));
                self.pop_scope();
                self.pop_fn();
                self.pure_functions.insert(fn_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
        }

        // Lower body
        let result = IrPureFn::lower(def, &file_id, self);

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_fn();

        // Cache and return
        self.pure_functions.insert(fn_id.clone(), result.clone());
        result
    }

    // ─── Resolve: Effects ────────────────────────────────────

    // TODO: box LoweringBail to reduce Result size
    #[allow(clippy::result_large_err)]
    pub fn resolve_effect(&mut self, effect_id: &EffectId) -> Result<IrEffect, LoweringBail> {
        // Check cache
        if let Some(result) = self.effects.get(effect_id) {
            return result;
        }

        // Check cycle
        if let Some(cycle) = self.find_effect_cycle(effect_id) {
            let bail = LoweringBail::Invalid(InvalidReport::Cycle(cycle));
            self.effects.insert(effect_id.clone(), Err(bail.clone()));
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

        // Create and populate local tables (effects get all three)
        let mut fn_table = self.local_fn_table();
        let mut pure_fn_table = self.local_pure_fn_table();
        let mut effect_table = self.local_effect_table();
        if let Err(e) = self.populate_local_tables(
            &effect_id.module,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        ) {
            let bail = LoweringBail::Invalid(e);
            self.effects.insert(effect_id.clone(), Err(bail.clone()));
            return Err(bail);
        }

        // Push in-progress
        let span = IrSpan::new(file_id.clone(), def.span);
        self.push_effect(effect_id.clone(), span);

        // Push scope
        self.push_scope(LoweringScope {
            module_path: effect_id.module.clone(),
            fn_table,
            pure_fn_table,
            effect_table: Some(effect_table),
        });

        // Evaluate markers
        let env = self.env.clone();
        let definition = DefinitionRef::Effect(effect_id.clone());
        match crate::dsl::resolver::ir::marker::eval_marker(
            &def.markers,
            definition,
            &env,
            &file_id,
            self,
        ) {
            Ok(Some(skip)) => {
                let cause_id = skip.cause_id();
                self.register_cause(cause_id, Cause::Skip(skip.clone()));
                let bail = LoweringBail::Skip(skip);
                self.pop_scope();
                self.pop_effect();
                self.effects.insert(effect_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
            Ok(None) => {}
            Err(bail) => {
                let cause_id = bail.cause_id();
                self.register_cause(cause_id, Cause::from_bail(&bail));
                self.pop_scope();
                self.pop_effect();
                self.effects.insert(effect_id.clone(), Err(bail.clone()));
                return Err(bail);
            }
        }

        // Lower body
        let result = IrEffect::lower(def, &file_id, self);

        // Pop scope and in-progress
        self.pop_scope();
        self.pop_effect();

        // Cache and return
        self.effects.insert(effect_id.clone(), result.clone());
        result
    }
}

// ─── Shared Test Helpers ─────────────────────────────────────
// Accessible from ir/ sub-module tests via
// `use crate::dsl::resolver::lower::test_helpers::*;`

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use crate::Span;
    use crate::diagnostics::{CauseTable, IrSpan, LoweringBail, ModulePath, WarningTable};
    use crate::dsl::parser::ast::*;
    use crate::dsl::resolver::ir::*;
    use crate::stack::Env;
    use crate::table::{FileId, FrozenTable, SharedTable, SourceFile};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    pub fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("/test/file.relux"))
    }

    pub fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), Span::new(0, 10))
    }

    pub fn test_env() -> Arc<Env> {
        Arc::new(Env::from_map(HashMap::new()))
    }

    pub fn empty_ast_table() -> AstTable {
        let shared: SharedTable<ModulePath, (FileId, AstModule)> = SharedTable::new();
        FrozenTable::try_from(shared).unwrap()
    }

    pub fn empty_source_table() -> SourceTable {
        let shared: SharedTable<FileId, SourceFile> = SharedTable::new();
        FrozenTable::try_from(shared).unwrap()
    }

    /// Parse a Relux source string into an AstModule.
    pub fn parse_module(source: &str) -> AstModule {
        crate::dsl::parse(source).unwrap_or_else(|e| panic!("parse error: {e:?}"))
    }

    /// Build a LoweringContext with a single "test" module from source,
    /// register BIFs, populate and push a scope with all three tables.
    pub fn ctx_with_source(source: &str) -> LoweringContext {
        ctx_with_modules(vec![("tests/a", "/test/a.relux", source)])
    }

    /// Build a LoweringContext with multiple modules.
    /// Each entry is (module_path, file_path, source).
    pub fn ctx_with_modules(modules: Vec<(&str, &str, &str)>) -> LoweringContext {
        let shared: SharedTable<ModulePath, (FileId, AstModule)> = SharedTable::new();
        for (mod_path, file_path, source) in &modules {
            let ast = parse_module(source);
            shared.insert(
                ModulePath((*mod_path).into()),
                (FileId::new(PathBuf::from(file_path)), ast),
            );
        }
        let ast_table = FrozenTable::try_from(shared).unwrap();
        let ctx = LoweringContext::new(
            ast_table,
            empty_source_table(),
            test_env(),
            CauseTable::default(),
            WarningTable::default(),
        );
        ctx.register_bifs();
        ctx
    }

    /// Push a scope for the given module path onto ctx, populating all three local tables.
    pub fn push_test_scope(ctx: &mut LoweringContext, mod_path: &str) {
        let module_path = ModulePath(mod_path.into());
        let file_id = ctx
            .ast_table()
            .get(&module_path)
            .unwrap_or_else(|| panic!("module {mod_path} not in ast_table"))
            .0
            .clone();
        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &module_path,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();
        ctx.push_scope(LoweringScope {
            module_path,
            fn_table,
            pure_fn_table,
            effect_table: Some(effect_table),
        });
    }

    /// Get the FileId for a module in the context.
    pub fn file_id_for(ctx: &LoweringContext, mod_path: &str) -> FileId {
        ctx.ast_table()
            .get(&ModulePath(mod_path.into()))
            .unwrap()
            .0
            .clone()
    }

    /// Helper: parse source, extract the first shell stmt from the first fn body.
    pub fn extract_first_stmt(source: &str) -> AstStmt {
        let module = parse_module(source);
        if let AstItem::Fn { def, .. } = &module.items[0].node {
            def.body[0].node.clone()
        } else {
            panic!("expected fn");
        }
    }

    /// Helper to extract the value expression from the first let statement in the first fn.
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

    /// Helper to extract the first test def from the context's ast_table and lower it.
    // TODO: box LoweringBail to reduce Result size
    #[allow(clippy::result_large_err)]
    pub fn lower_first_test(
        ctx: &mut LoweringContext,
        mod_path_str: &str,
    ) -> Result<IrTest, LoweringBail> {
        let mod_path = ModulePath(mod_path_str.into());
        let file = file_id_for(ctx, mod_path_str);
        let ast_table = ctx.ast_table().clone();
        let def = ast_table
            .get(&mod_path)
            .unwrap()
            .1
            .items
            .iter()
            .find_map(|item| match &item.node {
                AstItem::Test { def, .. } => Some(def),
                _ => None,
            })
            .unwrap();

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &mod_path,
            &file,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .map_err(LoweringBail::Invalid)?;
        ctx.push_scope(LoweringScope {
            module_path: mod_path.clone(),
            fn_table,
            pure_fn_table,
            effect_table: Some(effect_table),
        });
        let result = IrTest::lower(def, &file, ctx);
        ctx.pop_scope();
        result
    }

    /// Full pipeline: parse sources → load → lower → build plans → suite.
    pub fn resolve_source(sources: &[(&str, &str)], env: HashMap<String, String>) -> Suite {
        use crate::dsl::resolver::load_modules;
        use crate::dsl::resolver::loader::InMemoryLoader;

        let mut loader = InMemoryLoader::new();
        let mut seeds = Vec::new();
        for (mod_path, source) in sources {
            loader.add(mod_path, source);
            seeds.push(ModulePath((*mod_path).into()));
        }

        let causes: CauseTable = SharedTable::new();
        let warnings: WarningTable = SharedTable::new();
        let (ast_shared, source_shared) = load_modules(&loader, seeds, &causes, &warnings);
        let ast_table: AstTable = ast_shared.try_into().expect("ast_table freeze failed");
        let source_table: SourceTable = source_shared
            .try_into()
            .expect("source_table freeze failed");
        let mut ctx = LoweringContext::new(
            ast_table,
            source_table,
            Arc::new(Env::from_map(env)),
            causes,
            warnings,
        );
        ctx.register_bifs();
        let plans = build_all_plans(&mut ctx);
        ctx.into_suite(plans)
    }

    pub fn resolve_source_no_env(sources: &[(&str, &str)]) -> Suite {
        resolve_source(sources, HashMap::new())
    }

    pub fn plan_name(plan: &NewPlan) -> &str {
        plan.meta().name()
    }

    pub fn is_runnable(plan: &NewPlan) -> bool {
        matches!(plan, NewPlan::Runnable { .. })
    }

    pub fn is_skipped(plan: &NewPlan) -> bool {
        matches!(plan, NewPlan::Skipped { .. })
    }

    pub fn is_invalid(plan: &NewPlan) -> bool {
        matches!(plan, NewPlan::Invalid { .. })
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::diagnostics::{
        Cause, CauseId, CauseTable, DefinitionRef, EffectId, EffectName, FnId, InvalidReport,
        IrSpan, LoweringBail, ModulePath, SkipEvaluation, SkipReport, WarningTable,
    };
    use crate::dsl::resolver::ir::*;

    use crate::Span;
    use crate::Spanned;
    use crate::dsl::parser::ast::*;
    use crate::stack::Env;
    use crate::table::{FileId, FrozenTable, SharedTable, SourceFile};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

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
        )
    }

    fn make_context_with_bifs() -> LoweringContext {
        let ctx = make_context();
        ctx.register_bifs();
        ctx
    }

    /// Build an AstTable with the given modules.
    fn make_ast_table(modules: Vec<(ModulePath, FileId, AstModule)>) -> AstTable {
        let shared: SharedTable<ModulePath, (FileId, AstModule)> = SharedTable::new();
        for (path, file_id, module) in modules {
            shared.insert(path, (file_id, module));
        }
        FrozenTable::try_from(shared).unwrap()
    }

    fn make_context_with_ast(modules: Vec<(ModulePath, FileId, AstModule)>) -> LoweringContext {
        let ast_table = make_ast_table(modules);
        let ctx = LoweringContext::new(
            ast_table,
            empty_source_table(),
            test_env(),
            CauseTable::default(),
            WarningTable::default(),
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
            exported_shell: Spanned::new(AstIdent::new("sh", Span::new(0, 2)), Span::new(0, 2)),
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
        let env = Arc::new(Env::from_map(m));
        let ctx = LoweringContext::new(
            empty_ast_table(),
            empty_source_table(),
            env.clone(),
            CauseTable::default(),
            WarningTable::default(),
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
        );
        assert!(ctx.ast_table().get(&mod_path).is_some());
    }

    #[test]
    fn context_new_preserves_cause_table() {
        let causes: CauseTable = SharedTable::new();
        let id = CauseId::generate("m", "f", 0, "err");
        causes.insert(
            id.clone(),
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        let ctx = LoweringContext::new(
            empty_ast_table(),
            empty_source_table(),
            test_env(),
            causes,
            WarningTable::default(),
        );
        assert!(ctx.causes().get(&id).is_some());
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
        let impure_bifs: Vec<(&str, usize)> =
            vec![("match_prompt", 0), ("ctrl_c", 0), ("ctrl_d", 0)];
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
            assert_eq!(arity, 3);
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
        let mut lt = ctx.local_fn_table();
        let local_key = LocalFnKey::new("trim", 1);
        let global_key = FnId {
            module: builtin_mod(),
            name: "trim".into(),
            arity: 1,
        };
        lt.insert(local_key.clone(), global_key);
        assert!(lt.get(&local_key).is_some());
    }

    #[test]
    fn local_pure_fn_table_shares_registry() {
        let ctx = make_context_with_bifs();
        let mut lt1 = ctx.local_pure_fn_table();
        let mut lt2 = ctx.local_pure_fn_table();
        // Both should see the same registry.
        let key = LocalFnKey::new("uuid", 0);
        let gk = FnId {
            module: builtin_mod(),
            name: "uuid".into(),
            arity: 0,
        };
        lt1.insert(key.clone(), gk.clone());
        lt2.insert(key.clone(), gk);
        assert!(lt1.get(&key).is_some());
        assert!(lt2.get(&key).is_some());
    }

    #[test]
    fn local_effect_table_initially_empty() {
        let ctx = make_context();
        let lt = ctx.local_effect_table();
        let key = LocalEffectKey::new(EffectName("Db".into()));
        assert!(lt.get(&key).is_none());
    }

    #[test]
    fn local_fn_table_independent_locals() {
        let ctx = make_context_with_bifs();
        let mut lt1 = ctx.local_fn_table();
        let mut lt2 = ctx.local_fn_table();
        let gk = FnId {
            module: builtin_mod(),
            name: "trim".into(),
            arity: 1,
        };
        lt1.insert(LocalFnKey::new("my_trim", 1), gk.clone());
        lt2.insert(LocalFnKey::new("your_trim", 1), gk);
        // lt1 doesn't see lt2's local mapping.
        assert!(lt1.get(&LocalFnKey::new("your_trim", 1)).is_none());
        assert!(lt2.get(&LocalFnKey::new("my_trim", 1)).is_none());
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let result =
            ctx.populate_local_tables(&mod_path, &file_id, &mut fn_table, &mut pure_fn_table, None);
        assert!(result.is_ok());
        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 1)));
    }

    #[test]
    fn populate_own_fn_multiple_arities() {
        let mod_path = ModulePath("tests/a".into());
        let file_id = test_file_id();
        let module = make_module(vec![make_fn_def("foo", 0), make_fn_def("foo", 1)]);
        let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(&mod_path, &file_id, &mut fn_table, &mut pure_fn_table, None)
            .unwrap();
        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 0)));
        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 1)));
    }

    #[test]
    fn populate_own_effect_definitions() {
        let mod_path = ModulePath("tests/a".into());
        let file_id = test_file_id();
        let module = make_module(vec![make_effect_def("Db")]);
        let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &mod_path,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();
        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("Db".into()))));
    }

    #[test]
    fn populate_own_pure_fn_definitions() {
        let mod_path = ModulePath("tests/a".into());
        let file_id = test_file_id();
        let module = make_module(vec![make_pure_fn_def("bar", 0)]);
        let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(&mod_path, &file_id, &mut fn_table, &mut pure_fn_table, None)
            .unwrap();
        // Pure fns go in both tables.
        assert!(fn_table.contains_local(&LocalFnKey::new("bar", 0)));
        assert!(pure_fn_table.contains_local(&LocalFnKey::new("bar", 0)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &mod_path,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();
        assert!(fn_table.contains_local(&LocalFnKey::new("impure_fn", 1)));
        assert!(fn_table.contains_local(&LocalFnKey::new("pure_fn", 0)));
        assert!(pure_fn_table.contains_local(&LocalFnKey::new("pure_fn", 0)));
        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("Setup".into()))));
    }

    #[test]
    fn populate_empty_module() {
        let mod_path = ModulePath("tests/a".into());
        let file_id = test_file_id();
        let module = make_module(vec![]);
        let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &mod_path,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();
        // No local entries (BIFs are in the registry, not local mappings).
        assert!(!fn_table.contains_local(&LocalFnKey::new("anything", 0)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();

        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 1)));
        assert!(fn_table.contains_local(&LocalFnKey::new("bar", 0)));
        assert!(pure_fn_table.contains_local(&LocalFnKey::new("bar", 0)));
        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("StartDb".into()))));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        )
        .unwrap();

        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 1)));
        // bar was not selectively imported.
        assert!(!fn_table.contains_local(&LocalFnKey::new("bar", 0)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();

        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("StartDb".into()))));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();

        assert!(fn_table.contains_local(&LocalFnKey::new("foo", 0)));
        assert!(fn_table.contains_local(&LocalFnKey::new("bar", 1)));
        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("StartDb".into()))));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        )
        .unwrap();

        // "bar" is the local alias for "foo".
        assert!(fn_table.contains_local(&LocalFnKey::new("bar", 1)));
        // "foo" should NOT be in the local table — only the alias.
        assert!(!fn_table.contains_local(&LocalFnKey::new("foo", 1)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        )
        .unwrap();

        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("Db".into()))));
        assert!(!effect_table.contains_local(&LocalEffectKey::new(EffectName("StartDb".into()))));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        )
        .unwrap();

        assert!(fn_table.contains_local(&LocalFnKey::new("alpha", 0)));
        assert!(fn_table.contains_local(&LocalFnKey::new("beta", 1)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        ctx.populate_local_tables(
            &test_path,
            &test_fid,
            &mut fn_table,
            &mut pure_fn_table,
            None,
        )
        .unwrap();

        // my_fn is imported, but BIFs like "trim" are not in local mappings.
        assert!(fn_table.contains_local(&LocalFnKey::new("my_fn", 0)));
        assert!(!fn_table.contains_local(&LocalFnKey::new("trim", 1)));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                Some(&mut effect_table),
            )
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
            .unwrap_err();
        assert!(matches!(err, InvalidReport::NameConflict { .. }));
    }

    #[test]
    fn populate_fn_and_effect_same_name_ok() {
        let mod_path = ModulePath("tests/a".into());
        let file_id = test_file_id();
        let module = make_module(vec![make_fn_def("setup", 0), make_effect_def("Setup")]);
        let ctx = make_context_with_ast(vec![(mod_path.clone(), file_id.clone(), module)]);

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let mut effect_table = ctx.local_effect_table();
        let result = ctx.populate_local_tables(
            &mod_path,
            &file_id,
            &mut fn_table,
            &mut pure_fn_table,
            Some(&mut effect_table),
        );
        assert!(result.is_ok());
        assert!(fn_table.contains_local(&LocalFnKey::new("setup", 0)));
        assert!(effect_table.contains_local(&LocalEffectKey::new(EffectName("Setup".into()))));
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

        let mut fn_table = ctx.local_fn_table();
        let mut pure_fn_table = ctx.local_pure_fn_table();
        let err = ctx
            .populate_local_tables(
                &test_path,
                &test_fid,
                &mut fn_table,
                &mut pure_fn_table,
                None,
            )
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
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        assert!(ctx.causes().get(&id).is_some());
    }

    #[test]
    fn register_cause_retrievable() {
        let ctx = make_context();
        let id = CauseId::generate("m", "f", 0, "err");
        ctx.register_cause(
            id.clone(),
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
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
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        ctx.register_cause(
            id2.clone(),
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
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
        ctx.register_cause(id.clone(), Cause::Skip(skip));
        assert!(matches!(ctx.causes().get(&id).unwrap(), Cause::Skip(_)));
    }

    #[test]
    fn register_cause_invalid_variant() {
        let ctx = make_context();
        let id = CauseId::generate("m", "f", 0, "invalid");
        ctx.register_cause(
            id.clone(),
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
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
        let plan = NewPlan::Invalid {
            meta,
            causes: vec![],
            warnings: vec![],
        };
        let suite = ctx.into_suite(vec![plan]);
        assert_eq!(suite.plans.len(), 1);
    }

    #[test]
    fn into_suite_transfers_source_map() {
        let shared_src: SharedTable<FileId, SourceFile> = SharedTable::new();
        let fid = test_file_id();
        shared_src.insert(
            fid.clone(),
            SourceFile {
                path: PathBuf::from("/test/file.relux"),
                source: "// test".into(),
            },
        );
        let source_map = FrozenTable::try_from(shared_src).unwrap();

        let ctx = LoweringContext::new(
            empty_ast_table(),
            source_map,
            test_env(),
            CauseTable::default(),
            WarningTable::default(),
        );
        let suite = ctx.into_suite(vec![]);
        assert!(suite.source_map.get(&fid).is_some());
    }

    #[test]
    fn into_suite_transfers_env() {
        let mut m = HashMap::new();
        m.insert("MY_VAR".into(), "my_val".into());
        let env = Arc::new(Env::from_map(m));
        let ctx = LoweringContext::new(
            empty_ast_table(),
            empty_source_table(),
            env,
            CauseTable::default(),
            WarningTable::default(),
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
            Cause::Invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        let ctx = LoweringContext::new(
            empty_ast_table(),
            empty_source_table(),
            test_env(),
            causes,
            WarningTable::default(),
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
    fn lower_imported_effect_with_sub_needs() {
        let mut ctx = ctx_with_modules(vec![
            (
                "tests/a",
                "/test/a.relux",
                "import effects\ntest \"t\" {\n  need App\n  shell sh {\n    > cmd\n  }\n}\n",
            ),
            (
                "lib/effects",
                "/lib/effects.relux",
                "effect Db -> db {\n  shell db {\n    > db\n  }\n}\neffect App -> app {\n  need Db\n  shell app {\n    > app\n  }\n}\n",
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
        let source = r#"effect Bad -> bad {
  need Nonexistent
  shell bad {
    > x
  }
}
effect User -> user {
  need Bad
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
        if let Err(LoweringBail::Invalid(InvalidReport::UndefinedFunctionCall { span, .. })) =
            &result
        {
            assert_eq!(
                span.file(),
                &FileId::new(PathBuf::from("/lib/helpers.relux"))
            );
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
        if let Err(LoweringBail::Invalid(InvalidReport::UndefinedFunctionCall {
            name, span, ..
        })) = &result
        {
            assert_eq!(name, "nonexistent");
            let s = span.span();
            assert!(s.end > s.start);
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
            r#"effect Setup -> sh {
  shell sh {
    > echo setup
  }
}

test "t1" {
  need Setup
  shell sh {
    > echo 1
  }
}

test "t2" {
  need Setup
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
                r#"effect Db -> db_sh {
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
  need Db
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
    fn effect_cycle_via_need() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"effect A -> sh {
  need B
  shell sh {
    > echo a
  }
}

effect B -> sh {
  need A
  shell sh {
    > echo b
  }
}

test "t" {
  need A
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
    fn undefined_effect_need() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t" {
  need NonExistent
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
            r#"effect Db -> db {
  shell db {
    > echo db
  }
}

test "t" {
  need Db
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
}
