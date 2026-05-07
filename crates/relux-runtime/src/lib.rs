use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use std::collections::VecDeque;
use std::sync::OnceLock;

use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

pub(crate) enum CancelReason {
    SuiteTimeout,
    FailFast,
}

use crate::effect::CleanupSource;
use crate::effect::EffectManager;
use crate::effect::Warning;
use crate::effect::registry::EffectRegistry;
use crate::observe::structured::EnvInfo;
use crate::observe::structured::FailureRecord;
use crate::observe::structured::SpanId;
use crate::observe::structured::SpanKind;
use crate::observe::structured::StructuredLogBuilder;
use crate::observe::structured::TestInfo;
use crate::report::result::Failure;
use crate::report::result::Outcome;
use crate::report::result::TestResult;
use crate::vm::Vm;
use crate::vm::context::ExecutionContext;
use crate::vm::context::Scope;
use crate::vm::context::ShellState;
use relux_core::diagnostics::Cause;
use relux_core::diagnostics::CauseId;
use relux_core::diagnostics::CauseTable;
use relux_core::diagnostics::WarningId;
use relux_core::pure::Env;
use relux_core::pure::LayeredEnv;
use relux_core::pure::VarScope;
use relux_core::table::SourceTable;
use relux_ir::IrNode;
use relux_ir::IrTest;
use relux_ir::IrTestItem;
use relux_ir::IrTimeout;
use relux_ir::Plan;
use relux_ir::Suite;

pub mod effect;
pub mod observe;
pub mod report;
pub mod runtime_context;
pub mod vm;

pub use runtime_context::RuntimeContext;
pub use runtime_context::ShellConfig;

use relux_core::config;

// ─── RunStrategy ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStrategy {
    All,
    FailFast,
}

// ─── ProgressMode ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    /// Detect TTY on stderr; use TUI if interactive, plain otherwise.
    Auto,
    /// Always use plain output (result lines only, no cursor control).
    Plain,
    /// Always use TUI (live progress, even if not a TTY).
    Tui,
}

// ─── RunContext ─────────────────────────────────────────────

pub struct RunContext {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub project_root: PathBuf,
    pub shell_command: String,
    pub shell_prompt: String,
    pub default_timeout: IrTimeout,
    pub test_timeout: IrTimeout,
    pub suite_timeout: Duration,
    pub strategy: RunStrategy,
    pub flaky: relux_core::config::FlakyConfig,
    pub jobs: usize,
    pub progress: ProgressMode,
}

// ─── Environment Helpers ────────────────────────────────────

fn build_env(ctx: &RunContext) -> Arc<LayeredEnv> {
    let mut env = Env::capture();
    env.insert("__RELUX_RUN_ID".into(), ctx.run_id.clone());
    env.insert(
        "__RELUX_RUN_ARTIFACTS".into(),
        ctx.artifacts_dir.display().to_string(),
    );
    env.insert("__RELUX_SHELL_PROMPT".into(), ctx.shell_prompt.clone());
    env.insert(
        "__RELUX_SUITE_ROOT".into(),
        ctx.project_root.display().to_string(),
    );
    if let Ok(exe) = std::env::current_exe() {
        env.insert("__RELUX".into(), exe.display().to_string());
    }
    Arc::new(env.into())
}

fn make_test_env(
    base: &Arc<LayeredEnv>,
    test_file: &Path,
    artifacts_dir: &Path,
) -> Arc<LayeredEnv> {
    let mut test_vars = Env::new();
    if let Some(dir) = test_file.parent() {
        test_vars.insert("__RELUX_TEST_ROOT".into(), dir.display().to_string());
    }
    test_vars.insert(
        "__RELUX_TEST_ARTIFACTS".into(),
        artifacts_dir.display().to_string(),
    );
    Arc::new(LayeredEnv::child(base.clone(), test_vars))
}

// ─── Log / Display Helpers ──────────────────────────────────

fn test_log_dir(
    run_dir: &Path,
    source_table: &SourceTable,
    meta: &relux_ir::TestMeta,
    project_root: &Path,
) -> PathBuf {
    let file_id = meta.span().file();
    let source_path = source_table
        .get(file_id)
        .map(|sf| sf.path.clone())
        .unwrap_or_else(|| file_id.path().clone());
    let relative = source_path
        .strip_prefix(project_root)
        .unwrap_or(&source_path);
    run_dir
        .join("logs")
        .join(relative.with_extension(""))
        .join(slugify(meta.name()))
}

fn test_path_from_meta(
    source_table: &SourceTable,
    meta: &relux_ir::TestMeta,
    project_root: &Path,
) -> String {
    let file_id = meta.span().file();
    let source_path = source_table
        .get(file_id)
        .map(|sf| sf.path.clone())
        .unwrap_or_else(|| file_id.path().clone());
    let tests_dir = config::tests_dir(project_root);
    source_path
        .strip_prefix(&tests_dir)
        .unwrap_or(&source_path)
        .display()
        .to_string()
}

/// Format cause/warning IDs as typed groups for test line output.
///
/// Example: ` [invalid: cheap-walrus-0042] [warning: worn-falcon-5678]`
fn format_cause_tags(
    causes: &[CauseId],
    warnings: &[WarningId],
    cause_table: &CauseTable,
) -> String {
    let mut parts = Vec::new();

    let mut invalid_ids = Vec::new();
    let mut skip_ids = Vec::new();
    for id in causes {
        match cause_table.get(id) {
            Some(Cause::Invalid(_)) => invalid_ids.push(id.to_string()),
            Some(Cause::Skip(_)) => skip_ids.push(id.to_string()),
            None => {}
        }
    }

    if !invalid_ids.is_empty() {
        parts.push(format!("[invalid: {}]", invalid_ids.join(", ")));
    }
    if !skip_ids.is_empty() {
        parts.push(format!("[skip: {}]", skip_ids.join(", ")));
    }
    if !warnings.is_empty() {
        let ids: Vec<String> = warnings.iter().map(|w| w.to_string()).collect();
        parts.push(format!("[warning: {}]", ids.join(", ")));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(" "))
    }
}

/// Format a test identifier for display: `path/slugified-name`.
pub fn test_display_id(test_path: &str, test_name: &str) -> String {
    format!("{}/{}", test_path, slugify(test_name))
}

/// Convert a test name to a filesystem-safe slug.
pub fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ─── Execute (Suite Entry Point) ────────────────────────────

pub struct ExecuteResult {
    pub results: Vec<TestResult>,
    pub wall_duration: Duration,
}

pub async fn execute(suite: &Suite, run_ctx: &RunContext) -> ExecuteResult {
    let wall_start = Instant::now();
    let base_env = build_env(run_ctx);
    let jobs = run_ctx.jobs;

    if jobs > 1 {
        eprintln!("\nrunning {} tests ({jobs} workers)", suite.plans.len());
    } else {
        eprintln!("\nrunning {} tests", suite.plans.len());
    }

    let cancel = CancellationToken::new();
    let cancel_reason: Arc<OnceLock<CancelReason>> = Arc::new(OnceLock::new());

    // Spawn suite timeout watchdog
    let watchdog = {
        let timeout = run_ctx.suite_timeout;
        let watchdog_cancel = cancel.clone();
        let watchdog_reason = cancel_reason.clone();
        Some(tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = watchdog_reason.set(CancelReason::SuiteTimeout);
            watchdog_cancel.cancel();
        }))
    };

    // Spawn TUI renderer
    let is_tty = match run_ctx.progress {
        ProgressMode::Auto => std::io::IsTerminal::is_terminal(&std::io::stderr()),
        ProgressMode::Plain => false,
        ProgressMode::Tui => true,
    };
    let (tui_tx, tui_rx) = observe::tui::channel();
    let tui_handle = observe::tui::spawn_tui(
        tui_rx,
        jobs,
        is_tty,
        suite.tables.sources.clone(),
        run_ctx.project_root.clone(),
    );

    // Build shared test queue with original indices for deterministic ordering
    let queue: Arc<std::sync::Mutex<VecDeque<(usize, &Plan)>>> = Arc::new(std::sync::Mutex::new(
        suite.plans.iter().enumerate().collect(),
    ));

    // Spawn N workers as concurrent futures
    let mut worker_futs = Vec::with_capacity(jobs);
    for slot in 0..jobs {
        let ctx = WorkerContext {
            queue: queue.clone(),
            cancel: cancel.clone(),
            cancel_reason: cancel_reason.clone(),
            suite,
            run_ctx,
            base_env: base_env.clone(),
            tui_tx: tui_tx.clone(),
        };
        worker_futs.push(run_worker(ctx, slot));
    }

    // Await all workers concurrently
    let worker_results = futures::future::join_all(worker_futs).await;

    // Drop our copy of tui_tx so the renderer can finish
    drop(tui_tx);
    tui_handle.await.ok();

    // Abort suite timeout watchdog if it's still running
    if let Some(handle) = watchdog {
        handle.abort();
    }

    // Merge and sort results by original plan index
    let mut all_results: Vec<(usize, TestResult)> = worker_results.into_iter().flatten().collect();
    all_results.sort_by_key(|(idx, _)| *idx);
    ExecuteResult {
        results: all_results.into_iter().map(|(_, r)| r).collect(),
        wall_duration: wall_start.elapsed(),
    }
}

struct WorkerContext<'a> {
    queue: Arc<std::sync::Mutex<VecDeque<(usize, &'a Plan)>>>,
    cancel: CancellationToken,
    cancel_reason: Arc<OnceLock<CancelReason>>,
    suite: &'a Suite,
    run_ctx: &'a RunContext,
    base_env: Arc<LayeredEnv>,
    tui_tx: observe::tui::TuiTx,
}

async fn run_worker(ctx: WorkerContext<'_>, slot: usize) -> Vec<(usize, TestResult)> {
    let mut results = Vec::new();
    let mut generation: u64 = 0;
    loop {
        if ctx.cancel.is_cancelled() {
            break;
        }

        let entry = {
            let mut q = ctx.queue.lock().expect("queue lock poisoned");
            q.pop_front()
        };
        let Some((plan_idx, plan)) = entry else {
            break;
        };

        let test_path = test_path_from_meta(
            &ctx.suite.tables.sources,
            plan.meta(),
            &ctx.run_ctx.project_root,
        );

        let result = match plan {
            Plan::Runnable {
                meta,
                test,
                warnings: plan_warnings,
            } => {
                let tags = format_cause_tags(&[], plan_warnings, &ctx.suite.causes);
                let display_id = test_display_id(&test_path, meta.name());
                generation += 1;
                let _ = ctx.tui_tx.send(observe::tui::TuiEvent::TestStarted {
                    slot,
                    test_id: display_id.clone(),
                    generation,
                });

                let mut result = run_test_cancellable(
                    meta,
                    test,
                    ctx.run_ctx,
                    ctx.base_env.clone(),
                    &test_path,
                    &tags,
                    &ctx.cancel,
                    &ctx.suite.tables,
                    &ctx.suite.causes,
                    1.0,
                    slot,
                    &ctx.tui_tx,
                    generation,
                )
                .await;

                // Flaky retry loop
                if meta.flaky()
                    && result.is_failure()
                    && ctx.run_ctx.flaky.max_retries > 0
                    && !ctx.cancel.is_cancelled()
                {
                    let mut retries = 0u32;
                    for retry in 1..=ctx.run_ctx.flaky.max_retries {
                        if ctx.cancel.is_cancelled() {
                            break;
                        }
                        retries += 1;
                        let flaky_m = ctx.run_ctx.flaky.timeout_multiplier.powi(retry as i32);
                        let retry_test_path = format!("{test_path}-flaky-rerun-{retry}");
                        result = run_test_cancellable(
                            meta,
                            test,
                            ctx.run_ctx,
                            ctx.base_env.clone(),
                            &retry_test_path,
                            &tags,
                            &ctx.cancel,
                            &ctx.suite.tables,
                            &ctx.suite.causes,
                            flaky_m,
                            slot,
                            &ctx.tui_tx,
                            generation,
                        )
                        .await;
                        if !result.is_failure() {
                            break;
                        }
                    }
                    result.flaky_retries = retries;
                    result.test_path = test_path.clone();
                }

                // Send finish event and get progress string back
                let (progress_oneshot_tx, progress_oneshot_rx) = tokio::sync::oneshot::channel();
                let result_line = format_result_line(&display_id, &result, &tags);
                let failure = match &result.outcome {
                    Outcome::Fail(f) => Some((f.clone(), result.log_dir.clone())),
                    _ => None,
                };
                let _ = ctx.tui_tx.send(observe::tui::TuiEvent::TestFinished {
                    slot,
                    result_line,
                    failure,
                    progress_tx: progress_oneshot_tx,
                });
                if let Ok(progress) = progress_oneshot_rx.await {
                    result.progress = progress;
                }

                result
            }
            Plan::Skipped {
                meta,
                causes,
                warnings,
            } => {
                let tags = format_cause_tags(causes, warnings, &ctx.suite.causes);
                let display_id = test_display_id(&test_path, meta.name());
                let result_line = format!(
                    "test {display_id}: {}{tags}",
                    colored::Colorize::yellow("skipped")
                );
                let _ = ctx
                    .tui_tx
                    .send(observe::tui::TuiEvent::Skipped { result_line });
                TestResult {
                    test_name: meta.name().to_string(),
                    test_path: test_path.clone(),
                    outcome: Outcome::Skipped("skipped".to_string()),
                    duration: Duration::ZERO,
                    progress: String::new(),
                    log_dir: None,
                    warnings: Vec::new(),
                    flaky_retries: 0,
                }
            }
            Plan::Invalid {
                meta,
                causes,
                warnings,
            } => {
                let tags = format_cause_tags(causes, warnings, &ctx.suite.causes);
                let display_id = test_display_id(&test_path, meta.name());
                let result_line = format!(
                    "test {display_id}: {}{tags}",
                    colored::Colorize::red("INVALID")
                );
                let _ = ctx
                    .tui_tx
                    .send(observe::tui::TuiEvent::Skipped { result_line });
                TestResult {
                    test_name: meta.name().to_string(),
                    test_path: test_path.clone(),
                    outcome: Outcome::Invalid("invalid".to_string()),
                    duration: Duration::ZERO,
                    progress: String::new(),
                    log_dir: None,
                    warnings: Vec::new(),
                    flaky_retries: 0,
                }
            }
        };

        let failed = matches!(result.outcome, Outcome::Fail(_));
        results.push((plan_idx, result));

        if failed && ctx.run_ctx.strategy == RunStrategy::FailFast {
            let _ = ctx.cancel_reason.set(CancelReason::FailFast);
            ctx.cancel.cancel();
            break;
        }
    }

    // Drain remaining queue as skipped
    let skip_reason = if ctx.cancel.is_cancelled() {
        match ctx.cancel_reason.get() {
            Some(CancelReason::FailFast) => "fail fast",
            Some(CancelReason::SuiteTimeout) => "suite timeout",
            None => "cancelled",
        }
    } else {
        return results;
    };

    let remaining: Vec<(usize, &Plan)> = {
        let mut q = ctx.queue.lock().expect("queue lock poisoned");
        q.drain(..).collect()
    };
    for (plan_idx, plan) in remaining {
        let test_path = test_path_from_meta(
            &ctx.suite.tables.sources,
            plan.meta(),
            &ctx.run_ctx.project_root,
        );
        let display_id = test_display_id(&test_path, plan.meta().name());
        let result_line = format!(
            "test {display_id}: {}",
            colored::Colorize::yellow("skipped")
        );
        let _ = ctx
            .tui_tx
            .send(observe::tui::TuiEvent::Skipped { result_line });
        results.push((
            plan_idx,
            TestResult {
                test_name: plan.meta().name().to_string(),
                test_path,
                outcome: Outcome::Skipped(skip_reason.to_string()),
                duration: Duration::ZERO,
                progress: String::new(),
                log_dir: None,
                warnings: Vec::new(),
                flaky_retries: 0,
            },
        ));
    }

    results
}

fn format_result_line(display_id: &str, result: &TestResult, cause_tags: &str) -> String {
    use crate::report::result::format_duration;
    use colored::Colorize;
    let outcome_str = match &result.outcome {
        Outcome::Pass => format!("{}", "ok".green()),
        Outcome::Fail(_) => format!("{}", "FAILED".red()),
        Outcome::Skipped(_) => format!("{}", "skipped".yellow()),
        Outcome::Invalid(_) => format!("{}", "INVALID".red()),
    };
    format!(
        "test {display_id}: {outcome_str} ({}){cause_tags}",
        format_duration(result.duration)
    )
}

/// Run a single test with cancellation support. On cancellation, cleanup
/// still runs and a partial result is returned.
#[allow(clippy::too_many_arguments)]
async fn run_test_cancellable(
    meta: &relux_ir::TestMeta,
    test: &IrTest,
    run_ctx: &RunContext,
    base_env: Arc<LayeredEnv>,
    test_path: &str,
    cause_tags: &str,
    cancel: &CancellationToken,
    tables: &relux_ir::Tables,
    _causes: &CauseTable,
    flaky_timeout_multiplier: f64,
    slot: usize,
    tui_tx: &observe::tui::TuiTx,
    generation: u64,
) -> TestResult {
    // Create a child token for test-level timeout
    let test_cancel = cancel.child_token();

    let effective_timeout = meta
        .timeout()
        .map(|t| t.adjusted_duration_with_flaky(flaky_timeout_multiplier))
        .unwrap_or_else(|| {
            run_ctx
                .test_timeout
                .adjusted_duration_with_flaky(flaky_timeout_multiplier)
        });

    // Spawn test-level timeout watchdog
    let test_watchdog = Some({
        let timeout = effective_timeout;
        let timeout_cancel = test_cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            timeout_cancel.cancel();
        })
    });

    let mut result = run_test(
        meta,
        test,
        run_ctx,
        base_env,
        test_path,
        cause_tags,
        &test_cancel,
        tables,
        flaky_timeout_multiplier,
        slot,
        tui_tx,
        generation,
    )
    .await;

    // Abort test timeout watchdog if it's still running
    if let Some(handle) = test_watchdog {
        handle.abort();
    }

    // If the test was cancelled due to its own timeout (not the parent suite
    // cancel), rewrite the Cancelled failure into a specific timeout message.
    if test_cancel.is_cancelled()
        && !cancel.is_cancelled()
        && matches!(result.outcome, Outcome::Fail(Failure::Cancelled { .. }))
    {
        result.outcome = Outcome::Fail(Failure::Runtime {
            message: format!("test timeout ({effective_timeout:?}) exceeded"),
            span: None,
            shell: None,
        });
    }

    result
}

// ─── Run Test ───────────────────────────────────────────────

/// Create a ProgressTx that forwards events to the TUI renderer tagged with slot.
fn make_tui_progress_tx(
    tui_tx: &observe::tui::TuiTx,
    slot: usize,
    generation: u64,
) -> observe::progress::ProgressTx {
    let (tx, mut rx) = observe::progress::channel();
    let tui_tx = tui_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = tui_tx.send(observe::tui::TuiEvent::Progress {
                slot,
                event,
                generation,
            });
        }
    });
    tx
}

#[allow(clippy::too_many_arguments)]
async fn run_test(
    meta: &relux_ir::TestMeta,
    test: &IrTest,
    run_ctx: &RunContext,
    base_env: Arc<LayeredEnv>,
    test_path: &str,
    _cause_tags: &str,
    cancel: &CancellationToken,
    tables: &relux_ir::Tables,
    flaky_timeout_multiplier: f64,
    slot: usize,
    tui_tx: &observe::tui::TuiTx,
    generation: u64,
) -> TestResult {
    let test_start = Instant::now();
    let source_table = &tables.sources;
    let log_dir = test_log_dir(&run_ctx.run_dir, source_table, meta, &run_ctx.project_root);
    let _ = std::fs::create_dir_all(&log_dir);

    let progress_tx = make_tui_progress_tx(tui_tx, slot, generation);

    let file_id = meta.span().file();
    let source_file = source_table
        .get(file_id)
        .map(|sf| sf.path.clone())
        .unwrap_or_else(|| file_id.path().clone());
    let artifacts_dir = log_dir.join("artifacts");
    let _ = std::fs::create_dir_all(&artifacts_dir);
    let test_env = make_test_env(&base_env, &source_file, &artifacts_dir);
    let mut warnings = Vec::new();

    let shell_config = ShellConfig {
        command: Arc::from(run_ctx.shell_command.as_str()),
        prompt: Arc::from(run_ctx.shell_prompt.as_str()),
        default_timeout: run_ctx.default_timeout.clone(),
    };

    let log = StructuredLogBuilder::new(
        progress_tx.clone(),
        test_start,
        tables.sources.clone(),
        Arc::from(run_ctx.project_root.as_path()),
    );

    // Open the root span for this test. Every emission inside the test body
    // (effect setup, shell block, fn call, cleanup block) is parented on this.
    let test_span = log.open_span(SpanKind::Test, None, Some(meta.span()));
    let test_span_id = test_span.id();

    let rt_ctx = RuntimeContext {
        log: log.clone(),
        shell: shell_config,
        log_dir: Arc::from(log_dir.as_path()),
        tables: tables.clone(),
        env: test_env.clone(),
        cancel: cancel.clone(),
        test_start,
        flaky_timeout_multiplier,
    };

    // Create a per-test EffectManager
    let test_manager = EffectManager::new(Arc::new(EffectRegistry::new()), rt_ctx.clone());

    let outcome = run_test_body(
        meta,
        test,
        &test_manager,
        &mut warnings,
        &rt_ctx,
        test_span_id,
    )
    .await;

    if outcome.is_err() {
        log.emit_failure_progress();
    }

    // Release effects (always runs, even after cancellation)
    let effect_warnings = test_manager.cleanup_all().await;
    warnings.extend(effect_warnings);

    // Drop all remaining ProgressTx holders so the forwarder task can finish.
    drop(test_manager);
    drop(rt_ctx);
    drop(progress_tx);
    let duration = test_start.elapsed();

    test_span.close();

    // Build the structured log. Failure record is a stub for now — commit 4
    // populates it from the outcome with call-stack / buffer-tail / vars.
    let structured = log.build(
        TestInfo {
            name: meta.name().to_string(),
            path: test_path.to_string(),
            outcome: match &outcome {
                Ok(()) => "pass".to_string(),
                Err(_) => "fail".to_string(),
            },
            duration_ms: duration.as_millis() as u64,
        },
        EnvInfo::default(),
        outcome.as_ref().err().map(failure_record_stub),
    );

    let events_json_path = log_dir.join("events.json");
    match serde_json::to_vec_pretty(&structured) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&events_json_path, &bytes) {
                eprintln!(
                    "warning: failed to write {}: {}",
                    events_json_path.display(),
                    e
                );
            }
        }
        Err(e) => {
            eprintln!(
                "warning: failed to serialize structured log for {}: {}",
                events_json_path.display(),
                e
            );
        }
    }

    match outcome {
        Ok(()) => TestResult {
            test_name: meta.name().to_string(),
            test_path: test_path.to_string(),
            outcome: Outcome::Pass,
            duration,
            progress: String::new(),
            log_dir: Some(log_dir),
            warnings,
            flaky_retries: 0,
        },
        Err(failure) => TestResult {
            test_name: meta.name().to_string(),
            test_path: test_path.to_string(),
            outcome: Outcome::Fail(failure),
            duration,
            progress: String::new(),
            log_dir: Some(log_dir),
            warnings,
            flaky_retries: 0,
        },
    }
}

/// Translate a runtime `Failure` into a stub `FailureRecord` with empty
/// convenience fields. Commit 4 fills `call_stack`, `buffer_tail`, and
/// `vars_in_scope` for real.
fn failure_record_stub(failure: &Failure) -> FailureRecord {
    match failure {
        Failure::MatchTimeout { pattern, shell, .. } => FailureRecord::MatchTimeout {
            span: 0,
            event_seq: 0,
            shell: shell.clone(),
            pattern: pattern.clone(),
            call_stack: Vec::new(),
            buffer_tail: String::new(),
            vars_in_scope: Vec::new(),
        },
        Failure::FailPatternMatched {
            pattern,
            matched_line,
            shell,
            ..
        } => FailureRecord::FailPatternMatched {
            span: 0,
            event_seq: 0,
            shell: shell.clone(),
            pattern: pattern.clone(),
            matched_line: matched_line.clone(),
            call_stack: Vec::new(),
            buffer_tail: String::new(),
            vars_in_scope: Vec::new(),
        },
        Failure::ShellExited {
            shell, exit_code, ..
        } => FailureRecord::ShellExited {
            span: 0,
            event_seq: 0,
            shell: shell.clone(),
            exit_code: *exit_code,
            call_stack: Vec::new(),
            buffer_tail: String::new(),
            vars_in_scope: Vec::new(),
        },
        Failure::Runtime { message, shell, .. } => FailureRecord::Runtime {
            span: None,
            event_seq: None,
            shell: shell.clone(),
            message: message.clone(),
            call_stack: Vec::new(),
            vars_in_scope: Vec::new(),
        },
        Failure::Cancelled { shell, .. } => FailureRecord::Cancelled {
            span: None,
            event_seq: None,
            shell: shell.clone(),
            call_stack: Vec::new(),
        },
    }
}

// ─── Run Test Body ──────────────────────────────────────────

async fn run_test_body(
    meta: &relux_ir::TestMeta,
    test: &IrTest,
    manager: &EffectManager,
    warnings: &mut Vec<Warning>,
    rt_ctx: &RuntimeContext,
    test_span: SpanId,
) -> Result<(), Failure> {
    // 1. Create test scope
    let scope = Scope::Test {
        name: meta.name().to_string(),
        vars: Arc::new(TokioMutex::new(VarScope::new())),
        timeout: meta.timeout().cloned(),
    };

    // 2. Evaluate test-level lets into scope (parser enforces lets come before starts)
    for item in test.body() {
        if let IrTestItem::Let { stmt, .. } = item {
            let mut vars = scope.vars().lock().await;
            let value = if let Some(expr) = stmt.value() {
                relux_ir::evaluator::eval_pure_expr(
                    expr,
                    &vars,
                    &rt_ctx.env,
                    &rt_ctx.tables.pure_fns,
                )
            } else {
                String::new()
            };
            vars.insert(stmt.name().name().to_string(), value);
        }
    }

    // 3. Instantiate effects (overlays can now see test-level vars)
    let caller_vars = scope.vars().lock().await.clone();
    let root_env = rt_ctx.env.clone();
    let exported = manager
        .instantiate(test.starts(), &caller_vars, &root_env, test_span)
        .await?;

    // 4. Build shell map from exposed effect shells
    //    Each start returns a map of exposed shells. We store them
    //    keyed by (alias, shell_name) for dot-access resolution.
    let mut shells: HashMap<String, Arc<TokioMutex<Vm>>> = HashMap::new();
    let mut effect_shells: HashMap<String, HashMap<String, Arc<TokioMutex<Vm>>>> = HashMap::new();
    let mut effect_vars: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut reset_seen = HashSet::new();
    for (start, exported) in test.starts().iter().zip(exported) {
        for vm_arc in exported.shells.values() {
            let ptr = Arc::as_ptr(vm_arc) as usize;
            if reset_seen.insert(ptr) {
                vm_arc.lock().await.reset_for_export(scope.clone());
            }
        }
        if let Some(alias) = start.alias() {
            // For backwards compat: if effect exposes exactly one shell,
            // also insert it under the alias name directly
            if exported.shells.len() == 1 {
                let vm_arc = exported.shells.values().next().unwrap().clone();
                let source = vm_arc.lock().await.current_name();
                rt_ctx.log.emit_shell_alias(test_span, alias, &source);
                shells.insert(alias.to_string(), vm_arc);
            }
            effect_shells.insert(alias.to_string(), exported.shells);
            if !exported.vars.is_empty() {
                effect_vars.insert(alias.to_string(), exported.vars);
            }
        }
    }

    // Inject effect-exposed variables into the test scope so they're
    // accessible via ${Alias.var_name} in shell blocks.
    {
        let mut vars = scope.vars().lock().await;
        for (alias, var_map) in &effect_vars {
            for (var_name, value) in var_map {
                vars.insert(format!("{alias}.{var_name}"), value.clone());
            }
        }
    }

    // 5. Walk IrTestItems (lets already evaluated, starts already instantiated)
    let cleanup_block = test.body().iter().find_map(|item| match item {
        IrTestItem::Cleanup { block, span } => Some((block.clone(), span.clone())),
        _ => None,
    });
    let body_result: Result<(), Failure> = async {
        for item in test.body() {
            match item {
                IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => continue,
                IrTestItem::Start { .. } => continue,
                IrTestItem::Let { .. } => continue,
                IrTestItem::Shell { block, .. } => {
                    let switch_span = block.name().span();
                    if let Some(qualifier) = block.qualifier() {
                        // Qualified shell block: alias.shell { ... }
                        let alias = qualifier.name();
                        let shell_name = block.name().name();
                        let display = format!("{alias}.{shell_name}");
                        let block_span = rt_ctx.log.open_span(
                            SpanKind::ShellBlock {
                                shell: display.clone(),
                            },
                            Some(test_span),
                            Some(switch_span),
                        );
                        let block_span_id = block_span.id();
                        rt_ctx.log.emit_shell_switch(block_span_id, &display);
                        let dep = effect_shells.get(alias).ok_or_else(|| Failure::Runtime {
                            message: format!("unknown effect alias `{alias}`"),
                            span: None,
                            shell: None,
                        })?;
                        let vm_arc = dep.get(shell_name).ok_or_else(|| Failure::Runtime {
                            message: format!(
                                "effect alias `{alias}` does not expose shell `{shell_name}`"
                            ),
                            span: None,
                            shell: None,
                        })?;
                        let mut vm = vm_arc.lock().await;
                        let vm_name = vm.current_name();
                        rt_ctx.log.emit_shell_switch(block_span_id, &vm_name);
                        vm.set_block_span(block_span_id);
                        vm.exec_stmts(block.body()).await?;
                        // block_span drops here, closing the span.
                    } else {
                        // Unqualified shell block: shell name { ... }
                        let name = block.name().name().to_string();
                        let block_span = rt_ctx.log.open_span(
                            SpanKind::ShellBlock {
                                shell: name.clone(),
                            },
                            Some(test_span),
                            Some(switch_span),
                        );
                        let block_span_id = block_span.id();
                        rt_ctx.log.emit_shell_switch(block_span_id, &name);
                        if !shells.contains_key(&name) {
                            let shell_state = ShellState::new(name.clone(), None);
                            let ctx = ExecutionContext::new(
                                scope.clone(),
                                shell_state,
                                rt_ctx.shell.default_timeout.clone(),
                                rt_ctx.env.clone(),
                                block_span_id,
                            );
                            let vm = Vm::new(name.clone(), ctx, rt_ctx).await?;
                            shells.insert(name.clone(), Arc::new(TokioMutex::new(vm)));
                        }
                        let vm_arc = shells.get(&name).expect("shell just inserted above");
                        let mut vm = vm_arc.lock().await;
                        let display_name = vm.current_name();
                        rt_ctx.log.emit_shell_switch(block_span_id, &display_name);
                        vm.set_block_span(block_span_id);
                        vm.exec_stmts(block.body()).await?;
                        // block_span drops here, closing the span.
                    }
                }
                IrTestItem::Cleanup { .. } => continue,
            }
        }
        Ok(())
    }
    .await;

    // 6. Terminate all test shells (deduplicated by Arc pointer)
    let mut seen = HashSet::new();
    for (_, vm_arc) in shells.drain() {
        let ptr = Arc::as_ptr(&vm_arc) as usize;
        if seen.insert(ptr) {
            vm_arc.lock().await.shutdown().await;
        }
    }

    // 7. Run test cleanup (fresh shell, best-effort)
    if let Some((cleanup, cleanup_span)) = &cleanup_block {
        let cleanup_block_span =
            rt_ctx
                .log
                .open_span(SpanKind::CleanupBlock, Some(test_span), Some(cleanup_span));
        let cleanup_block_span_id = cleanup_block_span.id();
        let shell_state = ShellState::new("__cleanup".to_string(), None);
        let ctx = ExecutionContext::new(
            scope.clone(),
            shell_state,
            rt_ctx.shell.default_timeout.clone(),
            rt_ctx.env.clone(),
            cleanup_block_span_id,
        );
        // Cleanup uses its own uncancellable token
        let mut cleanup_rt_ctx = rt_ctx.clone();
        cleanup_rt_ctx.cancel = CancellationToken::new();
        match Vm::new("__cleanup".to_string(), ctx, &cleanup_rt_ctx).await {
            Ok(mut cleanup_vm) => {
                if let Err(failure) = cleanup_vm.exec_stmts(cleanup.body()).await {
                    rt_ctx.log.emit_warning(
                        cleanup_block_span_id,
                        "__cleanup",
                        "test cleanup failed",
                    );
                    warnings.push(Warning::CleanupFailed {
                        source: CleanupSource::Test,
                        failure,
                    });
                }
                cleanup_vm.shutdown().await;
            }
            Err(e) => {
                rt_ctx.log.emit_warning(
                    cleanup_block_span_id,
                    "__cleanup",
                    "failed to spawn cleanup shell",
                );
                warnings.push(Warning::CleanupFailed {
                    source: CleanupSource::Test,
                    failure: Failure::Runtime {
                        message: format!("failed to spawn cleanup shell: {e:?}"),
                        span: None,
                        shell: None,
                    },
                });
            }
        }
        // cleanup_block_span drops here, closing the span.
    }

    body_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_simple() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(slugify("test: foo/bar"), "test--foo-bar");
    }

    #[test]
    fn slugify_alphanumeric() {
        assert_eq!(slugify("abc-123_def"), "abc-123_def");
    }

    #[test]
    fn slugify_leading_trailing_dashes() {
        assert_eq!(slugify("  hello  "), "hello");
    }

    #[test]
    fn test_display_id_format() {
        assert_eq!(
            test_display_id("basic/test.relux", "my test"),
            "basic/test.relux/my-test"
        );
    }
}
