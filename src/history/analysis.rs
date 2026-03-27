use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;

use crate::core::config;
use crate::runtime::report::run_summary::TestEntry;
use crate::runtime::slugify;

use super::loader::LoadedRun;

// ─── Core Types ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TestKey {
    path: String,
    name: String,
}

impl TestKey {
    pub(crate) fn new(entry: &TestEntry) -> Self {
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

pub(crate) type RunId = String;

pub(crate) struct TestMeta {
    pub outcome: String,
    pub duration_ms: u64,
    pub failure_type: Option<String>,
    pub failure_summary: Option<String>,
}

// ─── LoadedRunsCollection ──────────────────────────────────────

pub struct LoadedRunsCollection {
    pub(crate) tests: HashMap<TestKey, HashMap<RunId, TestMeta>>,
    run_count: usize,
    test_count: usize,
    pub(crate) run_order: Vec<RunId>,
    pub(crate) run_dirs: HashMap<RunId, PathBuf>,
    pub(crate) run_timestamps: HashMap<RunId, String>,
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
        // Update test_count to reflect post-filter size, so truncation()
        // only fires when --top N actually drops results.
        self.test_count = items.len();
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

                Some((
                    key.clone(),
                    FlakyRecord {
                        flips,
                        pass,
                        fail,
                        rate,
                    },
                ))
            })
            .collect()
    }
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
                Some((
                    key.clone(),
                    FailureRecord {
                        fails,
                        runs: total,
                        rate,
                    },
                ))
            })
            .collect()
    }
}

pub fn compute_failure_modes(coll: &LoadedRunsCollection) -> Vec<FailureModeEntry> {
    let mut mode_counts: HashMap<String, usize> = HashMap::new();
    for runs_map in coll.tests.values() {
        for meta in runs_map.values() {
            if meta.outcome == "fail"
                && let Some(ft) = &meta.failure_type
            {
                *mode_counts.entry(ft.clone()).or_insert(0) += 1;
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
            FailureModeEntry {
                failure_type,
                count,
                percentage,
            }
        })
        .collect();

    modes.sort_by(|a, b| b.count.cmp(&a.count));
    modes
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
                            if meta.outcome == "pass" {
                                "pass"
                            } else {
                                "fail"
                            },
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
                    if let Some(meta) = runs_map.get(rid)
                        && meta.outcome != "skipped"
                    {
                        total += meta.duration_ms;
                        found = true;
                    }
                }
                found.then_some(total)
            })
            .collect();

        compute_stats(&run_totals)
    }
}

// ─── Helpers ───────────────────────────────────────────────────

pub(crate) fn compute_stats(values: &[u64]) -> DurationStats {
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

    let variance = values
        .iter()
        .map(|&v| (v as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
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

pub(crate) fn linear_trend(values: &[u64]) -> String {
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

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::runtime::report::run_summary::RunMeta;
    use crate::runtime::report::run_summary::RunSummary;
    use crate::runtime::report::run_summary::TestEntry;

    use super::super::loader::LoadedRun;

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
            name: path.split('/').next_back().unwrap_or(path).to_string(),
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

    pub(crate) fn sample_runs() -> Vec<LoadedRun> {
        vec![
            make_run(
                "run1",
                "2026-03-01T00:00:00Z",
                vec![
                    make_test("a.relux", "pass", 100),
                    make_test("b.relux", "pass", 200),
                    make_test("c.relux", "fail", 300),
                ],
            ),
            make_run(
                "run2",
                "2026-03-02T00:00:00Z",
                vec![
                    make_test("a.relux", "fail", 110),
                    make_test("b.relux", "pass", 210),
                    make_test("c.relux", "fail", 310),
                ],
            ),
            make_run(
                "run3",
                "2026-03-03T00:00:00Z",
                vec![
                    make_test("a.relux", "pass", 120),
                    make_test("b.relux", "fail", 220),
                    make_test("c.relux", "pass", 320),
                ],
            ),
            make_run(
                "run4",
                "2026-03-04T00:00:00Z",
                vec![
                    make_test("a.relux", "pass", 130),
                    make_test("b.relux", "pass", 230),
                    make_test("c.relux", "fail", 330),
                ],
            ),
        ]
    }

    fn find_entry<'a, T>(entries: &'a [(TestKey, T)], path: &str) -> Option<(&'a TestKey, &'a T)> {
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
            make_run(
                "run1",
                "2026-03-01T00:00:00Z",
                vec![make_test("stable.relux", "pass", 100)],
            ),
            make_run(
                "run2",
                "2026-03-02T00:00:00Z",
                vec![make_test("stable.relux", "pass", 100)],
            ),
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
        use super::super::loader::filter_summaries;
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
}
