use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, process};

use serde::Serialize;
use tabled::builder::Builder;
use tabled::settings::object::Columns;
use tabled::settings::span::Span;
use tabled::settings::{Alignment, Modify, Style, Width};

use crate::config;
use crate::dsl::discover_relux_files;
use crate::runtime::result::format_duration;
use crate::runtime::run_summary::{RunSummary, TestEntry, read_run_summary};
use crate::runtime::slugify;

// ─── Loading & Filtering ───────────────────────────────────────

pub struct LoadedRun {
    pub dir: PathBuf,
    pub summary: RunSummary,
}

pub fn load_all_summaries(out_root: &Path, last_n: Option<usize>) -> Vec<LoadedRun> {
    let mut run_dirs: Vec<PathBuf> = match fs::read_dir(out_root) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("run-") && entry.file_type().ok()?.is_dir() {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return Vec::new(),
    };

    run_dirs.sort();

    if let Some(n) = last_n {
        let skip = run_dirs.len().saturating_sub(n);
        run_dirs = run_dirs.into_iter().skip(skip).collect();
    }

    run_dirs
        .into_iter()
        .filter_map(|dir| {
            let summary = read_run_summary(&dir).ok()?;
            Some(LoadedRun { dir, summary })
        })
        .collect()
}

pub fn resolve_test_filters(project_root: &Path, raw_paths: &[PathBuf]) -> Vec<String> {
    let tests_dir = config::tests_dir(project_root);
    let mut test_paths = Vec::new();

    for path in raw_paths {
        if path.is_dir() {
            match path.canonicalize() {
                Ok(canonical) => {
                    for file in discover_relux_files(&canonical) {
                        if let Ok(rel) = file.strip_prefix(&tests_dir) {
                            test_paths.push(rel.display().to_string());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("warning: cannot resolve {}: {e}", path.display());
                }
            }
        } else if path.exists() {
            match path.canonicalize() {
                Ok(canonical) => {
                    if let Ok(rel) = canonical.strip_prefix(&tests_dir) {
                        test_paths.push(rel.display().to_string());
                    }
                }
                Err(e) => {
                    eprintln!("warning: cannot resolve {}: {e}", path.display());
                }
            }
        } else {
            eprintln!("warning: path does not exist: {}", path.display());
        }
    }

    test_paths.sort();
    test_paths.dedup();
    test_paths
}

pub fn filter_summaries(runs: &mut [LoadedRun], test_paths: &[String]) {
    if test_paths.is_empty() {
        return;
    }
    let filter_set: HashSet<&str> = test_paths.iter().map(|s| s.as_str()).collect();
    for run in runs.iter_mut() {
        run.summary.tests.retain(|t| filter_set.contains(t.path.as_str()));
    }
}

// ─── Core Types ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TestKey {
    path: String,
    name: String,
}

impl TestKey {
    fn new(entry: &TestEntry) -> Self {
        Self {
            path: entry.path.clone(),
            name: entry.name.clone(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn into_path(self) -> String {
        self.path
    }

    pub fn into_name(self) -> String {
        self.name
    }
}

impl fmt::Display for TestKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.path, slugify(&self.name))
    }
}

type RunId = String;

struct TestMeta {
    outcome: String,
    duration_ms: u64,
    failure_type: Option<String>,
    failure_summary: Option<String>,
}

// ─── LoadedRunsCollection ──────────────────────────────────────

pub struct LoadedRunsCollection {
    tests: HashMap<TestKey, HashMap<RunId, TestMeta>>,
    run_count: usize,
    test_count: usize,
    run_order: Vec<RunId>,
    run_dirs: HashMap<RunId, PathBuf>,
    run_timestamps: HashMap<RunId, String>,
}

impl LoadedRunsCollection {
    pub fn new(runs: Vec<LoadedRun>) -> Self {
        let run_count = runs.len();
        let mut tests: HashMap<TestKey, HashMap<RunId, TestMeta>> = HashMap::new();
        let mut run_order = Vec::with_capacity(run_count);
        let mut run_dirs = HashMap::with_capacity(run_count);
        let mut run_timestamps = HashMap::with_capacity(run_count);

        for run in runs {
            let run_id = run.summary.run.run_id.clone();
            run_order.push(run_id.clone());
            run_dirs.insert(run_id.clone(), run.dir);
            run_timestamps.insert(run_id.clone(), run.summary.run.timestamp.clone());

            for entry in run.summary.tests {
                let key = TestKey::new(&entry);
                let meta = TestMeta {
                    outcome: entry.outcome,
                    duration_ms: entry.duration_ms,
                    failure_type: entry.failure_type,
                    failure_summary: entry.failure_summary,
                };
                tests.entry(key).or_default().insert(run_id.clone(), meta);
            }
        }

        let test_count = tests.len();

        Self {
            tests,
            run_count,
            test_count,
            run_order,
            run_dirs,
            run_timestamps,
        }
    }

    pub fn run_count(&self) -> usize {
        self.run_count
    }

    pub fn truncation(&self) -> Option<(usize, usize)> {
        if self.tests.len() < self.test_count {
            Some((self.tests.len(), self.test_count))
        } else {
            None
        }
    }

    pub fn truncate<T: Preaggregate>(&mut self, top_n: Option<usize>) -> Vec<(TestKey, T::Item)> {
        let mut items = T::preaggregate(self);
        items.sort_by(|a, b| a.1.cmp(&b.1));
        if let Some(n) = top_n {
            items.truncate(n);
        }
        let kept: HashSet<&TestKey> = items.iter().map(|(k, _)| k).collect();
        self.tests.retain(|k, _| kept.contains(k));
        items
    }

    pub fn aggregate<T: Aggregate>(&self) -> T::Item {
        T::aggregate(self)
    }
}

// ─── Traits ────────────────────────────────────────────────────

pub trait Preaggregate {
    type Item: Ord;
    fn preaggregate(collection: &LoadedRunsCollection) -> Vec<(TestKey, Self::Item)>;
}

pub trait Aggregate {
    type Item;
    fn aggregate(collection: &LoadedRunsCollection) -> Self::Item;
}

// ─── Flakiness ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FlakyRecord {
    pub flips: usize,
    pub pass: usize,
    pub fail: usize,
    pub rate: f64,
}

impl PartialEq for FlakyRecord {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate && self.flips == other.flips
    }
}
impl Eq for FlakyRecord {}

impl PartialOrd for FlakyRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FlakyRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Descending by rate
        other
            .rate
            .partial_cmp(&self.rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(other.flips.cmp(&self.flips))
    }
}

pub struct FlakyPreaggregate;

impl Preaggregate for FlakyPreaggregate {
    type Item = FlakyRecord;

    fn preaggregate(coll: &LoadedRunsCollection) -> Vec<(TestKey, FlakyRecord)> {
        coll.tests
            .iter()
            .filter_map(|(key, runs_map)| {
                let outcomes: Vec<&str> = coll
                    .run_order
                    .iter()
                    .filter_map(|rid| runs_map.get(rid).map(|m| m.outcome.as_str()))
                    .collect();

                let mut flips = 0;
                for w in outcomes.windows(2) {
                    if w[0] != w[1] && w[0] != "skipped" && w[1] != "skipped" {
                        flips += 1;
                    }
                }

                if flips == 0 {
                    return None;
                }

                let pass = outcomes.iter().filter(|&&o| o == "pass").count();
                let fail = outcomes.iter().filter(|&&o| o == "fail").count();
                let non_skipped = outcomes.iter().filter(|&&o| o != "skipped").count();
                let rate = if non_skipped > 1 {
                    (flips as f64 / (non_skipped - 1) as f64) * 100.0
                } else {
                    0.0
                };

                Some((key.clone(), FlakyRecord { flips, pass, fail, rate }))
            })
            .collect()
    }
}

pub fn format_flaky_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FlakyRecord)],
) -> String {
    let mut out = format!("Flakiness Report ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No flaky tests detected.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Flips", "Pass", "Fail", "Rate"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            rec.flips.to_string(),
            rec.pass.to_string(),
            rec.fail.to_string(),
            format!("{:.1}%", rec.rate),
        ]);
    }

    out.push_str(&make_table(builder, coll.truncation(), None));
    out.push_str(&legend);
    out
}

pub fn format_flaky_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FlakyRecord)],
) -> String {
    #[derive(Serialize)]
    struct FlakyReport {
        meta: ReportMeta,
        tests: Vec<FlakyEntryOut>,
    }
    #[derive(Serialize)]
    struct FlakyEntryOut {
        path: String,
        flips: usize,
        pass: usize,
        fail: usize,
        rate: f64,
    }

    let report = FlakyReport {
        meta: ReportMeta { runs: coll.run_count() },
        tests: entries
            .iter()
            .map(|(k, r)| FlakyEntryOut {
                path: k.to_string(),
                flips: r.flips,
                pass: r.pass,
                fail: r.fail,
                rate: r.rate,
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize flaky report")
}

// ─── Failure Analysis ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FailureRecord {
    pub fails: usize,
    pub runs: usize,
    pub rate: f64,
}

impl PartialEq for FailureRecord {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate && self.fails == other.fails
    }
}
impl Eq for FailureRecord {}

impl PartialOrd for FailureRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FailureRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .rate
            .partial_cmp(&self.rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(other.fails.cmp(&self.fails))
    }
}

#[derive(Debug, Serialize)]
pub struct FailureModeEntry {
    pub failure_type: String,
    pub count: usize,
    pub percentage: f64,
}

pub struct FailurePreaggregate;

impl Preaggregate for FailurePreaggregate {
    type Item = FailureRecord;

    fn preaggregate(coll: &LoadedRunsCollection) -> Vec<(TestKey, FailureRecord)> {
        coll.tests
            .iter()
            .filter_map(|(key, runs_map)| {
                let total = runs_map.len();
                let fails = runs_map.values().filter(|m| m.outcome == "fail").count();
                if fails == 0 {
                    return None;
                }
                let rate = (fails as f64 / total as f64) * 100.0;
                Some((key.clone(), FailureRecord { fails, runs: total, rate }))
            })
            .collect()
    }
}

pub fn compute_failure_modes(coll: &LoadedRunsCollection) -> Vec<FailureModeEntry> {
    let mut mode_counts: HashMap<String, usize> = HashMap::new();
    for runs_map in coll.tests.values() {
        for meta in runs_map.values() {
            if meta.outcome == "fail" {
                if let Some(ft) = &meta.failure_type {
                    *mode_counts.entry(ft.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    let total_failures: usize = mode_counts.values().sum();
    let mut modes: Vec<FailureModeEntry> = mode_counts
        .into_iter()
        .map(|(failure_type, count)| {
            let percentage = if total_failures > 0 {
                (count as f64 / total_failures as f64) * 100.0
            } else {
                0.0
            };
            FailureModeEntry { failure_type, count, percentage }
        })
        .collect();

    modes.sort_by(|a, b| b.count.cmp(&a.count));
    modes
}

pub fn format_failures_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FailureRecord)],
    modes: &[FailureModeEntry],
) -> String {
    let mut out = format!("Failure Report ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No failures detected.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Fails", "Runs", "Rate"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            rec.fails.to_string(),
            rec.runs.to_string(),
            format!("{:.1}%", rec.rate),
        ]);
    }

    out.push_str(&make_table(builder, coll.truncation(), None));

    if !modes.is_empty() {
        out.push_str("\n\n");
        let mut builder = Builder::default();
        builder.push_record(["Type", "Count", "%"]);
        for m in modes {
            builder.push_record([
                m.failure_type.clone(),
                m.count.to_string(),
                format!("{:.1}%", m.percentage),
            ]);
        }
        out.push_str(&make_table(builder, None, None));
    }

    out.push_str(&legend);
    out
}

pub fn format_failures_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FailureRecord)],
    modes: &[FailureModeEntry],
) -> String {
    #[derive(Serialize)]
    struct FailureReport {
        meta: ReportMeta,
        tests: Vec<FailureEntryOut>,
        modes: Vec<FailureModeOut>,
    }
    #[derive(Serialize)]
    struct FailureEntryOut {
        path: String,
        fails: usize,
        runs: usize,
        rate: f64,
    }
    #[derive(Serialize)]
    struct FailureModeOut {
        failure_type: String,
        count: usize,
        percentage: f64,
    }

    let report = FailureReport {
        meta: ReportMeta { runs: coll.run_count() },
        tests: entries
            .iter()
            .map(|(k, r)| FailureEntryOut {
                path: k.to_string(),
                fails: r.fails,
                runs: r.runs,
                rate: r.rate,
            })
            .collect(),
        modes: modes
            .iter()
            .map(|m| FailureModeOut {
                failure_type: m.failure_type.clone(),
                count: m.count,
                percentage: m.percentage,
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize failure report")
}

// ─── First-Fail Analysis ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FirstFailRecord {
    pub report: String,
    pub timestamp: String,
    pub failure_type: String,
    pub summary: String,
}

impl PartialEq for FirstFailRecord {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}
impl Eq for FirstFailRecord {}

impl PartialOrd for FirstFailRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FirstFailRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Descending by timestamp (most recent first)
        other.timestamp.cmp(&self.timestamp)
    }
}

pub struct FirstFailPreaggregate;

impl Preaggregate for FirstFailPreaggregate {
    type Item = FirstFailRecord;

    fn preaggregate(coll: &LoadedRunsCollection) -> Vec<(TestKey, FirstFailRecord)> {
        let mut prev_outcome: HashMap<&TestKey, &str> = HashMap::new();
        let mut first_fails: HashMap<&TestKey, FirstFailRecord> = HashMap::new();

        for run_id in &coll.run_order {
            for (key, runs_map) in &coll.tests {
                if let Some(meta) = runs_map.get(run_id) {
                    let prev = prev_outcome.get(key).copied();
                    if meta.outcome == "fail" && prev == Some("pass") {
                        let test_log = Path::new("logs")
                            .join(config::RELUX_DIR)
                            .join(config::TESTS_DIR)
                            .join(Path::new(key.path()).with_extension(""))
                            .join(slugify(key.name()))
                            .join("event.html");
                        let run_dir = &coll.run_dirs[run_id];
                        let report = run_dir.join(test_log).display().to_string();
                        let timestamp = coll.run_timestamps[run_id].clone();
                        first_fails.insert(
                            key,
                            FirstFailRecord {
                                report,
                                timestamp,
                                failure_type: meta.failure_type.clone().unwrap_or_default(),
                                summary: meta.failure_summary.clone().unwrap_or_default(),
                            },
                        );
                    }
                    if meta.outcome != "skipped" {
                        prev_outcome.insert(
                            key,
                            if meta.outcome == "pass" { "pass" } else { "fail" },
                        );
                    }
                }
            }
        }

        first_fails
            .into_iter()
            .map(|(k, v)| (k.clone(), v))
            .collect()
    }
}

pub fn format_first_fail_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FirstFailRecord)],
) -> String {
    let mut out = format!("Latest Regressions ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No pass-to-fail transitions detected.\n");
        return out;
    }

    for (i, (key, rec)) in entries.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("  Test:    {}\n", key));
        out.push_str(&format!("  Time:    {}\n", rec.timestamp));
        out.push_str(&format!("  Type:    {}\n", rec.failure_type));
        out.push_str(&format!("  Summary: {}\n", rec.summary));
        out.push_str(&format!("  Report:  {}\n", rec.report));
    }

    out
}

pub fn format_first_fail_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, FirstFailRecord)],
) -> String {
    #[derive(Serialize)]
    struct FirstFailReport {
        meta: ReportMeta,
        tests: Vec<FirstFailEntryOut>,
    }
    #[derive(Serialize)]
    struct FirstFailEntryOut {
        path: String,
        report: String,
        timestamp: String,
        failure_type: String,
        summary: String,
    }

    let report = FirstFailReport {
        meta: ReportMeta { runs: coll.run_count() },
        tests: entries
            .iter()
            .map(|(k, r)| FirstFailEntryOut {
                path: k.to_string(),
                report: r.report.clone(),
                timestamp: r.timestamp.clone(),
                failure_type: r.failure_type.clone(),
                summary: r.summary.clone(),
            })
            .collect(),
    };
    toml::to_string_pretty(&report).expect("failed to serialize first-fail report")
}

// ─── Duration Analysis ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DurationRecord {
    pub mean_ms: f64,
    pub stddev_ms: f64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub trend: String,
}

impl PartialEq for DurationRecord {
    fn eq(&self, other: &Self) -> bool {
        self.mean_ms == other.mean_ms
    }
}
impl Eq for DurationRecord {}

impl PartialOrd for DurationRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DurationRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Descending by mean_ms (slowest first)
        other
            .mean_ms
            .partial_cmp(&self.mean_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

#[derive(Debug, Serialize)]
pub struct DurationStats {
    pub mean_ms: f64,
    pub stddev_ms: f64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub trend: String,
}

pub struct DurationPreaggregate;

impl Preaggregate for DurationPreaggregate {
    type Item = DurationRecord;

    fn preaggregate(coll: &LoadedRunsCollection) -> Vec<(TestKey, DurationRecord)> {
        coll.tests
            .iter()
            .filter_map(|(key, runs_map)| {
                let durations: Vec<u64> = coll
                    .run_order
                    .iter()
                    .filter_map(|rid| {
                        runs_map
                            .get(rid)
                            .filter(|m| m.outcome != "skipped")
                            .map(|m| m.duration_ms)
                    })
                    .collect();

                if durations.is_empty() {
                    return None;
                }

                let stats = compute_stats(&durations);
                Some((
                    key.clone(),
                    DurationRecord {
                        mean_ms: stats.mean_ms,
                        stddev_ms: stats.stddev_ms,
                        min_ms: stats.min_ms,
                        max_ms: stats.max_ms,
                        trend: stats.trend,
                    },
                ))
            })
            .collect()
    }
}

pub struct DurationAggregate;

impl Aggregate for DurationAggregate {
    type Item = DurationStats;

    fn aggregate(coll: &LoadedRunsCollection) -> DurationStats {
        let run_totals: Vec<u64> = coll
            .run_order
            .iter()
            .filter_map(|rid| {
                let mut total: u64 = 0;
                let mut found = false;
                for runs_map in coll.tests.values() {
                    if let Some(meta) = runs_map.get(rid) {
                        if meta.outcome != "skipped" {
                            total += meta.duration_ms;
                            found = true;
                        }
                    }
                }
                found.then_some(total)
            })
            .collect();

        compute_stats(&run_totals)
    }
}

pub fn format_durations_human(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, DurationRecord)],
    aggregate: &DurationStats,
) -> String {
    let mut out = format!("Duration Analysis ({} runs)\n\n", coll.run_count());

    if entries.is_empty() {
        out.push_str("No duration data available.\n");
        return out;
    }

    let display_ids: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let display_refs: Vec<&str> = display_ids.iter().map(|s| s.as_str()).collect();
    let (idx, legend) = build_file_index(&display_refs);

    let mut builder = Builder::default();
    builder.push_record(["Test", "Mean", "StdDev", "Min", "Max", "Trend"]);
    for (key, rec) in entries {
        let display_id = key.to_string();
        builder.push_record([
            format_test_col(&idx, &display_id),
            fmt_dur(rec.mean_ms),
            fmt_dur(rec.stddev_ms),
            fmt_dur_u64(rec.min_ms),
            fmt_dur_u64(rec.max_ms),
            rec.trend.clone(),
        ]);
    }

    let footer = vec![
        vec![String::new(); 6],
        vec![
            "Aggregate".to_string(),
            fmt_dur(aggregate.mean_ms),
            fmt_dur(aggregate.stddev_ms),
            fmt_dur_u64(aggregate.min_ms),
            fmt_dur_u64(aggregate.max_ms),
            aggregate.trend.clone(),
        ],
    ];

    out.push_str(&make_table(builder, coll.truncation(), Some(&footer)));
    out.push_str(&legend);
    out
}

pub fn format_durations_toml(
    coll: &LoadedRunsCollection,
    entries: &[(TestKey, DurationRecord)],
    aggregate: &DurationStats,
) -> String {
    #[derive(Serialize)]
    struct DurationReport {
        meta: ReportMeta,
        tests: Vec<DurationEntryOut>,
        aggregate: DurationStatsOut,
    }
    #[derive(Serialize)]
    struct DurationEntryOut {
        path: String,
        mean_ms: f64,
        stddev_ms: f64,
        min_ms: u64,
        max_ms: u64,
        trend: String,
    }
    #[derive(Serialize)]
    struct DurationStatsOut {
        mean_ms: f64,
        stddev_ms: f64,
        min_ms: u64,
        max_ms: u64,
        trend: String,
    }

    let report = DurationReport {
        meta: ReportMeta { runs: coll.run_count() },
        tests: entries
            .iter()
            .map(|(k, r)| DurationEntryOut {
                path: k.to_string(),
                mean_ms: r.mean_ms,
                stddev_ms: r.stddev_ms,
                min_ms: r.min_ms,
                max_ms: r.max_ms,
                trend: r.trend.clone(),
            })
            .collect(),
        aggregate: DurationStatsOut {
            mean_ms: aggregate.mean_ms,
            stddev_ms: aggregate.stddev_ms,
            min_ms: aggregate.min_ms,
            max_ms: aggregate.max_ms,
            trend: aggregate.trend.clone(),
        },
    };
    toml::to_string_pretty(&report).expect("failed to serialize duration report")
}

// ─── Shared Helpers ────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ReportMeta {
    runs: usize,
}

fn build_file_index(test_ids: &[&str]) -> (HashMap<String, usize>, String) {
    let mut seen: Vec<String> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();
    for id in test_ids {
        if let Some(pos) = id.rfind('/') {
            let path = &id[..pos];
            if seen_set.insert(path.to_string()) {
                seen.push(path.to_string());
            }
        }
    }
    let map: HashMap<String, usize> = seen
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i + 1))
        .collect();

    let mut legend = String::from("\n\nFiles:\n");
    for (i, path) in seen.iter().enumerate() {
        legend.push_str(&format!("  {}: {path}\n", i + 1));
    }
    (map, legend)
}

fn format_test_col(file_idx: &HashMap<String, usize>, test_id: &str) -> String {
    if let Some(pos) = test_id.rfind('/') {
        let path = &test_id[..pos];
        let slug = &test_id[pos + 1..];
        let n = file_idx.get(path).copied().unwrap_or(0);
        format!("[{n}] {slug}")
    } else {
        test_id.to_string()
    }
}

fn term_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

fn make_table(
    mut builder: Builder,
    truncated: Option<(usize, usize)>,
    footer: Option<&[Vec<String>]>,
) -> String {
    let cols = builder.count_columns();
    let trunc_row = if let Some((shown, total)) = truncated {
        let mut row: Vec<String> = vec![String::new(); cols];
        row[0] = format!("~~~ showing {shown} of {total} results ~~~");
        let row_idx = shown + 1;
        builder.push_record(row);
        Some(row_idx)
    } else {
        None
    };
    if let Some(rows) = footer {
        for row in rows {
            builder.push_record(row.clone());
        }
    }
    let mut table = builder.build();
    table
        .with(Style::blank())
        .with(Alignment::right())
        .with(Modify::new(Columns::first()).with(Alignment::left()));
    if let Some(row_idx) = trunc_row {
        table
            .modify((row_idx, 0), Span::column(cols as isize))
            .modify((row_idx, 0), Alignment::center());
    }
    table.with(Width::increase(term_width()));
    table.to_string()
}

fn compute_stats(values: &[u64]) -> DurationStats {
    if values.is_empty() {
        return DurationStats {
            mean_ms: 0.0,
            stddev_ms: 0.0,
            min_ms: 0,
            max_ms: 0,
            trend: "stable".to_string(),
        };
    }

    let n = values.len() as f64;
    let sum: f64 = values.iter().map(|&v| v as f64).sum();
    let mean = sum / n;

    let variance = values.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();

    let min = *values.iter().min().unwrap();
    let max = *values.iter().max().unwrap();

    let trend = linear_trend(values);

    DurationStats {
        mean_ms: mean,
        stddev_ms: stddev,
        min_ms: min,
        max_ms: max,
        trend,
    }
}

fn linear_trend(values: &[u64]) -> String {
    if values.len() < 3 {
        return "stable".to_string();
    }

    let n = values.len() as f64;
    let x_mean = (n - 1.0) / 2.0;
    let y_mean: f64 = values.iter().map(|&v| v as f64).sum::<f64>() / n;

    let mut num = 0.0;
    let mut den = 0.0;
    for (i, &v) in values.iter().enumerate() {
        let x = i as f64 - x_mean;
        let y = v as f64 - y_mean;
        num += x * y;
        den += x * x;
    }

    if den == 0.0 {
        return "stable".to_string();
    }

    let slope = num / den;
    let relative_slope = slope / y_mean;

    if relative_slope > 0.02 {
        "slower".to_string()
    } else if relative_slope < -0.02 {
        "faster".to_string()
    } else {
        "stable".to_string()
    }
}

fn fmt_dur(ms: f64) -> String {
    format_duration(Duration::from_secs_f64(ms / 1000.0))
}

fn fmt_dur_u64(ms: u64) -> String {
    format_duration(Duration::from_millis(ms))
}

// ─── Dispatch ──────────────────────────────────────────────────

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

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::run_summary::{RunMeta, RunSummary, TestEntry};

    fn make_run(run_id: &str, timestamp: &str, tests: Vec<TestEntry>) -> LoadedRun {
        LoadedRun {
            dir: PathBuf::from(format!("/tmp/out/{run_id}")),
            summary: RunSummary {
                run: RunMeta {
                    run_id: run_id.to_string(),
                    timestamp: timestamp.to_string(),
                    duration_ms: tests.iter().map(|t| t.duration_ms).sum(),
                    hostname: "test-host".to_string(),
                },
                tests,
            },
        }
    }

    fn make_test(path: &str, outcome: &str, duration_ms: u64) -> TestEntry {
        TestEntry {
            name: path.split('/').last().unwrap_or(path).to_string(),
            path: path.to_string(),
            outcome: outcome.to_string(),
            duration_ms,
            failure_type: if outcome == "fail" {
                Some("MatchTimeout".to_string())
            } else {
                None
            },
            failure_summary: if outcome == "fail" {
                Some("timed out".to_string())
            } else {
                None
            },
            skip_reason: if outcome == "skipped" {
                Some("os:linux".to_string())
            } else {
                None
            },
        }
    }

    fn sample_runs() -> Vec<LoadedRun> {
        vec![
            make_run("run1", "2026-03-01T00:00:00Z", vec![
                make_test("a.relux", "pass", 100),
                make_test("b.relux", "pass", 200),
                make_test("c.relux", "fail", 300),
            ]),
            make_run("run2", "2026-03-02T00:00:00Z", vec![
                make_test("a.relux", "fail", 110),
                make_test("b.relux", "pass", 210),
                make_test("c.relux", "fail", 310),
            ]),
            make_run("run3", "2026-03-03T00:00:00Z", vec![
                make_test("a.relux", "pass", 120),
                make_test("b.relux", "fail", 220),
                make_test("c.relux", "pass", 320),
            ]),
            make_run("run4", "2026-03-04T00:00:00Z", vec![
                make_test("a.relux", "pass", 130),
                make_test("b.relux", "pass", 230),
                make_test("c.relux", "fail", 330),
            ]),
        ]
    }

    fn find_entry<'a, T>(
        entries: &'a [(TestKey, T)],
        path: &str,
    ) -> Option<(&'a TestKey, &'a T)> {
        entries
            .iter()
            .find(|(k, _)| k.to_string() == path)
            .map(|(k, v)| (k, v))
    }

    #[test]
    fn flaky_detects_alternating_outcomes() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<FlakyPreaggregate>(None);

        assert_eq!(coll.run_count(), 4);
        assert_eq!(entries.len(), 3);

        let (_, a) = find_entry(&entries, "a.relux/a-relux").unwrap();
        assert_eq!(a.flips, 2);
        assert_eq!(a.pass, 3);
        assert_eq!(a.fail, 1);

        let (_, c) = find_entry(&entries, "c.relux/c-relux").unwrap();
        assert_eq!(c.flips, 2);

        let (_, b) = find_entry(&entries, "b.relux/b-relux").unwrap();
        assert_eq!(b.flips, 2);
    }

    #[test]
    fn flaky_excludes_stable_tests() {
        let runs = vec![
            make_run("run1", "2026-03-01T00:00:00Z", vec![
                make_test("stable.relux", "pass", 100),
            ]),
            make_run("run2", "2026-03-02T00:00:00Z", vec![
                make_test("stable.relux", "pass", 100),
            ]),
        ];

        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<FlakyPreaggregate>(None);
        assert!(entries.is_empty());
    }

    #[test]
    fn failures_counts_correctly() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let modes = compute_failure_modes(&coll);
        let entries = coll.truncate::<FailurePreaggregate>(None);

        assert_eq!(coll.run_count(), 4);

        let (_, c) = find_entry(&entries, "c.relux/c-relux").unwrap();
        assert_eq!(c.fails, 3);
        assert_eq!(c.runs, 4);

        let (_, a) = find_entry(&entries, "a.relux/a-relux").unwrap();
        assert_eq!(a.fails, 1);
        assert_eq!(a.runs, 4);

        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].failure_type, "MatchTimeout");
    }

    #[test]
    fn first_fail_finds_transitions() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<FirstFailPreaggregate>(None);

        let (_, a) = find_entry(&entries, "a.relux/a-relux").unwrap();
        assert!(a.report.contains("run2"));

        let (_, b) = find_entry(&entries, "b.relux/b-relux").unwrap();
        assert!(b.report.contains("run3"));

        let (_, c) = find_entry(&entries, "c.relux/c-relux").unwrap();
        assert!(c.report.contains("run4"));
    }

    #[test]
    fn durations_computes_stats() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<DurationPreaggregate>(None);

        assert_eq!(coll.run_count(), 4);
        assert_eq!(entries.len(), 3);

        let (_, a) = find_entry(&entries, "a.relux/a-relux").unwrap();
        assert!((a.mean_ms - 115.0).abs() < 0.01);
        assert_eq!(a.min_ms, 100);
        assert_eq!(a.max_ms, 130);
    }

    #[test]
    fn durations_aggregate_covers_all() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let _entries = coll.truncate::<DurationPreaggregate>(None);
        let aggregate = coll.aggregate::<DurationAggregate>();

        assert!((aggregate.mean_ms - 645.0).abs() < 0.01);
        assert_eq!(aggregate.min_ms, 600);
        assert_eq!(aggregate.max_ms, 690);
    }

    #[test]
    fn filter_summaries_narrows_scope() {
        let mut runs = sample_runs();
        let filters = vec!["a.relux".to_string()];
        filter_summaries(&mut runs, &filters);

        for run in &runs {
            assert_eq!(run.summary.tests.len(), 1);
            assert_eq!(run.summary.tests[0].path, "a.relux");
        }
    }

    #[test]
    fn linear_trend_detects_increase() {
        assert_eq!(linear_trend(&[100, 200, 300, 400, 500]), "slower");
    }

    #[test]
    fn linear_trend_detects_decrease() {
        assert_eq!(linear_trend(&[500, 400, 300, 200, 100]), "faster");
    }

    #[test]
    fn linear_trend_stable_for_flat() {
        assert_eq!(linear_trend(&[100, 100, 100, 100]), "stable");
    }

    #[test]
    fn linear_trend_stable_for_few_points() {
        assert_eq!(linear_trend(&[100, 200]), "stable");
    }

    #[test]
    fn format_flaky_human_has_legend() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<FlakyPreaggregate>(None);
        let output = format_flaky_human(&coll, &entries);
        assert!(output.contains("Flakiness Report (4 runs)"));
        assert!(output.contains("[1]") || output.contains("[2]") || output.contains("[3]"));
        assert!(output.contains("Files:"));
        assert!(output.contains("a.relux"));
    }

    #[test]
    fn format_durations_human_has_aggregate() {
        let runs = sample_runs();
        let mut coll = LoadedRunsCollection::new(runs);
        let entries = coll.truncate::<DurationPreaggregate>(None);
        let aggregate = coll.aggregate::<DurationAggregate>();
        let output = format_durations_human(&coll, &entries, &aggregate);
        assert!(output.contains("Aggregate"));
        assert!(output.contains("Duration Analysis (4 runs)"));
        assert!(output.contains("Files:"));
    }

    #[test]
    fn build_file_index_assigns_sequential_numbers() {
        let ids = vec!["foo/bar.relux/test-one", "foo/bar.relux/test-two", "baz.relux/test-three"];
        let (idx, legend) = build_file_index(&ids);
        assert_eq!(idx["foo/bar.relux"], 1);
        assert_eq!(idx["baz.relux"], 2);
        assert!(legend.contains("1: foo/bar.relux"));
        assert!(legend.contains("2: baz.relux"));
    }

    #[test]
    fn format_test_col_produces_bracket_format() {
        let mut idx = HashMap::new();
        idx.insert("foo.relux".to_string(), 3);
        assert_eq!(format_test_col(&idx, "foo.relux/my-test"), "[3] my-test");
    }
}
