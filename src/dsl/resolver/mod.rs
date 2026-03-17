pub mod error;
pub mod ir;

mod discover;
mod effect_graph;
mod loader;
mod lower;
mod plan;
mod scope;

pub use error::{DiagnosticError, DiagnosticWarning};

use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dsl::discover_relux_files;

use crate::config;
use crate::dsl::parser;
use ir::{FileId, SourceMap, Span};

// ─── Source Loader ──────────────────────────────────────────

pub trait SourceLoader {
    fn load(&self, mod_path: &str) -> Option<(PathBuf, String)>;
}

pub struct FsSourceLoader {
    search_paths: Vec<PathBuf>,
}

impl FsSourceLoader {
    pub fn new(project_root: PathBuf, extra_search_paths: Vec<PathBuf>) -> Self {
        let mut search_paths = vec![project_root];
        search_paths.extend(extra_search_paths);
        Self { search_paths }
    }
}

impl SourceLoader for FsSourceLoader {
    fn load(&self, mod_path: &str) -> Option<(PathBuf, String)> {
        for base in &self.search_paths {
            let file_path = base.join(mod_path).with_extension("relux");
            if let Ok(source) = std::fs::read_to_string(&file_path) {
                return Some((file_path, source));
            }
        }
        None
    }
}

// ─── Name Resolution Types ──────────────────────────────────

/// A definition paired with the file it originates from.
#[derive(Debug, Clone)]
struct Located<T> {
    file: FileId,
    def: T,
}

/// A keyed lookup table built during resolution.
#[derive(Debug, Clone)]
struct LookupTable<K, V>(HashMap<K, V>);

impl<K, V> std::ops::Deref for LookupTable<K, V> {
    type Target = HashMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> std::ops::DerefMut for LookupTable<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V> LookupTable<K, V> {
    fn new() -> Self {
        LookupTable(HashMap::new())
    }
}

/// Function identity: name + arity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FnKey {
    name: String,
    arity: usize,
}

/// Shared context for all `lower_*` functions.
/// Each instance owns a fresh error vec; callers merge after use.
struct LoweringContext<'a> {
    file_id: FileId,
    scope: &'a ModuleScope,
    multiplier: f64,
    errors: Vec<DiagnosticError>,
}

// ─── Result Types ──────────────────────────────────────────

/// Scope building always succeeds. Errors/warnings indicate issues that didn't
/// prevent scope construction (e.g. bad import — name just isn't added).
struct ScopeResult {
    scope: ModuleScope,
    errors: Vec<DiagnosticError>,
    warnings: Vec<DiagnosticWarning>,
}

/// Per-test plan result. No invalid states:
/// - Ok: plan is valid, may have warnings
/// - Err: plan construction failed, errors explain why
pub enum PlanResult {
    Ok {
        plan: Box<ir::Plan>,
        warnings: Vec<DiagnosticWarning>,
    },
    Err {
        errors: Vec<DiagnosticError>,
        warnings: Vec<DiagnosticWarning>,
    },
}

/// Top-level resolution output.
pub struct ResolveResult {
    pub plan_results: Vec<PlanResult>,
    pub source_map: SourceMap,
    pub module_errors: Vec<DiagnosticError>,
    pub module_warnings: Vec<DiagnosticWarning>,
}

/// Accumulator for resolved functions, indexed by `FnKey`.
/// Generic over the function type (`T`) and its index type (`I`).
struct FunctionRegistry<T, I: From<usize>> {
    entries: ir::IndexVec<I, T>,
    seen: HashMap<FnKey, I>,
}

impl<T, I: From<usize> + Copy> FunctionRegistry<T, I> {
    fn new() -> Self {
        Self {
            entries: ir::IndexVec::new(),
            seen: HashMap::new(),
        }
    }

    fn contains(&self, key: &FnKey) -> bool {
        self.seen.contains_key(key)
    }

    fn register(&mut self, key: FnKey, value: T) -> I {
        let id = self.entries.push(value);
        self.seen.insert(key, id);
        id
    }
}

/// A module path like "imports/scoped".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModulePath(String);

impl std::ops::Deref for ModulePath {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ModulePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModulePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ModulePath {
    fn from(s: String) -> Self {
        ModulePath(s)
    }
}

impl From<&str> for ModulePath {
    fn from(s: &str) -> Self {
        ModulePath(s.to_string())
    }
}

impl Borrow<str> for ModulePath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// An effect name like "StartDb" — CamelCase by convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EffectName(String);

impl std::ops::Deref for EffectName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for EffectName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EffectName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for EffectName {
    fn from(s: String) -> Self {
        EffectName(s)
    }
}

impl From<&str> for EffectName {
    fn from(s: &str) -> Self {
        EffectName(s.to_string())
    }
}

impl Borrow<str> for EffectName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

// ─── Table Aliases ──────────────────────────────────────────

type FnTable = LookupTable<FnKey, Located<parser::AstFnDef>>;
type PureFnTable = LookupTable<FnKey, Located<parser::AstPureFnDef>>;
type EffectTable = LookupTable<EffectName, Located<parser::AstEffectDef>>;
type AstTable = LookupTable<ModulePath, Located<parser::AstModule>>;

/// What a module exports: all its fn and effect definitions.
#[derive(Debug, Clone)]
struct ModuleExports {
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
}

/// The resolved scope for a single module: own definitions + imports.
#[derive(Debug)]
struct ModuleScope {
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
}

// ─── Public API ─────────────────────────────────────────────

pub fn resolve(
    project_root: &Path,
    paths: Option<&[PathBuf]>,
    multiplier: f64,
) -> Result<ir::TestSuite, crate::error::DiagnosticReports> {
    use crate::error::{DiagnosticReport, DiagnosticReports};

    let lib_dir = config::lib_dir(project_root);
    let tests_dir = config::tests_dir(project_root);

    let lib_files = discover_relux_files(&lib_dir);

    let (test_files, early_errors) = match paths {
        Some(paths) => discover::resolve_paths(paths, project_root),
        None => (discover_relux_files(&tests_dir), Vec::new()),
    };

    let mut all_files = lib_files;
    all_files.extend(test_files);

    let loader = FsSourceLoader::new(project_root.to_path_buf(), vec![lib_dir.to_path_buf()]);
    let mod_paths: Vec<String> = all_files
        .iter()
        .map(|p| discover::path_to_mod(p, project_root))
        .collect();

    let mut result = resolve_with(&mod_paths, &loader, multiplier);
    result.source_map.project_root = Some(project_root.to_path_buf());

    // Merge early discovery errors into module errors
    let mut all_errors = early_errors;
    all_errors.extend(result.module_errors);

    if !all_errors.is_empty() {
        let errors = all_errors.iter().map(DiagnosticReport::from).collect();
        let warnings = result
            .module_warnings
            .iter()
            .map(DiagnosticReport::from)
            .collect();
        return Err(DiagnosticReports {
            errors,
            warnings,
            source_map: result.source_map,
        });
    }

    // Filter out plans from lib-only modules
    let plan_results = result
        .plan_results
        .into_iter()
        .filter(|pr| match pr {
            PlanResult::Ok { plan, .. } => {
                let source_path = &result.source_map.files[plan.test.span.file].path;
                !source_path.starts_with(&lib_dir)
            }
            PlanResult::Err { .. } => true, // keep errors for reporting
        })
        .collect();

    Ok(ir::TestSuite {
        plan_results,
        source_map: result.source_map,
        warnings: result.module_warnings,
    })
}

pub fn resolve_with(
    root_mod_paths: &[String],
    source_loader: &dyn SourceLoader,
    multiplier: f64,
) -> ResolveResult {
    let (asts, source_map, mut module_errors) = load_modules(root_mod_paths, source_loader);
    let (scopes, scope_errors, module_warnings) = build_scopes(&asts);
    module_errors.extend(scope_errors);
    let plan_results = build_plans(
        root_mod_paths,
        &asts,
        &scopes,
        source_map.files.len(),
        multiplier,
    );
    ResolveResult {
        plan_results,
        source_map,
        module_errors,
        module_warnings,
    }
}

fn load_modules(
    root_mod_paths: &[String],
    source_loader: &dyn SourceLoader,
) -> (AstTable, SourceMap, Vec<DiagnosticError>) {
    let mut ldr = loader::Loader::new(source_loader);
    for mod_path in root_mod_paths {
        ldr.load_module(mod_path, None);
    }
    (ldr.asts, ldr.source_map, ldr.diagnostics)
}

fn build_scopes(
    asts: &AstTable,
) -> (
    LookupTable<ModulePath, ModuleScope>,
    Vec<DiagnosticError>,
    Vec<DiagnosticWarning>,
) {
    let mut scopes = LookupTable::new();
    let mut all_errors = Vec::new();
    let mut all_warnings = Vec::new();
    for (mod_path, located) in asts.iter() {
        let result = scope::build_module_scope(located.file, &located.def, asts);
        all_errors.extend(result.errors);
        all_warnings.extend(result.warnings);
        scopes.insert(mod_path.clone(), result.scope);
    }
    (scopes, all_errors, all_warnings)
}

fn build_plans(
    root_mod_paths: &[String],
    asts: &AstTable,
    scopes: &LookupTable<ModulePath, ModuleScope>,
    file_count: usize,
    multiplier: f64,
) -> Vec<PlanResult> {
    let mut scopes_by_file: ir::IndexVec<FileId, Option<&ModuleScope>> =
        ir::IndexVec::from_elem(None, file_count);
    for (path, located) in asts.iter() {
        if let Some(scope) = scopes.get(path) {
            scopes_by_file[located.file] = Some(scope);
        }
    }

    let mut results = Vec::new();
    for mod_path in root_mod_paths {
        let located = match asts.get(mod_path.as_str()) {
            Some(located) => located,
            None => continue,
        };
        let file_id = located.file;
        let module = &located.def;

        if scopes_by_file[file_id].is_none() {
            continue;
        }

        for item in &module.items {
            if let parser::AstItem::Test { def: test_def, .. } = &item.node {
                results.push(plan::build_plan(
                    file_id,
                    test_def,
                    &item.span,
                    &scopes_by_file,
                    multiplier,
                ));
            }
        }
    }
    results
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct InMemoryLoader {
        modules: HashMap<String, String>,
    }

    impl InMemoryLoader {
        fn new() -> Self {
            Self {
                modules: HashMap::new(),
            }
        }

        fn add(&mut self, mod_path: &str, source: &str) {
            self.modules
                .insert(mod_path.to_string(), source.to_string());
        }

        fn resolve_one(&self, root: &str) -> ResolveResult {
            resolve_with(&[root.to_string()], self, 1.0)
        }

        /// Extract plans from Ok results, panicking if any are Err.
        fn plans_from(result: &ResolveResult) -> Vec<&ir::Plan> {
            result
                .plan_results
                .iter()
                .map(|pr| match pr {
                    PlanResult::Ok { plan, .. } => plan.as_ref(),
                    PlanResult::Err { errors, .. } => {
                        panic!("unexpected plan errors: {errors:?}")
                    }
                })
                .collect()
        }
    }

    impl SourceLoader for InMemoryLoader {
        fn load(&self, mod_path: &str) -> Option<(PathBuf, String)> {
            let source = self.modules.get(mod_path)?;
            let path = PathBuf::from(format!("{mod_path}.relux"));
            Some((path, source.clone()))
        }
    }

    fn error_names(diags: &[DiagnosticError]) -> Vec<&str> {
        diags.iter().map(|d| d.name()).collect()
    }

    /// Collect all errors from plan results.
    fn plan_errors(results: &[PlanResult]) -> Vec<&DiagnosticError> {
        results
            .iter()
            .flat_map(|pr| match pr {
                PlanResult::Err { errors, .. } => errors.iter().collect::<Vec<_>>(),
                PlanResult::Ok { .. } => Vec::new(),
            })
            .collect()
    }

    /// Collect error names from plan results.
    fn plan_error_names(results: &[PlanResult]) -> Vec<&str> {
        plan_errors(results).iter().map(|e| e.name()).collect()
    }

    // ── Module Loading ──

    #[test]
    fn test_load_single_module() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "fn foo() {\n  > echo hello\n}\n\ntest \"basic\" {\n  shell s {\n    foo()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(result.source_map.files.len(), 1);
    }

    #[test]
    fn test_load_with_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { helper }\n\ntest \"t\" {\n  shell s {\n    helper()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(result.source_map.files.len(), 2);
    }

    #[test]
    fn test_diamond_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/shared", "fn shared_fn() {\n  > echo shared\n}\n");
        loader.add(
            "lib/a",
            "import lib/shared { shared_fn }\nfn a_fn() {\n  shared_fn()\n}\n",
        );
        loader.add(
            "lib/b",
            "import lib/shared { shared_fn }\nfn b_fn() {\n  shared_fn()\n}\n",
        );
        loader.add("main", "import lib/a { a_fn }\nimport lib/b { b_fn }\n\ntest \"t\" {\n  shell s {\n    a_fn()\n    b_fn()\n  }\n}\n");
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(result.source_map.files.len(), 4);
    }

    #[test]
    fn test_imported_function_calls_non_imported_sibling() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "lib/m",
            "fn helper() {\n  > echo help\n}\n\nfn caller() {\n  helper()\n}\n",
        );
        loader.add(
            "main",
            "import lib/m { caller }\n\ntest \"t\" {\n  shell s {\n    caller()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].functions.len(),
            2,
            "both caller and its sibling helper should be reachable"
        );
    }

    #[test]
    fn test_imported_function_transitive_sibling_calls() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "lib/m",
            "fn deep() {\n  > echo deep\n}\n\nfn mid() {\n  deep()\n}\n\nfn top() {\n  mid()\n}\n",
        );
        loader.add(
            "main",
            "import lib/m { top }\n\ntest \"t\" {\n  shell s {\n    top()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(
            plans[0].functions.len(),
            3,
            "top, mid, and deep should all be reachable"
        );
    }

    #[test]
    fn test_module_not_found() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "import lib/nonexistent { foo }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(error_names(&result.module_errors).contains(&"ModuleNotFound"));
    }

    #[test]
    fn test_circular_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/a", "import lib/b\nfn a_fn() {\n  > echo a\n}\n");
        loader.add("lib/b", "import lib/a\nfn b_fn() {\n  > echo b\n}\n");
        loader.add(
            "main",
            "import lib/a { a_fn }\n\ntest \"t\" {\n  shell s {\n    a_fn()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(error_names(&result.module_errors).contains(&"CircularImport"));
    }

    // ── Name Resolution ──

    #[test]
    fn test_import_not_exported() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { nonexistent }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(error_names(&result.module_errors).contains(&"ImportNotExported"));
        if let DiagnosticError::ImportNotExported {
            name, module_path, ..
        } = &result.module_errors[0]
        {
            assert_eq!(name, "nonexistent");
            assert_eq!(module_path, "lib/utils");
        }
    }

    #[test]
    fn test_undefined_function_call() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    nonexistent_fn()\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(plan_error_names(&result.plan_results).contains(&"UndefinedName"));
    }

    #[test]
    fn test_function_arity_distinction() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "fn foo() {\n  > echo zero\n}\n\nfn foo(a) {\n  > echo one\n}\n\ntest \"t\" {\n  shell s {\n    foo()\n    foo(\"x\")\n  }\n}\n"
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].functions.len(),
            2,
            "foo/0 and foo/1 should both be resolved"
        );
    }

    #[test]
    fn test_wrong_arity_hints() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "fn foo(a) {\n  > echo one\n}\n\ntest \"t\" {\n  shell s {\n    foo(\"x\", \"y\")\n  }\n}\n"
        );
        let result = loader.resolve_one("main");
        let errors = plan_errors(&result.plan_results);
        let undef = errors
            .iter()
            .find(|d| matches!(d, DiagnosticError::UndefinedName { .. }));
        assert!(undef.is_some(), "expected UndefinedName diagnostic");
        if let DiagnosticError::UndefinedName {
            available_arities, ..
        } = undef.unwrap()
        {
            assert_eq!(available_arities, &[1], "should hint that foo/1 exists");
        }
    }

    // ── Effect Graph ──

    #[test]
    fn test_effect_deduplication() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "effect StartDb -> db {\n  shell db {\n    > start db\n  }\n}\n\ntest \"t\" {\n  need StartDb as db1\n  need StartDb as db2\n  shell db1 {\n    > query 1\n  }\n}\n"
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].effect_graph.dag.node_count(), 1);
        assert_eq!(plans[0].test.needs.len(), 2);
        let inst0 = plans[0].test.needs[0].node.instance;
        let inst1 = plans[0].test.needs[1].node.instance;
        assert_eq!(
            inst0, inst1,
            "both needs should resolve to the same instance"
        );
    }

    #[test]
    fn test_need_without_alias_has_no_shell_binding() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "effect StartDb -> db {\n  shell db {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartDb\n  shell db {\n    > query\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert!(
            plans[0].test.needs[0].node.alias.is_none(),
            "bare need (no `as`) should have no alias"
        );
    }

    #[test]
    fn test_need_with_alias_has_shell_binding() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "effect StartDb -> db {\n  shell db {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartDb as mydb\n  shell mydb {\n    > query\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        let alias = plans[0].test.needs[0]
            .node
            .alias
            .as_ref()
            .expect("explicit alias should be Some");
        assert_eq!(alias.node, "mydb");
    }

    #[test]
    fn test_effect_different_overlay_different_instance() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "effect StartSvc -> svc {\n  shell svc {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartSvc as s1 {\n    PORT = \"8080\"\n  }\n  need StartSvc as s2 {\n    PORT = \"9090\"\n  }\n  shell s1 {\n    > query\n  }\n}\n"
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(
            plans[0].effect_graph.dag.node_count(),
            2,
            "different overlays → 2 instances"
        );
    }

    #[test]
    fn test_overlay_identity_structural_equality() {
        let mut loader = InMemoryLoader::new();
        // Two needs with the same call-expression overlay at different source
        // positions should deduplicate. Debug-based identity breaks this because
        // CallExpr contains Spanned fields whose byte offsets differ by position.
        loader.add(
            "main",
            "pure fn tag() {\n  let t = \"v1\"\n}\n\neffect E -> s {\n  shell s {\n    > echo ${TAG}\n  }\n}\n\ntest \"t\" {\n  need E as s1 {\n    TAG = tag()\n  }\n  need E as s2 {\n    TAG = tag()\n  }\n  shell s1 {\n    > echo hi\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(
            plans[0].effect_graph.dag.node_count(),
            1,
            "identical call-expression overlays should deduplicate to one instance"
        );
    }

    #[test]
    fn test_undefined_effect() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  need NonexistentEffect as x\n  shell x {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(plan_error_names(&result.plan_results).contains(&"UndefinedName"));
    }

    // ── Lowering ──

    #[test]
    fn test_timeout_parsing() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~10s\n    > echo\n    ~500ms\n    > echo2\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => assert_eq!(
                *t,
                ir::Timeout::Tolerance {
                    duration: Duration::from_secs(10),
                    multiplier: 1.0
                }
            ),
            other => panic!("expected Timeout, got {other:?}"),
        }
        match &stmts[2].node {
            ir::ShellStmt::Timeout(t) => assert_eq!(
                *t,
                ir::Timeout::Tolerance {
                    duration: Duration::from_millis(500),
                    multiplier: 1.0
                }
            ),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~10xyz\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(plan_error_names(&result.plan_results).contains(&"InvalidTimeout"));
    }

    #[test]
    fn test_inline_test_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" ~5s {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(
            plans[0].test.timeout,
            Some(ir::Timeout::Tolerance {
                duration: Duration::from_secs(5),
                multiplier: 1.0
            })
        );
    }

    #[test]
    fn test_no_inline_test_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add("main", "test \"t\" {\n  shell s {\n    > echo\n  }\n}\n");
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert!(plans[0].test.timeout.is_none());
    }

    #[test]
    fn test_comments_stripped_from_ir() {
        let mut loader = InMemoryLoader::new();
        loader.add("main", "// top comment\ntest \"t\" {\n  // test comment\n  shell s {\n    // shell comment\n    > echo\n  }\n}\n");
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        for stmt in stmts {
            assert!(
                !matches!(&stmt.node, ir::ShellStmt::Expr(ir::Expr::Var(s)) if s.starts_with('#')),
                "comments should be stripped from IR"
            );
        }
    }

    // ── Error Spans ──

    #[test]
    fn test_error_span_points_to_correct_file() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { nonexistent }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        let import_err = result
            .module_errors
            .iter()
            .find(|d| matches!(d, DiagnosticError::ImportNotExported { .. }));
        assert!(import_err.is_some());
        if let DiagnosticError::ImportNotExported { span, .. } = import_err.unwrap() {
            let file = &result.source_map.files[span.file];
            assert!(
                file.path.to_string_lossy().contains("main"),
                "error should point to main module"
            );
            let text = &file.source[span.range.clone()];
            assert_eq!(text, "nonexistent", "span should cover the undefined name");
        }
    }

    #[test]
    fn test_undefined_name_span_accuracy() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    missing_fn(\"a\")\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        let errors = plan_errors(&result.plan_results);
        let undef = errors
            .iter()
            .find(|d| matches!(d, DiagnosticError::UndefinedName { .. }));
        assert!(undef.is_some());
        if let DiagnosticError::UndefinedName { span, .. } = undef.unwrap() {
            let file = &result.source_map.files[span.file];
            let text = &file.source[span.range.clone()];
            assert_eq!(text, "missing_fn", "span should cover the function name");
        }
    }

    // ── Sub-need scope resolution ──

    #[test]
    fn test_effect_sub_need_resolved_in_home_scope() {
        let mut loader = InMemoryLoader::new();
        // lib/base defines effect Base
        loader.add(
            "lib/base",
            "effect Base -> s {\n  shell s {\n    > echo base\n  }\n}\n",
        );
        // lib/composite imports Base from lib/base and needs it
        loader.add(
            "lib/composite",
            "import lib/base { Base }\n\neffect Composite -> s {\n  need Base as dep\n  shell s {\n    > echo composite\n  }\n}\n",
        );
        // Test file imports ONLY Composite — does NOT import Base
        loader.add(
            "main",
            "import lib/composite { Composite }\n\ntest \"t\" {\n  need Composite as c\n  shell c {\n    > echo test\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "sub-need Base should resolve in lib/composite's scope, not main's scope: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1);
        assert!(
            plans[0].effect_graph.dag.node_count() >= 2,
            "should have both Base and Composite effect instances"
        );
    }

    // ── Missing root module ──

    #[test]
    fn test_missing_root_module_does_not_panic() {
        let loader = InMemoryLoader::new(); // empty — no modules
        let result = resolve_with(&["nonexistent".to_string()], &loader, 1.0);
        assert!(
            error_names(&result.module_errors).contains(&"RootNotFound"),
            "should produce RootNotFound diagnostic, not panic: {:?}",
            result.module_errors
        );
    }

    // ── Marker sub-expression spans ──

    #[test]
    fn test_marker_undefined_call_span_accuracy() {
        let source = "# skip if nonexistent_fn()\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n";
        let mut loader = InMemoryLoader::new();
        loader.add("main", source);
        let result = loader.resolve_one("main");
        let errors = plan_errors(&result.plan_results);
        let undef = errors
            .iter()
            .find(|d| matches!(d, DiagnosticError::UndefinedName { .. }));
        assert!(undef.is_some(), "expected UndefinedName for nonexistent_fn");
        if let DiagnosticError::UndefinedName { span, .. } = undef.unwrap() {
            let file = &result.source_map.files[span.file];
            let text = &file.source[span.range.clone()];
            assert_eq!(
                text, "nonexistent_fn",
                "span should cover the function name inside the marker"
            );
        }
    }

    // ── Integration: syntax_demo-style ──

    #[test]
    fn test_multi_module_resolve() {
        let mut loader = InMemoryLoader::new();

        loader.add(
            "lib/module1",
            r#"fn function1() {
  > echo f1
}
fn function2() {
  > echo f2
}
fn function3() {
  > echo f3
}
effect Effect1 -> e1shell {
  shell e1shell {
    > start e1
  }
}
effect Effect2 -> e2shell {
  shell e2shell {
    > start e2
  }
}
effect Effect3 -> e3shell {
  shell e3shell {
    > start e3
  }
}
"#,
        );
        loader.add(
            "lib/module2",
            r#"fn mod2_fn() {
  > echo mod2
}
"#,
        );
        loader.add(
            "main",
            r#"import lib/module1 {
  function1, function2, function3 as f3,
  Effect1, Effect2, Effect3 as E3,
}
import lib/module2

fn some_function(arg1, arg2) {
  > echo ${arg1} ${arg2}
}

fn match_uuid() {
  <? ([0-9a-f-]+)
  $1
}

effect StartSomething -> something {
  need Effect1 as e1
  need Effect2 as e2
  need E3 as e3 {
    E3_VAR = "value"
  }
  let some_important_var
  shell e3 {
    > some command
    <? match (\d+)
    some_important_var = $1
  }
  shell something {
    some_function("a", "b")
  }
  cleanup {
    let flags = "--graceful"
    > shutdown ${flags}
  }
}

test "Some test" {
  """
  The test description
  """
  need StartSomething as something_shell
  need StartSomething as another_something_shell {
    E3_VAR = "another value"
  }
  let global_test_var
  shell myshell {
    ~10s
    !? [Ee]rror|FATAL|panic
    let variable = "always-string"
    let global_test_var = "new value"
    > echo ${variable}
    <? always-string
    ~120s
    > ./long_running_command
    <? completed
    != error
  }
  shell something_shell {
    let result = some_function("arg1", "arg2")
    let id = match_uuid()
    > curl localhost:8080/resource/${id}
    <? 200
  }
  cleanup {
    > rm -f /tmp/test_artifacts
  }
}
"#,
        );

        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(plans.len(), 1, "one test → one plan");
        assert!(
            result.source_map.files.len() >= 3,
            "main + 2 imported modules"
        );

        let plan = &plans[0];
        assert_eq!(plan.test.name.node, "Some test");
        assert!(plan.test.doc.is_some());
        assert_eq!(plan.test.needs.len(), 2);
        assert!(plan.test.shells.len() >= 2);
        assert!(plan.test.cleanup.is_some());

        assert!(
            plan.effect_graph.dag.node_count() >= 2,
            "at least 2 distinct effect instances (different overlays on StartSomething)"
        );
        assert!(!plan.effects.is_empty());
        assert!(
            !plan.functions.is_empty(),
            "some_function and match_uuid should be reachable"
        );
    }

    #[test]
    fn test_timed_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~2s? regex\n    <~500ms= literal\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) => {
                assert_eq!(
                    m.timeout_override,
                    Some(ir::Timeout::Tolerance {
                        duration: Duration::from_secs(2),
                        multiplier: 1.0
                    })
                );
            }
            other => panic!("expected MatchRegex with timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) => {
                assert_eq!(
                    m.timeout_override,
                    Some(ir::Timeout::Tolerance {
                        duration: Duration::from_millis(500),
                        multiplier: 1.0
                    })
                );
            }
            other => panic!("expected MatchLiteral with timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_timed_match_invalid_duration() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~0xyz? regex\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(plan_error_names(&result.plan_results).contains(&"InvalidTimeout"));
    }

    #[test]
    fn test_timed_match_regex_pattern_span_accuracy() {
        // Timed match operators like `<~2s? hello world` have variable-length
        // prefixes. The pattern span must cover only "hello world", not the prefix.
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~2s? hello world\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let shell = &plans[0].test.shells[0].node;
        if let ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) = &shell.stmts[0].node {
            let file = &result.source_map.files[m.pattern.span.file];
            let text = &file.source[m.pattern.span.range.clone()];
            assert_eq!(
                text, "hello world",
                "span should cover the pattern content, not the operator prefix"
            );
        } else {
            panic!("expected MatchRegex");
        }
    }

    #[test]
    fn test_timed_match_literal_pattern_span_accuracy() {
        // Same for timed literal match with longer duration string
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~500ms= hello world\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let shell = &plans[0].test.shells[0].node;
        if let ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) = &shell.stmts[0].node {
            let file = &result.source_map.files[m.pattern.span.file];
            let text = &file.source[m.pattern.span.range.clone()];
            assert_eq!(
                text, "hello world",
                "span should cover the pattern content, not the operator prefix"
            );
        } else {
            panic!("expected MatchLiteral");
        }
    }

    // ── Assertion timeouts ──

    #[test]
    fn test_assertion_timeout_in_shell() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    @5s\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => {
                assert_eq!(*t, ir::Timeout::Assertion(Duration::from_secs(5)))
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_assertion_timeout_on_test() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" @3s {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        assert_eq!(
            plans[0].test.timeout,
            Some(ir::Timeout::Assertion(Duration::from_secs(3)))
        );
    }

    #[test]
    fn test_assertion_timed_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <@2s? regex\n    <@500ms= literal\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) => {
                assert_eq!(
                    m.timeout_override,
                    Some(ir::Timeout::Assertion(Duration::from_secs(2)))
                );
            }
            other => panic!("expected MatchRegex with assertion timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) => {
                assert_eq!(
                    m.timeout_override,
                    Some(ir::Timeout::Assertion(Duration::from_millis(500)))
                );
            }
            other => panic!("expected MatchLiteral with assertion timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_multiplier_does_not_affect_assertion_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    @5s\n    > echo\n  }\n}\n",
        );
        let result = resolve_with(&["main".to_string()], &loader, 3.0);
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => {
                assert_eq!(*t, ir::Timeout::Assertion(Duration::from_secs(5)));
                assert_eq!(t.resolve(), Duration::from_secs(5));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_multiplier_scales_tolerance_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~5s\n    > echo\n  }\n}\n",
        );
        let result = resolve_with(&["main".to_string()], &loader, 2.0);
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => {
                assert_eq!(
                    *t,
                    ir::Timeout::Tolerance {
                        duration: Duration::from_secs(5),
                        multiplier: 2.0
                    }
                );
                assert_eq!(t.resolve(), Duration::from_secs(10));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    // ── Condition markers ──

    #[test]
    fn test_marker_lowering_test() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "# skip unless CI\n# run if OS = \"linux\"\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let plan = &plans[0];
        assert_eq!(plan.test.conditions.len(), 2);
        assert!(matches!(
            plan.test.conditions[0].node.kind,
            ir::CondKind::Skip
        ));
        let c0 = plan.test.conditions[0].node.cond.as_ref().unwrap();
        assert!(matches!(c0.modifier, ir::CondModifier::Unless));
        assert!(matches!(c0.body, ir::CondBody::Bare(_)));
        assert!(matches!(
            plan.test.conditions[1].node.kind,
            ir::CondKind::Run
        ));
        let c1 = plan.test.conditions[1].node.cond.as_ref().unwrap();
        assert!(matches!(c1.modifier, ir::CondModifier::If));
        assert!(matches!(c1.body, ir::CondBody::Eq(_, _)));
    }

    #[test]
    fn test_marker_lowering_effect() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            concat!(
                "# skip unless PLATFORM ? ^linux\n",
                "effect E -> s {\n",
                "  shell s {\n    > start\n  }\n",
                "}\n",
                "test \"t\" {\n  need E as e\n  shell e {\n    > hi\n  }\n}\n",
            ),
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let plan = &plans[0];
        let effect = &plan.effects[ir::EffectId::from(0)];
        assert_eq!(effect.conditions.len(), 1);
        assert!(matches!(effect.conditions[0].node.kind, ir::CondKind::Skip));
        let c0 = effect.conditions[0].node.cond.as_ref().unwrap();
        assert!(matches!(c0.body, ir::CondBody::Regex(_, _)));
    }

    #[test]
    fn test_bare_marker_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "# skip\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let plan = &plans[0];
        assert_eq!(plan.test.conditions.len(), 1);
        assert!(matches!(
            plan.test.conditions[0].node.kind,
            ir::CondKind::Skip
        ));
        assert!(plan.test.conditions[0].node.cond.is_none());
    }

    #[test]
    fn test_marker_invalid_regex() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "# skip unless FOO ? *invalid\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(plan_error_names(&result.plan_results).contains(&"InvalidRegex"));
    }

    // ── Escape Interpretation ──
    // Escapes are resolved at the lexer boundary: literal contexts interpret
    // escapes (e.g. \n → newline), regex contexts keep them verbatim.
    // Invalid escapes are caught before parsing via check_literal_invalid_escapes.

    #[test]
    fn test_escape_interpreted_in_literal_context() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    > echo\\thello\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let shell = &plans[0].test.shells[0].node;
        if let ir::ShellStmt::Expr(ir::Expr::Send(expr)) = &shell.stmts[0].node {
            let combined: String = expr
                .parts
                .iter()
                .filter_map(|p| match &p.node {
                    ir::StringPart::Literal(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                combined.contains('\t'),
                "expected tab in send payload, got: {combined:?}"
            );
        } else {
            panic!("expected Send statement");
        }
    }

    #[test]
    fn test_escape_verbatim_in_regex_context() {
        let mut loader = InMemoryLoader::new();
        loader.add("main", "test \"t\" {\n  shell s {\n    <? \\d+\n  }\n}\n");
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let shell = &plans[0].test.shells[0].node;
        if let ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) = &shell.stmts[0].node {
            let combined: String = m
                .pattern
                .parts
                .iter()
                .filter_map(|p| match &p.node {
                    ir::StringPart::Literal(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                combined.contains("\\d"),
                "expected verbatim \\d in regex, got: {combined:?}"
            );
        } else {
            panic!("expected MatchRegex statement");
        }
    }

    #[test]
    fn test_unknown_escape_in_literal_is_error() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    > hello\\dworld\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            error_names(&result.module_errors).contains(&"Parse"),
            "expected Parse error for invalid escape, got: {:?}",
            error_names(&result.module_errors)
        );
    }

    #[test]
    fn test_unknown_escape_in_string_is_error() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    let x = \"hello\\dworld\"\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            error_names(&result.module_errors).contains(&"Parse"),
            "expected Parse error for invalid escape, got: {:?}",
            error_names(&result.module_errors)
        );
    }

    #[test]
    fn test_unknown_escape_in_regex_no_error() {
        let mut loader = InMemoryLoader::new();
        loader.add("main", "test \"t\" {\n  shell s {\n    <? \\d+\n  }\n}\n");
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
    }

    #[test]
    fn test_all_supported_escapes() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    let a = \"\\n\\t\\r\\\\\\\"\\0\\a\\b\\f\\v\\e\"\n  }\n}\n",
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let shell = &plans[0].test.shells[0].node;
        if let ir::ShellStmt::Let(decl) = &shell.stmts[0].node {
            if let Some(val) = &decl.value {
                if let ir::Expr::String(expr) = &val.node {
                    let combined: String = expr
                        .parts
                        .iter()
                        .filter_map(|p| match &p.node {
                            ir::StringPart::Literal(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(combined, "\n\t\r\\\"\0\x07\x08\x0C\x0B\x1B");
                } else {
                    panic!("expected String expr");
                }
            } else {
                panic!("expected initialized let");
            }
        } else {
            panic!("expected Let statement");
        }
    }

    #[test]
    fn test_unreachable_effect_functions_not_in_plan() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            concat!(
                "fn used_fn() {\n  > echo used\n}\n",
                "fn unused_fn() {\n  > echo unused\n}\n",
                "effect UsedEffect -> s {\n  shell s {\n    used_fn()\n  }\n}\n",
                "effect UnusedEffect -> s {\n  shell s {\n    unused_fn()\n  }\n}\n",
                "test \"t\" {\n  need UsedEffect as s\n  shell s {\n    > echo hi\n  }\n}\n",
            ),
        );
        let result = loader.resolve_one("main");
        assert!(
            result.module_errors.is_empty(),
            "unexpected errors: {:?}",
            result.module_errors
        );
        let plans = InMemoryLoader::plans_from(&result);
        let fn_names: Vec<&str> = plans[0]
            .functions
            .iter()
            .map(|f| f.name.node.as_str())
            .collect();
        assert!(
            fn_names.contains(&"used_fn"),
            "used_fn should be in the plan"
        );
        assert!(
            !fn_names.contains(&"unused_fn"),
            "unused_fn should NOT be in the plan — UnusedEffect is not reachable"
        );
    }

    #[test]
    fn test_circular_effect_dep_produces_no_plan() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            concat!(
                "effect A -> s {\n  need B as dep\n  shell s {\n    > echo a\n  }\n}\n",
                "effect B -> s {\n  need A as dep\n  shell s {\n    > echo b\n  }\n}\n",
                "test \"t\" {\n  need A as a\n  shell a {\n    > echo hi\n  }\n}\n",
            ),
        );
        let result = loader.resolve_one("main");
        let errors = plan_errors(&result.plan_results);
        let error_names: Vec<&str> = errors.iter().map(|e| e.name()).collect();
        assert!(
            error_names.contains(&"CircularEffectDependency"),
            "should detect circular dependency: {error_names:?}"
        );
        // No valid plans should be emitted — the cycle makes the plan invalid
        let valid_plans: Vec<_> = result
            .plan_results
            .iter()
            .filter(|pr| matches!(pr, PlanResult::Ok { .. }))
            .collect();
        assert!(
            valid_plans.is_empty(),
            "plans with circular effect dependencies should not be emitted"
        );
    }
}
