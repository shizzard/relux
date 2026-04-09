use std::collections::VecDeque;
use std::path::PathBuf;

use crate::SourceLoader;
use relux_core::Span;
use relux_core::diagnostics::Cause;
use relux_core::diagnostics::CauseId;
use relux_core::diagnostics::CauseTable;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::ModulePath;
use relux_core::diagnostics::WarningTable;
use relux_core::table::FileId;
use relux_core::table::SharedTable;
use relux_core::table::SourceFile;

use relux_core::table::SourceTable;
use relux_ir::AstTable;

// ─── load_modules ───────────────────────────────────────────

/// Load all reachable modules via a demand-driven BFS worklist.
///
/// Seeded with test module paths from discovery. For each module:
/// 1. Load source via `SourceLoader`, parse into `AstModule`
/// 2. Store `(FileId, AstModule)` in `ast_table`, `SourceFile` in `source_table`
/// 3. Walk AST import items, enqueue module paths not yet in the table
///
/// Errors (missing modules, parse failures) are recorded in `cause_table`
/// but do not block loading of other modules.
pub fn load_modules(
    source_loader: &dyn SourceLoader,
    seeds: Vec<ModulePath>,
    cause_table: &CauseTable,
    _warning_table: &WarningTable,
) -> (AstTable, SourceTable) {
    let ast_table: AstTable = SharedTable::new();
    let source_table: SourceTable = SharedTable::new();

    let mut queue: VecDeque<ModulePath> = VecDeque::new();

    // Seed the worklist, deduplicating
    for path in seeds {
        if !ast_table.contains(&path) && !queue.iter().any(|p| p == &path) {
            queue.push_back(path);
        }
    }

    while let Some(mod_path) = queue.pop_front() {
        // Already loaded (could have been enqueued multiple times before processing)
        if ast_table.contains(&mod_path) {
            continue;
        }

        // Load source
        let Some((file_path, source)) = source_loader.load(&mod_path.0) else {
            // Module not found — record cause
            let cause_id = CauseId::generate(&mod_path.0, "", 0, "module_not_found");
            let ir_span = IrSpan::new(FileId::new(PathBuf::from(&mod_path.0)), Span::new(0, 0));
            cause_table.insert(
                cause_id,
                Cause::invalid(InvalidReport::UndefinedModuleImport {
                    module_path: mod_path.clone(),
                    span: ir_span,
                }),
            );
            continue;
        };

        let file_id = FileId::new(file_path.clone());

        // Store source
        source_table.insert(
            file_id.clone(),
            SourceFile {
                path: file_path,
                source: source.clone(),
            },
        );

        // Parse
        match relux_parser::parse(&source) {
            Ok(module) => {
                // Walk imports to find transitive dependencies
                for item in &module.items {
                    if let relux_ast::AstItem::Import { import, .. } = &item.node {
                        // Imports are relative to lib/ — prepend to form ModulePath
                        let import_path = ModulePath(format!("lib/{}", import.path.node));
                        if !ast_table.contains(&import_path) {
                            queue.push_back(import_path);
                        }
                    }
                }

                ast_table.insert(mod_path, (file_id, module));
            }
            Err(parse_err) => {
                // Record parse error as cause
                let cause_id = CauseId::generate(&mod_path.0, "", 0, "parse_error");
                let s = parse_err.span();
                let err_span = Span::new(s.start, s.end);
                let ir_span = IrSpan::new(file_id, err_span);
                cause_table.insert(
                    cause_id,
                    Cause::invalid(InvalidReport::parse_error(
                        mod_path,
                        parse_err.to_string(),
                        ir_span,
                    )),
                );
            }
        }
    }

    (ast_table, source_table)
}

// ─── InMemoryLoader ─────────────────────────────────────────

/// Test helper implementing `SourceLoader` with in-memory sources.
#[derive(Default)]
pub struct InMemoryLoader {
    modules: std::collections::HashMap<String, (PathBuf, String)>,
}

impl InMemoryLoader {
    pub fn new() -> Self {
        Self {
            modules: std::collections::HashMap::new(),
        }
    }

    pub fn add(&mut self, mod_path: &str, source: &str) -> &mut Self {
        self.modules.insert(
            mod_path.to_string(),
            (
                PathBuf::from(format!("{mod_path}.relux")),
                source.to_string(),
            ),
        );
        self
    }
}

impl SourceLoader for InMemoryLoader {
    fn load(&self, mod_path: &str) -> Option<(PathBuf, String)> {
        self.modules.get(mod_path).cloned()
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use relux_core::table::SharedTable;

    fn mp(s: &str) -> ModulePath {
        ModulePath(s.into())
    }

    fn new_tables() -> (CauseTable, WarningTable) {
        (SharedTable::new(), SharedTable::new())
    }

    // ── InMemoryLoader ─────────────────────────────────────

    #[test]
    fn in_memory_loader_returns_source() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/login", "test \"login\" {}");
        let result = loader.load("tests/login");
        assert!(result.is_some());
        let (path, source) = result.unwrap();
        assert_eq!(path, PathBuf::from("tests/login.relux"));
        assert_eq!(source, "test \"login\" {}");
    }

    #[test]
    fn in_memory_loader_returns_none() {
        let loader = InMemoryLoader::new();
        assert!(loader.load("nonexistent").is_none());
    }

    #[test]
    fn in_memory_loader_empty() {
        let loader = InMemoryLoader::new();
        assert!(loader.load("anything").is_none());
    }

    // ── Worklist — happy path ──────────────────────────────

    #[test]
    fn load_single_module_no_imports() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/login", "test \"login\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/login")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/login")));
    }

    #[test]
    fn load_module_with_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/login", "import helpers\ntest \"login\" {}");
        loader.add("lib/helpers", "fn greet() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/login")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/login")));
        assert!(ast.contains(&mp("lib/helpers")));
    }

    #[test]
    fn load_transitive_imports() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import b\ntest \"a\" {}");
        loader.add("lib/b", "import c\nfn b() {}");
        loader.add("lib/c", "fn c() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("lib/b")));
        assert!(ast.contains(&mp("lib/c")));
    }

    #[test]
    fn load_diamond_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import b\nimport c\ntest \"a\" {}");
        loader.add("lib/b", "import d\nfn b() {}");
        loader.add("lib/c", "import d\nfn c() {}");
        loader.add("lib/d", "fn d() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("lib/b")));
        assert!(ast.contains(&mp("lib/c")));
        assert!(ast.contains(&mp("lib/d")));
    }

    #[test]
    fn load_circular_import_no_error() {
        let mut loader = InMemoryLoader::new();
        // lib/b imports lib/c, lib/c imports lib/b → circular
        loader.add("tests/a", "import b\ntest \"a\" {}");
        loader.add("lib/b", "import c\nfn b() {}");
        loader.add("lib/c", "import b\nfn c() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("lib/b")));
        assert!(ast.contains(&mp("lib/c")));
    }

    #[test]
    fn load_already_loaded_skipped() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "test \"a\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(
            &loader,
            vec![mp("tests/a"), mp("tests/a")],
            &causes,
            &warnings,
        );
        assert!(ast.contains(&mp("tests/a")));
    }

    #[test]
    fn load_source_stored_in_source_table() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/login", "test \"login\" {}");
        let (causes, warnings) = new_tables();
        let (_, src) = load_modules(&loader, vec![mp("tests/login")], &causes, &warnings);
        let file_id = FileId::new(PathBuf::from("tests/login.relux"));
        let sf = src.get(&file_id);
        assert!(sf.is_some());
        let sf = sf.unwrap();
        assert_eq!(sf.source, "test \"login\" {}");
    }

    #[test]
    fn load_empty_module() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/empty", "");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/empty")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/empty")));
    }

    #[test]
    fn load_module_with_only_comments() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/comments", "// just a comment\n");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/comments")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/comments")));
    }

    #[test]
    fn load_multiple_test_modules() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "test \"a\" {}");
        loader.add("tests/b", "test \"b\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(
            &loader,
            vec![mp("tests/a"), mp("tests/b")],
            &causes,
            &warnings,
        );
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("tests/b")));
    }

    #[test]
    fn load_lib_only_via_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import helpers\ntest \"a\" {}");
        loader.add("lib/helpers", "fn greet() {}");
        let (causes, warnings) = new_tables();
        // Only seed with test module — lib loaded via import
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("lib/helpers")));
    }

    #[test]
    fn load_unused_lib_not_loaded() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "test \"a\" {}");
        loader.add("lib/unused", "fn unused() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(!ast.contains(&mp("lib/unused")));
    }

    #[test]
    fn load_deeply_nested_transitive() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import b\ntest \"a\" {}");
        loader.add("lib/b", "import c\nfn b() {}");
        loader.add("lib/c", "import d\nfn c() {}");
        loader.add("lib/d", "import e\nfn d() {}");
        loader.add("lib/e", "fn e() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        for path in ["tests/a", "lib/b", "lib/c", "lib/d", "lib/e"] {
            assert!(ast.contains(&mp(path)), "missing {path}");
        }
    }

    // ── Error handling ─────────────────────────────────────

    #[test]
    fn load_missing_module_records_cause() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import missing\ntest \"a\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        let cause_id = CauseId::generate("lib/missing", "", 0, "module_not_found");
        let cause = causes.get(&cause_id);
        assert!(cause.is_some(), "expected cause for missing module");
        assert!(matches!(cause.unwrap(), Cause::Invalid(_)));
    }

    #[test]
    fn load_missing_module_other_modules_still_loaded() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import missing\ntest \"a\" {}");
        loader.add("tests/b", "test \"b\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(
            &loader,
            vec![mp("tests/a"), mp("tests/b")],
            &causes,
            &warnings,
        );
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("tests/b")));
    }

    #[test]
    fn load_parse_error_records_cause() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/bad", "test \"bad\" { !!! invalid syntax @@@ }");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/bad")], &causes, &warnings);
        assert!(!ast.contains(&mp("tests/bad")));
        let cause_id = CauseId::generate("tests/bad", "", 0, "parse_error");
        let cause = causes.get(&cause_id);
        assert!(cause.is_some(), "expected cause for parse error");
        assert!(matches!(cause.unwrap(), Cause::Invalid(_)));
    }

    #[test]
    fn load_parse_error_other_modules_still_loaded() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/bad", "test \"bad\" { !!! }");
        loader.add("tests/good", "test \"good\" {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(
            &loader,
            vec![mp("tests/bad"), mp("tests/good")],
            &causes,
            &warnings,
        );
        assert!(ast.contains(&mp("tests/good")));
    }

    #[test]
    fn load_missing_transitive_dep() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import b\ntest \"a\" {}");
        loader.add("lib/b", "import missing\nfn b() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        assert!(ast.contains(&mp("lib/b")));
        let cause_id = CauseId::generate("lib/missing", "", 0, "module_not_found");
        assert!(causes.get(&cause_id).is_some());
    }

    #[test]
    fn load_multiple_missing_modules() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import x\nimport y\ntest \"a\" {}");
        let (causes, warnings) = new_tables();
        let (_ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        let id_x = CauseId::generate("lib/x", "", 0, "module_not_found");
        let id_y = CauseId::generate("lib/y", "", 0, "module_not_found");
        assert!(causes.get(&id_x).is_some());
        assert!(causes.get(&id_y).is_some());
    }

    #[test]
    fn load_parse_error_in_transitive_dep() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/a", "import bad\ntest \"a\" {}");
        loader.add("lib/bad", "!!! invalid !!!");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/a")));
        assert!(!ast.contains(&mp("lib/bad")));
        let cause_id = CauseId::generate("lib/bad", "", 0, "parse_error");
        assert!(causes.get(&cause_id).is_some());
    }

    #[test]
    fn load_missing_root_module() {
        let loader = InMemoryLoader::new();
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/missing")], &causes, &warnings);
        assert!(!ast.contains(&mp("tests/missing")));
        let cause_id = CauseId::generate("tests/missing", "", 0, "module_not_found");
        assert!(causes.get(&cause_id).is_some());
    }

    // ── Freeze semantics ───────────────────────────────────

    // ── Additional error accumulation ─────────────────────

    #[test]
    fn load_multiple_parse_errors_accumulated() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/bad1", "!!! invalid1 !!!");
        loader.add("tests/bad2", "@@@ invalid2 @@@");
        let (causes, warnings) = new_tables();
        let (_ast, _src) = load_modules(
            &loader,
            vec![mp("tests/bad1"), mp("tests/bad2")],
            &causes,
            &warnings,
        );
        let id1 = CauseId::generate("tests/bad1", "", 0, "parse_error");
        let id2 = CauseId::generate("tests/bad2", "", 0, "parse_error");
        assert!(causes.get(&id1).is_some(), "expected cause for bad1");
        assert!(causes.get(&id2).is_some(), "expected cause for bad2");
    }

    #[test]
    fn load_circular_with_transitive() {
        let mut loader = InMemoryLoader::new();
        // A→B→C→B (circular), C also→D (transitive)
        loader.add("tests/a", "import b\ntest \"a\" {}");
        loader.add("lib/b", "import c\nfn b() {}");
        loader.add("lib/c", "import b\nimport d\nfn c() {}");
        loader.add("lib/d", "fn d() {}");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/a")], &causes, &warnings);
        // All reachable modules should be loaded despite the cycle.
        for path in ["tests/a", "lib/b", "lib/c", "lib/d"] {
            assert!(ast.contains(&mp(path)), "missing {path}");
        }
    }

    #[test]
    fn load_empty_source() {
        let mut loader = InMemoryLoader::new();
        loader.add("tests/empty", "");
        let (causes, warnings) = new_tables();
        let (ast, _src) = load_modules(&loader, vec![mp("tests/empty")], &causes, &warnings);
        assert!(ast.contains(&mp("tests/empty")));
    }
}
