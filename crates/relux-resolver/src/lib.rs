mod discover;
pub mod dotenv;
pub mod env;
pub mod loader;
pub mod lower;

pub use discover::discover_test_modules;
pub use loader::load_modules;

use std::path::PathBuf;
use std::sync::Arc;

use relux_core::diagnostics::Cause;
use relux_core::diagnostics::ModulePath;
use relux_core::pure::LayeredEnv;
use relux_ir::Plan;
use relux_ir::Suite;

use crate::env::DotenvError;

// --- Source Loader ---------------------------------------

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

// --- Public API ------------------------------------------

/// Resolve a suite: load + lower every reachable module, attach each test's
/// `.env` stack (`Base -> DotEnv...`), then decide every test's markers
/// against its own stack. Returns the decided `Suite` plus any malformed-
/// `.env` errors (kept off `Suite` itself: `relux-ir` cannot depend on
/// `relux-resolver`'s `DotenvError` without a dependency cycle, since
/// `relux-resolver` already depends on `relux-ir`).
pub fn resolve(
    source_loader: &dyn SourceLoader,
    test_paths: Vec<ModulePath>,
    env: Arc<LayeredEnv>,
    multiplier: f64,
    project_root: &std::path::Path,
) -> (Suite, Vec<DotenvError>) {
    use relux_ir::build_all_plans;
    use relux_ir::lowering_context::LoweringContext;

    let causes = relux_core::diagnostics::CauseTable::default();
    let warnings = relux_core::diagnostics::WarningTable::default();
    let (ast_table, source_table) = load_modules(source_loader, test_paths, &causes, &warnings);
    let mut ctx = LoweringContext::new(ast_table, source_table, env, causes, warnings, multiplier);
    ctx.register_bifs();
    let plans = build_all_plans(&mut ctx);
    let mut suite = ctx.into_suite(plans);

    let dotenv_errors = crate::env::attach_dotenv(&mut suite, project_root)
        .err()
        .unwrap_or_default();
    // The per-test decision pass registers skip/invalid causes into
    // `suite.causes`, so diagnostics are printed only after it runs - once
    // every cause a `Runnable` plan's decision can produce actually exists.
    // Printing before deciding would miss every marker-decision cause.
    decide_suite(&mut suite);
    print_diagnostics(&suite, Some(project_root));

    (suite, dotenv_errors)
}

/// Print every registered warning/cause as an ariadne diagnostic, mirroring
/// `LoweringContext::print_diagnostics` but reading off the already-built
/// `Suite` (whose `causes`/`warnings`/`tables` are the same `SharedTable`s the
/// lowering context populated, plus whatever the decision pass added since).
fn print_diagnostics(suite: &Suite, project_root: Option<&std::path::Path>) {
    use relux_core::diagnostics::Diagnostic;

    for (warning_id, warning) in suite.warnings.as_vec() {
        let diagnostic = Diagnostic::from(warning);
        diagnostic.eprint_with_id(&warning_id, &suite.tables.sources, project_root);
    }
    for (cause_id, cause) in suite.causes.as_vec() {
        let diagnostic = Diagnostic::from(cause);
        diagnostic.eprint_with_id(&cause_id, &suite.tables.sources, project_root);
    }
}

/// Per-test marker decision pass: for every `Plan::Runnable`, walk its
/// reachable defs and decide-or-reuse their markers against the plan's own
/// `env` (memoizing into `suite.tables.marker_decisions`), then fold the
/// aggregate into a final Skipped/flaky/Runnable verdict. A decision-time
/// error (e.g. an invalid interpolated regex) narrows the plan to `Invalid`
/// instead. Mutates `suite.plans` and `suite.causes` in place; does not touch
/// plans that are already `Skipped`/`Invalid` (e.g. a lowering-time error).
///
/// Exposed so `resolve()`'s callers and the in-memory test helpers (which
/// build a `Suite` directly from `build_all_plans` without going through
/// `.env` resolution) can share the exact same decision logic.
pub fn decide_suite(suite: &mut Suite) {
    let mut plans = std::mem::take(&mut suite.plans);
    for plan in plans.iter_mut() {
        if let Plan::Runnable {
            meta,
            test,
            warnings,
            env,
        } = plan
        {
            match relux_ir::reachability::collect_test_decision(test, meta, &suite.tables, env) {
                Ok(agg) => {
                    if let Some(skip) = agg.skip {
                        let cause_id = skip.cause_id_at(env.stack_hash());
                        suite.causes.insert(cause_id.clone(), Cause::skip(skip));
                        *plan = Plan::Skipped {
                            meta: meta.clone(),
                            causes: vec![cause_id],
                            warnings: std::mem::take(warnings),
                            env: env.clone(),
                        };
                    } else {
                        meta.set_flaky(agg.flaky);
                    }
                }
                Err(bail) => {
                    let cause_id = bail.cause_id();
                    suite
                        .causes
                        .insert(cause_id.clone(), Cause::from_bail(&bail));
                    *plan = Plan::Invalid {
                        meta: meta.clone(),
                        causes: vec![cause_id],
                        warnings: std::mem::take(warnings),
                    };
                }
            }
        }
    }
    suite.plans = plans;
}
