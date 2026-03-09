use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::runtime::result::{Outcome, TestResult};

#[derive(Debug, Serialize, Deserialize)]
pub struct RunSummary {
    pub run: RunMeta,
    pub tests: Vec<TestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunMeta {
    pub run_id: String,
    pub timestamp: String,
    pub duration_ms: u64,
    pub hostname: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestEntry {
    pub name: String,
    pub path: String,
    pub outcome: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

pub fn write_run_summary(
    run_dir: &Path,
    run_id: &str,
    results: &[TestResult],
    total_duration: Duration,
) {
    let summary = build_summary(run_id, results, total_duration);
    let toml_string = toml::to_string_pretty(&summary).expect("failed to serialize run summary");
    let path = run_dir.join("run_summary.toml");
    let _ = fs::write(path, toml_string);
}

pub fn read_run_summary(run_dir: &Path) -> Result<RunSummary, String> {
    let path = run_dir.join("run_summary.toml");
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&content)
        .map_err(|e| format!("cannot parse {}: {e}", path.display()))
}

/// Returns `(path, name)` pairs for all failed tests.
pub fn failed_test_ids(summary: &RunSummary) -> Vec<(&str, &str)> {
    summary
        .tests
        .iter()
        .filter(|t| t.outcome == "fail")
        .map(|t| (t.path.as_str(), t.name.as_str()))
        .collect()
}

fn build_summary(run_id: &str, results: &[TestResult], total_duration: Duration) -> RunSummary {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".into());

    let run = RunMeta {
        run_id: run_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        duration_ms: total_duration.as_millis() as u64,
        hostname,
    };

    let tests = results.iter().map(|r| {
        let (outcome, failure_type, failure_summary, skip_reason) = match &r.outcome {
            Outcome::Pass => ("pass".to_string(), None, None, None),
            Outcome::Fail(f) => (
                "fail".to_string(),
                Some(f.failure_type().to_string()),
                Some(f.summary()),
                None,
            ),
            Outcome::Skipped(reason) => (
                "skipped".to_string(),
                None,
                None,
                Some(reason.clone()),
            ),
        };

        TestEntry {
            name: r.test_name.clone(),
            path: r.test_path.clone(),
            outcome,
            duration_ms: r.duration.as_millis() as u64,
            failure_type,
            failure_summary,
            skip_reason,
        }
    }).collect();

    RunSummary { run, tests }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::dsl::resolver::ir::Span;
    use crate::runtime::result::Failure;

    fn make_result(name: &str, path: &str, outcome: Outcome) -> TestResult {
        TestResult {
            test_name: name.into(),
            test_path: path.into(),
            outcome,
            duration: Duration::from_millis(100),
            shell_logs: HashMap::new(),
            progress: String::new(),
            log_dir: None,
        }
    }

    #[test]
    fn round_trip_serialization() {
        let results = vec![
            make_result("passes", "basic/pass.relux", Outcome::Pass),
            make_result("fails", "basic/fail.relux", Outcome::Fail(
                Failure::MatchTimeout {
                    pattern: "/ready/".into(),
                    shell: "default".into(),
                    span: Span::new(0, 0..1),
                },
            )),
            make_result("skipped", "basic/skip.relux", Outcome::Skipped("os:linux".into())),
        ];

        let summary = build_summary("test-run-id", &results, Duration::from_secs(1));
        let toml_str = toml::to_string_pretty(&summary).unwrap();
        let parsed: RunSummary = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.run.run_id, "test-run-id");
        assert_eq!(parsed.run.duration_ms, 1000);
        assert_eq!(parsed.tests.len(), 3);

        assert_eq!(parsed.tests[0].outcome, "pass");
        assert!(parsed.tests[0].failure_type.is_none());

        assert_eq!(parsed.tests[1].outcome, "fail");
        assert_eq!(parsed.tests[1].failure_type.as_deref(), Some("MatchTimeout"));
        assert!(parsed.tests[1].failure_summary.is_some());

        assert_eq!(parsed.tests[2].outcome, "skipped");
        assert_eq!(parsed.tests[2].skip_reason.as_deref(), Some("os:linux"));
    }

    #[test]
    fn failed_test_ids_filters_correctly() {
        let results = vec![
            make_result("passes", "basic/pass.relux", Outcome::Pass),
            make_result("fails", "basic/fail.relux", Outcome::Fail(
                Failure::Runtime {
                    message: "boom".into(),
                    span: None,
                    shell: None,
                },
            )),
            make_result("also fails", "basic/fail2.relux", Outcome::Fail(
                Failure::Runtime {
                    message: "boom2".into(),
                    span: None,
                    shell: None,
                },
            )),
            make_result("skipped", "basic/skip.relux", Outcome::Skipped("reason".into())),
        ];

        let summary = build_summary("run-1", &results, Duration::from_secs(2));
        let failed = failed_test_ids(&summary);

        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0], ("basic/fail.relux", "fails"));
        assert_eq!(failed[1], ("basic/fail2.relux", "also fails"));
    }
}
