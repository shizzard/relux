use std::fs;
use std::process;
use std::sync::Arc;

use crate::history::LatestRun;
use relux_core::config;
use relux_core::diagnostics::ModulePath;
use relux_core::pure::Env;
use relux_core::pure::LayeredEnv;
use relux_ir::IrTimeout;
use relux_resolver::resolve;
use relux_runtime::RunContext;
use relux_runtime::RunStrategy;
use relux_runtime::report::result::Outcome;

use super::build_source_loader;
use super::resolve_project;
use super::resolve_test_paths;

fn generate_run_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bs58::encode(bytes).into_string().chars().take(10).collect()
}

pub async fn cmd_run(matches: &clap::ArgMatches) {
    let (project_root, cfg) = resolve_project(matches);

    let rerun = matches.get_flag("rerun");
    let test_paths = if rerun {
        match LatestRun::load(&project_root) {
            Ok(run) => {
                let paths = run.non_pass_paths();
                if paths.is_empty() {
                    eprintln!("nothing to rerun: all tests passed in the latest run");
                    return;
                }
                paths.into_iter().map(ModulePath).collect()
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    } else {
        resolve_test_paths(matches, &project_root)
    };

    let multiplier: f64 = *matches
        .get_one("multiplier")
        .expect("clap default guarantees presence");

    let flaky_config = {
        let mut fc = cfg.flaky.clone();
        if let Some(&retries) = matches.get_one::<u32>("flaky-retries") {
            fc.max_retries = retries;
        }
        if let Some(&m) = matches.get_one::<f64>("flaky-multiplier") {
            fc.timeout_multiplier = m;
        }
        fc
    };

    let test_names: Option<Vec<String>> = if rerun {
        None
    } else {
        matches
            .get_many::<String>("test")
            .map(|v| v.cloned().collect())
    };
    if test_names.is_some() && test_paths.len() != 1 {
        eprintln!("error: --test requires exactly one --file");
        process::exit(1);
    }

    let loader = build_source_loader(&project_root);
    let env = Arc::new(LayeredEnv::from(Env::capture()));

    let suite_name = cfg.name.clone().unwrap_or_default();
    let mut suite = resolve(
        &*loader,
        suite_name,
        test_paths,
        env,
        multiplier,
        &project_root,
    );

    if let Some(ref names) = test_names {
        Arc::get_mut(&mut suite.plans)
            .expect("suite is not shared yet")
            .retain(|plan| names.iter().any(|n| n == plan.meta().name()));
    }

    #[cfg(feature = "interactive-debugger")]
    if matches.get_flag("debug") {
        let port = *matches.get_one::<u16>("port").expect("has default");
        let log_level = *matches
            .get_one::<relux_debug::LogLevel>("log-level")
            .expect("has default");
        let config = relux_debug::DebugConfig { port, log_level };
        relux_debug::start_debug_session(suite.clone(), config).await;
        return;
    }

    let strategy = match matches.get_one::<String>("strategy").map(|s| s.as_str()) {
        Some("fail-fast") => RunStrategy::FailFast,
        _ => RunStrategy::All,
    };
    let progress = match matches.get_one::<String>("progress").map(|s| s.as_str()) {
        Some("plain") => relux_runtime::ProgressMode::Plain,
        Some("tui") => relux_runtime::ProgressMode::Tui,
        _ => relux_runtime::ProgressMode::Auto,
    };

    let run_id = generate_run_id();
    let timestamp = chrono::Utc::now().format("%Y-%m-%d-%H-%M-%S").to_string();
    let out_dir = config::out_dir(&project_root);
    let run_dir = out_dir.join(format!("run-{timestamp}-{run_id}"));
    let artifacts_dir = run_dir.join("artifacts");
    let _ = fs::create_dir_all(&artifacts_dir);

    // Update latest symlink
    let latest = out_dir.join("latest");
    let _ = fs::remove_file(&latest);
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(&run_dir, &latest);
    }

    let default_timeout = IrTimeout::tolerance_scaled(cfg.timeout.match_timeout, multiplier);

    let test_timeout = {
        let d = matches
            .get_one::<String>("test-timeout")
            .map(|s| {
                humantime::parse_duration(s).unwrap_or_else(|e| {
                    eprintln!("error: invalid --test-timeout: {e}");
                    process::exit(1);
                })
            })
            .unwrap_or(cfg.timeout.test);
        IrTimeout::tolerance_scaled(d, multiplier)
    };

    let suite_timeout = {
        let d = matches
            .get_one::<String>("suite-timeout")
            .map(|s| {
                humantime::parse_duration(s).unwrap_or_else(|e| {
                    eprintln!("error: invalid --suite-timeout: {e}");
                    process::exit(1);
                })
            })
            .unwrap_or(cfg.timeout.suite);
        // Unlike test/match timeouts which use IrTimeout (tolerance vs assertion
        // distinction, flaky retry scaling), the suite timeout is a plain Duration
        // used as a hard watchdog deadline. We apply the multiplier directly.
        d.mul_f64(multiplier)
    };

    let jobs = matches
        .get_one::<usize>("jobs")
        .copied()
        .unwrap_or(cfg.run.jobs)
        .max(1);

    let run_ctx = RunContext {
        run_id: run_id.clone(),
        run_dir: run_dir.clone(),
        artifacts_dir: artifacts_dir.clone(),
        project_root: project_root.clone(),
        shell_command: cfg.shell.command.clone(),
        shell_prompt: cfg.shell.prompt.clone(),
        default_timeout,
        test_timeout,
        suite_timeout,
        strategy,
        flaky: flaky_config,
        jobs,
        progress,
    };

    let exec = relux_runtime::execute(&suite, &run_ctx).await;
    let results = exec.results;

    // Summary
    let total_duration: std::time::Duration = results.iter().map(|r| r.duration).sum();
    relux_runtime::report::run_summary::write_run_summary(
        &run_dir,
        &run_id,
        &results,
        total_duration,
    );

    // Report
    let report = relux_runtime::report::result::RunReport {
        results: &results,
        run_dir: &run_dir,
        wall_duration: exec.wall_duration,
        jobs,
    };
    report.eprint();

    // HTML run summary (index.html)
    relux_runtime::report::html::generate_run_summary(&run_dir, &results);

    // Source pages (copy + syntax-highlighted HTML)
    relux_runtime::report::html::generate_source_pages(
        &run_dir,
        &suite.tables.sources,
        &project_root,
    );

    // Optional artifact formats
    let suite_name = cfg.name.as_deref().unwrap_or("relux");
    if matches.get_flag("tap") {
        relux_runtime::report::tap::generate_tap(
            &run_dir,
            suite_name,
            &results,
            &suite.tables.sources,
        );
    }
    if matches.get_flag("junit") {
        relux_runtime::report::junit::generate_junit(
            &run_dir,
            suite_name,
            &results,
            &suite.tables.sources,
        );
    }

    let has_problems = results
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Fail(_) | Outcome::Invalid(_)));
    if has_problems {
        process::exit(1);
    }
}
