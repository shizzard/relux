pub mod analysis;
pub mod format;
pub mod loader;

use std::path::{Path, PathBuf};
use std::process;

use crate::core::config;

use self::analysis::{
    DurationAggregate, DurationPreaggregate, FailurePreaggregate, FirstFailPreaggregate,
    FlakyPreaggregate, LoadedRunsCollection, compute_failure_modes,
};
use self::format::{
    format_durations_human, format_durations_toml, format_failures_human, format_failures_toml,
    format_first_fail_human, format_first_fail_toml, format_flaky_human, format_flaky_toml,
};
use crate::runtime::report::run_summary::{RunSummary, read_run_summary};

use self::loader::{filter_summaries, load_all_summaries, resolve_test_filters};

// ─── LatestRun ──────────────────────────────────────────────

pub struct LatestRun {
    pub summary: RunSummary,
}

impl LatestRun {
    pub fn load(project_root: &Path) -> Result<Self, String> {
        let latest = config::out_dir(project_root).join("latest");
        if !latest.exists() {
            return Err("no previous runs found (missing latest symlink)".into());
        }
        let summary = read_run_summary(&latest)?;
        Ok(Self { summary })
    }

    /// Returns module paths (no `.relux` extension) for all non-passing tests.
    ///
    /// Summary paths are relative to `tests_dir` (e.g. `trigger.relux`).
    /// Module paths are relative to `relux_dir` (e.g. `tests/trigger`).
    pub fn non_pass_paths(&self) -> Vec<String> {
        self.summary
            .tests
            .iter()
            .filter(|t| t.outcome != "pass")
            .map(|t| {
                let without_ext = t.path.strip_suffix(".relux").unwrap_or(&t.path);
                format!("tests/{without_ext}")
            })
            .collect()
    }
}

// ─── History Commands ───────────────────────────────────────

pub enum HistoryCommand {
    Flaky,
    Failures,
    FirstFail,
    Durations,
}

pub enum OutputFormat {
    Human,
    Toml,
}

pub fn run_history(
    project_root: &Path,
    command: HistoryCommand,
    test_paths: &[PathBuf],
    last_n: Option<usize>,
    top_n: Option<usize>,
    format: OutputFormat,
) {
    let out_root = config::out_dir(project_root);
    if !out_root.exists() {
        eprintln!("error: no output directory found at {}", out_root.display());
        process::exit(1);
    }

    let mut runs = load_all_summaries(&out_root, last_n);
    if runs.is_empty() {
        eprintln!("error: no run history found");
        process::exit(1);
    }

    if !test_paths.is_empty() {
        let filters = resolve_test_filters(project_root, test_paths);
        filter_summaries(&mut runs, &filters);
    }

    let mut coll = LoadedRunsCollection::new(runs);

    let output = match (&command, &format) {
        (HistoryCommand::Flaky, OutputFormat::Human) => {
            let entries = coll.truncate::<FlakyPreaggregate>(top_n);
            format_flaky_human(&coll, &entries)
        }
        (HistoryCommand::Flaky, OutputFormat::Toml) => {
            let entries = coll.truncate::<FlakyPreaggregate>(top_n);
            format_flaky_toml(&coll, &entries)
        }
        (HistoryCommand::Failures, OutputFormat::Human) => {
            let modes = compute_failure_modes(&coll);
            let entries = coll.truncate::<FailurePreaggregate>(top_n);
            format_failures_human(&coll, &entries, &modes)
        }
        (HistoryCommand::Failures, OutputFormat::Toml) => {
            let modes = compute_failure_modes(&coll);
            let entries = coll.truncate::<FailurePreaggregate>(top_n);
            format_failures_toml(&coll, &entries, &modes)
        }
        (HistoryCommand::FirstFail, OutputFormat::Human) => {
            let entries = coll.truncate::<FirstFailPreaggregate>(top_n);
            format_first_fail_human(&coll, &entries)
        }
        (HistoryCommand::FirstFail, OutputFormat::Toml) => {
            let entries = coll.truncate::<FirstFailPreaggregate>(top_n);
            format_first_fail_toml(&coll, &entries)
        }
        (HistoryCommand::Durations, OutputFormat::Human) => {
            let entries = coll.truncate::<DurationPreaggregate>(top_n);
            let aggregate = coll.aggregate::<DurationAggregate>();
            format_durations_human(&coll, &entries, &aggregate)
        }
        (HistoryCommand::Durations, OutputFormat::Toml) => {
            let entries = coll.truncate::<DurationPreaggregate>(top_n);
            let aggregate = coll.aggregate::<DurationAggregate>();
            format_durations_toml(&coll, &entries, &aggregate)
        }
    };

    print!("{output}");
}
