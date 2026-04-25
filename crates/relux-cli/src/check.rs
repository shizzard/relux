use std::process;
use std::sync::Arc;

use relux_core::diagnostics::Cause;
use relux_core::pure::Env;
use relux_core::pure::LayeredEnv;
use relux_ir::Plan;
use relux_resolver::resolve;

use super::build_source_loader;
use super::resolve_project;
use super::resolve_test_paths;

pub fn cmd_check(matches: &clap::ArgMatches) {
    let (project_root, cfg) = resolve_project(matches);
    let test_paths = resolve_test_paths(matches, &project_root);
    let loader = build_source_loader(&project_root);
    let env = Arc::new(LayeredEnv::from(Env::capture()));
    let suite_name = cfg.name.clone().unwrap_or_default();

    let suite = resolve(&*loader, suite_name, test_paths, env, 1.0, &project_root);

    // Diagnostics are already printed inside resolve().
    // Check if any plan is Invalid or any cause is Invalid → exit 1.
    let has_invalid_plan = suite
        .plans
        .iter()
        .any(|p| matches!(p, Plan::Invalid { .. }));
    let has_invalid_cause = suite
        .causes
        .as_vec()
        .into_iter()
        .any(|(_id, cause)| matches!(cause, Cause::Invalid(_)));
    if has_invalid_plan || has_invalid_cause {
        process::exit(1);
    }

    eprintln!("check passed");
}
