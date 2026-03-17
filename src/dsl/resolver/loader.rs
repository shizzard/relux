use super::*;

pub(super) struct Loader<'a> {
    pub(super) source_map: SourceMap,
    pub(super) asts: AstTable,
    loading_stack: Vec<(ModulePath, Option<Span>)>,
    pub(super) diagnostics: Vec<DiagnosticError>,
    source_loader: &'a dyn SourceLoader,
}

impl<'a> Loader<'a> {
    pub(super) fn new(source_loader: &'a dyn SourceLoader) -> Self {
        Self {
            source_map: SourceMap::new(),
            asts: LookupTable::new(),
            loading_stack: Vec::new(),
            diagnostics: Vec::new(),
            source_loader,
        }
    }

    pub(super) fn load_module(&mut self, mod_path: &str, referenced_from: Option<Span>) {
        if self.asts.contains_key(mod_path) {
            return;
        }

        if let Some(pos) = self
            .loading_stack
            .iter()
            .position(|(p, _)| p.as_ref() == mod_path)
        {
            let cycle: Vec<(String, Option<Span>)> = self.loading_stack[pos..]
                .iter()
                .map(|(p, s)| (p.to_string(), s.clone()))
                .chain(std::iter::once((
                    mod_path.to_string(),
                    referenced_from.clone(),
                )))
                .collect();
            self.diagnostics
                .push(DiagnosticError::CircularImport { cycle });
            return;
        }

        let (file_path, source) = match self.source_loader.load(mod_path) {
            Some(pair) => pair,
            None => {
                match referenced_from {
                    Some(span) => {
                        self.diagnostics.push(DiagnosticError::ModuleNotFound {
                            path: mod_path.to_string(),
                            referenced_from: span,
                        });
                    }
                    None => {
                        self.diagnostics.push(DiagnosticError::RootNotFound {
                            path: mod_path.to_string(),
                        });
                    }
                }
                return;
            }
        };

        let file_id = self.source_map.add(file_path, source.clone());
        let module = match parser::parse(&source) {
            Ok(m) => m,
            Err(error) => {
                self.diagnostics.push(DiagnosticError::Parse {
                    file: file_id,
                    error,
                });
                return;
            }
        };

        self.loading_stack
            .push((ModulePath::from(mod_path), referenced_from));

        let import_paths: Vec<_> = module
            .items
            .iter()
            .filter_map(|item| {
                if let parser::AstItem::Import { import: imp, .. } = &item.node {
                    Some((
                        imp.path.node.clone(),
                        Span::new(file_id, imp.path.span.into()),
                    ))
                } else {
                    None
                }
            })
            .collect();

        for (path, span) in &import_paths {
            self.load_module(path, Some(span.clone()));
        }

        self.loading_stack.pop();
        self.asts.insert(
            ModulePath::from(mod_path),
            Located {
                file: file_id,
                def: module,
            },
        );
    }
}
