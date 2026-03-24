pub mod error;
pub mod ir;

mod discover;
mod loader;
pub(crate) mod lower;

pub use discover::discover_test_modules;
pub use error::{DiagnosticError, DiagnosticWarning};
pub use loader::load_modules;

use std::path::PathBuf;
use std::sync::Arc;

use crate::diagnostics::ModulePath;
use crate::stack::Env;
use ir::Suite;

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
    test_paths: Vec<ModulePath>,
    env: Arc<Env>,
) -> Suite {
    use ir::{AstTable, SourceTable, build_all_plans};
    use lower::LoweringContext;

    let causes = crate::diagnostics::CauseTable::default();
    let warnings = crate::diagnostics::WarningTable::default();
    let (ast_shared, source_shared) = load_modules(source_loader, test_paths, &causes, &warnings);
    let ast_table: AstTable = ast_shared.try_into().expect("ast_table freeze failed");
    let source_table: SourceTable = source_shared
        .try_into()
        .expect("source_table freeze failed");
    let mut ctx = LoweringContext::new(ast_table, source_table, env, causes, warnings);
    ctx.register_bifs();
    let plans = build_all_plans(&mut ctx);
    ctx.print_diagnostics();
    ctx.into_suite(plans)
}
