mod discover;
pub mod loader;
pub mod lower;

pub use discover::discover_test_modules;
pub use loader::load_modules;

use std::path::PathBuf;
use std::sync::Arc;

use relux_core::diagnostics::ModulePath;
use relux_core::pure::LayeredEnv;
use relux_ir::Suite;

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

// ─── Public API ─────────────────────────────────────────────

pub fn resolve(
    source_loader: &dyn SourceLoader,
    name: String,
    test_paths: Vec<ModulePath>,
    env: Arc<LayeredEnv>,
    multiplier: f64,
    project_root: &std::path::Path,
) -> Suite {
    use relux_ir::build_all_plans;
    use relux_ir::lowering_context::LoweringContext;

    let causes = relux_core::diagnostics::CauseTable::default();
    let warnings = relux_core::diagnostics::WarningTable::default();
    let (ast_table, source_table) = load_modules(source_loader, test_paths, &causes, &warnings);
    let mut ctx = LoweringContext::new(ast_table, source_table, env, causes, warnings, multiplier);
    ctx.register_bifs();
    let plans = build_all_plans(&mut ctx);
    ctx.print_diagnostics(Some(project_root));
    ctx.into_suite(name, plans)
}
