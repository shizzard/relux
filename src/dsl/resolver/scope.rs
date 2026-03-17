use super::*;

pub(super) fn build_module_exports(file_id: FileId, module: &parser::AstModule) -> ModuleExports {
    let mut functions: FnTable = LookupTable::new();
    let mut pure_functions: PureFnTable = LookupTable::new();
    let mut effects: EffectTable = LookupTable::new();

    for item in &module.items {
        match &item.node {
            parser::AstItem::Fn { def: f, .. } => {
                let key = FnKey {
                    name: f.name.node.clone(),
                    arity: f.params.len(),
                };
                functions.insert(
                    key,
                    Located {
                        file: file_id,
                        def: f.clone(),
                    },
                );
            }
            parser::AstItem::PureFn { def: f, .. } => {
                let key = FnKey {
                    name: f.name.node.clone(),
                    arity: f.params.len(),
                };
                pure_functions.insert(
                    key,
                    Located {
                        file: file_id,
                        def: f.clone(),
                    },
                );
            }
            parser::AstItem::Effect { def: e, .. } => {
                effects.insert(
                    EffectName::from(e.name.node.clone()),
                    Located {
                        file: file_id,
                        def: e.clone(),
                    },
                );
            }
            _ => {}
        }
    }

    ModuleExports {
        functions,
        pure_functions,
        effects,
    }
}

pub(super) fn build_module_scope(
    file_id: FileId,
    module: &parser::AstModule,
    all_asts: &AstTable,
) -> ScopeResult {
    let mut errors = Vec::new();
    let warnings = Vec::new();
    let own_exports = build_module_exports(file_id, module);
    let mut scope = ModuleScope {
        functions: own_exports.functions.clone(),
        pure_functions: own_exports.pure_functions.clone(),
        effects: own_exports.effects.clone(),
    };

    for item in &module.items {
        let imp = match &item.node {
            parser::AstItem::Import { import: imp, .. } => imp,
            _ => continue,
        };

        let target_path = &imp.path.node;
        let Located {
            file: target_file_id,
            def: target_module,
        } = match all_asts.get(target_path.as_str()) {
            Some(located) => located,
            None => continue, // already reported as ModuleNotFound
        };

        let target_exports = build_module_exports(*target_file_id, target_module);

        match &imp.names {
            None => {
                // Wildcard import: bring everything in
                for (key, val) in target_exports.functions.iter() {
                    if let Some(existing) = scope.functions.get(key) {
                        errors.push(DiagnosticError::DuplicateDefinition {
                            name: key.name.clone(),
                            arity: Some(key.arity),
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.functions.insert(key.clone(), val.clone());
                    }
                }
                for (key, val) in target_exports.pure_functions.iter() {
                    if let Some(existing) = scope.pure_functions.get(key) {
                        errors.push(DiagnosticError::DuplicateDefinition {
                            name: key.name.clone(),
                            arity: Some(key.arity),
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.pure_functions.insert(key.clone(), val.clone());
                    }
                }
                for (name, val) in target_exports.effects.iter() {
                    if let Some(existing) = scope.effects.get(name) {
                        errors.push(DiagnosticError::DuplicateDefinition {
                            name: name.to_string(),
                            arity: None,
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.effects.insert(name.clone(), val.clone());
                    }
                }
            }
            Some(names) => {
                for import_name in names {
                    let raw_name = &import_name.node.name.node;
                    let local_name = import_name
                        .node
                        .alias
                        .as_ref()
                        .map(|a| a.node.clone())
                        .unwrap_or_else(|| raw_name.clone());

                    let mut found = false;

                    // Try functions (all arities)
                    let fn_matches: Vec<_> = target_exports
                        .functions
                        .iter()
                        .filter(|(key, _)| key.name == *raw_name)
                        .collect();
                    for (key, val) in &fn_matches {
                        found = true;
                        let new_key = FnKey {
                            name: local_name.clone(),
                            arity: key.arity,
                        };
                        if let Some(existing) = scope.functions.get(&new_key) {
                            errors.push(DiagnosticError::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: Some(key.arity),
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope.functions.insert(new_key, (*val).clone());
                        }
                    }

                    // Try pure functions (all arities)
                    let pure_fn_matches: Vec<_> = target_exports
                        .pure_functions
                        .iter()
                        .filter(|(key, _)| key.name == *raw_name)
                        .collect();
                    for (key, val) in &pure_fn_matches {
                        found = true;
                        let new_key = FnKey {
                            name: local_name.clone(),
                            arity: key.arity,
                        };
                        if let Some(existing) = scope.pure_functions.get(&new_key) {
                            errors.push(DiagnosticError::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: Some(key.arity),
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope.pure_functions.insert(new_key, (*val).clone());
                        }
                    }

                    // Try effects
                    if let Some(val) = target_exports.effects.get(raw_name.as_str()) {
                        found = true;
                        if let Some(existing) = scope.effects.get(local_name.as_str()) {
                            errors.push(DiagnosticError::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: None,
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope
                                .effects
                                .insert(EffectName::from(local_name.clone()), val.clone());
                        }
                    }

                    if !found {
                        errors.push(DiagnosticError::ImportNotExported {
                            name: raw_name.clone(),
                            module_path: target_path.clone(),
                            span: Span::new(file_id, import_name.node.name.span.into()),
                        });
                    }
                }
            }
        }
    }

    ScopeResult {
        scope,
        errors,
        warnings,
    }
}

fn def_span(file_id: FileId, name_span: &parser::Span) -> Span {
    Span::new(file_id, (*name_span).into())
}
