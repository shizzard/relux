use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::report::result::Outcome;
use crate::report::result::TestResult;

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
    /// Per-test log directory, relative to the run directory. When set,
    /// consumers can concatenate `<log_dir>/events.json` or
    /// `<log_dir>/event.html`. Absent when the runtime did not produce a
    /// log directory for the test (e.g. some invalid-test paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(default)]
    pub flaky_retries: u32,
}

pub fn write_run_summary(
    run_dir: &Path,
    run_id: &str,
    results: &[TestResult],
    total_duration: Duration,
) {
    let summary = build_summary(run_id, results, total_duration, run_dir);
    let toml_string = toml::to_string_pretty(&summary).expect("failed to serialize run summary");
    let path = run_dir.join("run_summary.toml");
    let _ = fs::write(path, toml_string);
}

pub fn read_run_summary(run_dir: &Path) -> Result<RunSummary, String> {
    let path = run_dir.join("run_summary.toml");
    let content =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&content).map_err(|e| format!("cannot parse {}: {e}", path.display()))
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

/// Returns `(path, name)` pairs for all tests with a nonzero outcome
/// (`fail`, `cancelled`, or `invalid`).
pub fn nonzero_test_ids(summary: &RunSummary) -> Vec<(&str, &str)> {
    summary
        .tests
        .iter()
        .filter(|t| t.outcome == "fail" || t.outcome == "cancelled" || t.outcome == "invalid")
        .map(|t| (t.path.as_str(), t.name.as_str()))
        .collect()
}

fn build_summary(
    run_id: &str,
    results: &[TestResult],
    total_duration: Duration,
    run_dir: &Path,
) -> RunSummary {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".into());

    let run = RunMeta {
        run_id: run_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        duration_ms: total_duration.as_millis() as u64,
        hostname,
    };

    let tests = results
        .iter()
        .map(|r| {
            let (
                outcome,
                failure_type,
                failure_summary,
                cancellation_reason,
                cancellation_detail,
                skip_reason,
            ) = match &r.outcome {
                Outcome::Pass => ("pass".to_string(), None, None, None, None, None),
                Outcome::Fail(f) => (
                    "fail".to_string(),
                    Some(f.failure_type().to_string()),
                    Some(f.summary()),
                    None,
                    None,
                    None,
                ),
                Outcome::Cancelled(c) => {
                    use crate::cancel::CancelReason;
                    let detail = match &c.reason {
                        CancelReason::TestTimeout { duration } => {
                            Some(format!("duration_ms={}", duration.as_millis()))
                        }
                        CancelReason::SuiteTimeout { duration } => {
                            Some(format!("duration_ms={}", duration.as_millis()))
                        }
                        CancelReason::FailFast { trigger_test } => {
                            Some(format!("trigger_test={trigger_test}"))
                        }
                        CancelReason::Sigint => None,
                    };
                    (
                        "cancelled".to_string(),
                        None,
                        None,
                        Some(c.reason_tag().to_string()),
                        detail,
                        None,
                    )
                }
                Outcome::Skipped(reason) => (
                    "skipped".to_string(),
                    None,
                    None,
                    None,
                    None,
                    Some(reason.clone()),
                ),
                Outcome::Invalid(reason) => (
                    "invalid".to_string(),
                    None,
                    None,
                    None,
                    None,
                    Some(reason.clone()),
                ),
            };

            let log_dir = r
                .log_dir
                .as_ref()
                .and_then(|d| d.strip_prefix(run_dir).ok())
                .map(|rel| rel.display().to_string());

            TestEntry {
                name: r.test_name.clone(),
                path: r.test_path.clone(),
                outcome,
                duration_ms: r.duration.as_millis() as u64,
                log_dir,
                failure_type,
                failure_summary,
                cancellation_reason,
                cancellation_detail,
                skip_reason,
                flaky_retries: r.flaky_retries,
            }
        })
        .collect();

    RunSummary { run, tests }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::result::Failure;
    use crate::report::result::FailureContext;
    use relux_core::diagnostics::IrSpan;

    fn make_result(name: &str, path: &str, outcome: Outcome) -> TestResult {
        TestResult {
            test_name: name.into(),
            test_path: path.into(),
            outcome,
            duration: Duration::from_millis(100),

            progress: String::new(),
            log_dir: None,
            warnings: Vec::new(),
            flaky_retries: 0,
        }
    }

    fn with_log_dir(mut result: TestResult, log_dir: &str) -> TestResult {
        result.log_dir = Some(std::path::PathBuf::from(log_dir));
        result
    }

    #[test]
    fn round_trip_serialization() {
        let results = vec![
            with_log_dir(
                make_result("passes", "basic/pass.relux", Outcome::Pass),
                "/tmp/runs/test-run-id/logs/basic/pass/passes",
            ),
            make_result(
                "fails",
                "basic/fail.relux",
                Outcome::Fail(Failure::MatchTimeout {
                    pattern: "/ready/".into(),
                    shell: "default".into(),
                    span: IrSpan::synthetic(),
                    effective: Box::new(relux_ir::IrTimeout::tolerance(Duration::from_secs(5))),
                    context: FailureContext::pre_vm(),
                }),
            ),
            make_result(
                "skipped",
                "basic/skip.relux",
                Outcome::Skipped("os:linux".into()),
            ),
        ];

        let summary = build_summary(
            "test-run-id",
            &results,
            Duration::from_secs(1),
            Path::new("/tmp/runs/test-run-id"),
        );
        let toml_str = toml::to_string_pretty(&summary).unwrap();
        let parsed: RunSummary = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.run.run_id, "test-run-id");
        assert_eq!(parsed.run.duration_ms, 1000);
        assert_eq!(parsed.tests.len(), 3);

        assert_eq!(parsed.tests[0].outcome, "pass");
        assert!(parsed.tests[0].failure_type.is_none());
        assert_eq!(
            parsed.tests[0].log_dir.as_deref(),
            Some("logs/basic/pass/passes"),
        );

        assert_eq!(parsed.tests[1].outcome, "fail");
        assert_eq!(
            parsed.tests[1].failure_type.as_deref(),
            Some("MatchTimeout")
        );
        assert!(parsed.tests[1].failure_summary.is_some());
        // `make_result` leaves `log_dir` unset; `skip_serializing_if`
        // means the parsed entry preserves `None`.
        assert!(parsed.tests[1].log_dir.is_none());

        assert_eq!(parsed.tests[2].outcome, "skipped");
        assert_eq!(parsed.tests[2].skip_reason.as_deref(), Some("os:linux"));
    }

    #[test]
    fn failed_test_ids_filters_correctly() {
        let results = vec![
            make_result("passes", "basic/pass.relux", Outcome::Pass),
            make_result(
                "fails",
                "basic/fail.relux",
                Outcome::Fail(Failure::Runtime {
                    message: "boom".into(),
                    span: None,
                    shell: None,
                    context: FailureContext::pre_vm(),
                }),
            ),
            make_result(
                "also fails",
                "basic/fail2.relux",
                Outcome::Fail(Failure::Runtime {
                    message: "boom2".into(),
                    span: None,
                    shell: None,
                    context: FailureContext::pre_vm(),
                }),
            ),
            make_result(
                "skipped",
                "basic/skip.relux",
                Outcome::Skipped("reason".into()),
            ),
        ];

        let summary = build_summary(
            "run-1",
            &results,
            Duration::from_secs(2),
            Path::new("/tmp/runs/run-1"),
        );
        let failed = failed_test_ids(&summary);

        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0], ("basic/fail.relux", "fails"));
        assert_eq!(failed[1], ("basic/fail2.relux", "also fails"));
    }
}
