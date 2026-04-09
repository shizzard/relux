use std::fmt::Write;
use std::path::Path;

use crate::report::result::Failure;
use crate::report::result::Outcome;
use crate::report::result::TestResult;
use crate::report::result::log_link;
use relux_core::diagnostics::IrSpan;
use relux_core::table::SourceTable;

/// Compute the 1-based line number for a byte offset in a source string.
fn line_number(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

/// Escape a string for use as a YAML double-quoted scalar.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Extract the span from a Failure, if present.
fn failure_span(failure: &Failure) -> Option<&IrSpan> {
    match failure {
        Failure::MatchTimeout { span, .. }
        | Failure::FailPatternMatched { span, .. }
        | Failure::ShellExited { span, .. } => Some(span),
        Failure::Runtime { span, .. } | Failure::Cancelled { span, .. } => span.as_ref(),
    }
}

/// Extract the shell name from a Failure, if present.
fn failure_shell(failure: &Failure) -> Option<&str> {
    match failure {
        Failure::MatchTimeout { shell, .. }
        | Failure::FailPatternMatched { shell, .. }
        | Failure::ShellExited { shell, .. } => Some(shell),
        Failure::Runtime { shell, .. } | Failure::Cancelled { shell, .. } => shell.as_deref(),
    }
}

/// Extract the pattern from a Failure, if present.
fn failure_pattern(failure: &Failure) -> Option<&str> {
    match failure {
        Failure::MatchTimeout { pattern, .. } | Failure::FailPatternMatched { pattern, .. } => {
            Some(pattern)
        }
        _ => None,
    }
}

/// Render TAP version 14 output for the given test results.
fn render_tap(
    run_dir: &Path,
    _suite_name: &str,
    results: &[TestResult],
    source_table: &SourceTable,
) -> String {
    let mut out = String::new();

    writeln!(out, "TAP version 14").unwrap();
    writeln!(out, "1..{}", results.len()).unwrap();

    for (i, result) in results.iter().enumerate() {
        let num = i + 1;

        match &result.outcome {
            Outcome::Pass => {
                writeln!(out, "ok {num} - {}", result.test_name).unwrap();
                writeln!(out, "  ---").unwrap();
                writeln!(out, "  duration_ms: {}", result.duration.as_millis()).unwrap();
                if let Some(link) = log_link(run_dir, result) {
                    writeln!(out, "  log: {link}").unwrap();
                }
                writeln!(out, "  ...").unwrap();
            }
            Outcome::Fail(failure) => {
                writeln!(out, "not ok {num} - {}", result.test_name).unwrap();
                writeln!(out, "  ---").unwrap();
                writeln!(out, "  message: \"{}\"", yaml_escape(&failure.summary())).unwrap();

                if let Some(shell) = failure_shell(failure) {
                    writeln!(out, "  shell: {shell}").unwrap();
                }
                if let Some(pattern) = failure_pattern(failure) {
                    writeln!(out, "  pattern: {pattern}").unwrap();
                }
                if let Some(span) = failure_span(failure)
                    && let Some(sf) = source_table.get(span.file())
                {
                    writeln!(out, "  file: {}", sf.path.display()).unwrap();
                    writeln!(
                        out,
                        "  line: {}",
                        line_number(&sf.source, span.span().start())
                    )
                    .unwrap();
                }
                writeln!(out, "  duration_ms: {}", result.duration.as_millis()).unwrap();
                if let Some(link) = log_link(run_dir, result) {
                    writeln!(out, "  log: {link}").unwrap();
                }
                writeln!(out, "  ...").unwrap();
            }
            Outcome::Skipped(reason) => {
                writeln!(out, "ok {num} - {} # SKIP {reason}", result.test_name).unwrap();
            }
            Outcome::Invalid(reason) => {
                writeln!(
                    out,
                    "not ok {num} - {} # INVALID {reason}",
                    result.test_name
                )
                .unwrap();
            }
        }
    }

    out
}

/// Generate TAP version 14 output and write it to `run_dir/results.tap`.
pub fn generate_tap(
    run_dir: &Path,
    suite_name: &str,
    results: &[TestResult],
    source_table: &SourceTable,
) {
    let tap = render_tap(run_dir, suite_name, results, source_table);
    let path = run_dir.join("results.tap");
    std::fs::write(path, tap).expect("failed to write results.tap");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::result::Failure;
    use crate::report::result::Outcome;
    use crate::report::result::TestResult;
    use relux_core::diagnostics::IrSpan;
    use relux_core::table::FileId;
    use relux_core::table::SharedTable;
    use relux_core::table::SourceFile;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;

    fn test_source_table() -> SourceTable {
        let table: SourceTable = SharedTable::new();
        table.insert(
            FileId::new(PathBuf::from("tests/auth/login.relux")),
            SourceFile {
                path: PathBuf::from("tests/auth/login.relux"),
                source: "line 1\nline 2\nline 3\n".to_string(),
            },
        );
        table
    }

    fn test_span(offset_start: usize, offset_end: usize) -> IrSpan {
        IrSpan::new(
            FileId::new(PathBuf::from("tests/auth/login.relux")),
            relux_core::Span::new(offset_start, offset_end),
        )
    }

    fn run_dir() -> &'static Path {
        Path::new("/tmp/runs/run-001")
    }

    fn pass_result(name: &str, ms: u64, log_dir: Option<&str>) -> TestResult {
        TestResult {
            test_name: name.into(),
            test_path: format!("tests/{name}.relux"),
            outcome: Outcome::Pass,
            duration: Duration::from_millis(ms),

            progress: String::new(),
            log_dir: log_dir.map(PathBuf::from),
            warnings: Vec::new(),
            flaky_retries: 0,
        }
    }

    fn fail_result(name: &str, ms: u64, failure: Failure, log_dir: Option<&str>) -> TestResult {
        TestResult {
            test_name: name.into(),
            test_path: format!("tests/{name}.relux"),
            outcome: Outcome::Fail(failure),
            duration: Duration::from_millis(ms),

            progress: String::new(),
            log_dir: log_dir.map(PathBuf::from),
            warnings: Vec::new(),
            flaky_retries: 0,
        }
    }

    fn skip_result(name: &str, reason: &str) -> TestResult {
        TestResult {
            test_name: name.into(),
            test_path: format!("tests/{name}.relux"),
            outcome: Outcome::Skipped(reason.into()),
            duration: Duration::ZERO,

            progress: String::new(),
            log_dir: None,
            warnings: Vec::new(),
            flaky_retries: 0,
        }
    }

    #[test]
    fn header_and_plan_line() {
        let st = test_source_table();
        let results = vec![pass_result("a", 100, None), pass_result("b", 200, None)];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        let lines: Vec<&str> = tap.lines().collect();
        assert_eq!(lines[0], "TAP version 14");
        assert_eq!(lines[1], "1..2");
    }

    #[test]
    fn passed_test_with_log() {
        let st = test_source_table();
        let results = vec![pass_result(
            "login-test",
            1230,
            Some("/tmp/runs/run-001/logs/auth/login-test"),
        )];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        let lines: Vec<&str> = tap.lines().collect();
        assert_eq!(lines[2], "ok 1 - login-test");
        assert_eq!(lines[3], "  ---");
        assert_eq!(lines[4], "  duration_ms: 1230");
        assert_eq!(lines[5], "  log: logs/auth/login-test/event.html");
        assert_eq!(lines[6], "  ...");
    }

    #[test]
    fn passed_test_without_log() {
        let st = test_source_table();
        let results = vec![pass_result("simple", 50, None)];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        assert!(tap.contains("ok 1 - simple"));
        assert!(tap.contains("duration_ms: 50"));
        assert!(!tap.contains("log:"));
    }

    #[test]
    fn failed_test_with_diagnostics() {
        let st = test_source_table();
        // span at byte 14 = start of line 3
        let failure = Failure::MatchTimeout {
            pattern: "/ready/".into(),
            span: test_span(14, 20),
            shell: "default".into(),
        };
        let results = vec![fail_result(
            "timeout-test",
            5000,
            failure,
            Some("/tmp/runs/run-001/logs/auth/timeout-test"),
        )];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        let lines: Vec<&str> = tap.lines().collect();

        assert_eq!(lines[2], "not ok 1 - timeout-test");
        assert_eq!(lines[3], "  ---");
        assert!(lines[4].starts_with("  message: \""));
        assert!(lines[4].contains("match timeout"));
        assert_eq!(lines[5], "  shell: default");
        assert_eq!(lines[6], "  pattern: /ready/");
        assert_eq!(lines[7], "  file: tests/auth/login.relux");
        assert_eq!(lines[8], "  line: 3");
        assert_eq!(lines[9], "  duration_ms: 5000");
        assert_eq!(lines[10], "  log: logs/auth/timeout-test/event.html");
        assert_eq!(lines[11], "  ...");
    }

    #[test]
    fn failed_runtime_error_without_span() {
        let st = test_source_table();
        let failure = Failure::Runtime {
            message: "something broke".into(),
            span: None,
            shell: None,
        };
        let results = vec![fail_result("broken", 100, failure, None)];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        assert!(tap.contains("not ok 1 - broken"));
        assert!(tap.contains("message: \"runtime error: something broke\""));
        assert!(!tap.contains("shell:"));
        assert!(!tap.contains("pattern:"));
        assert!(!tap.contains("file:"));
        assert!(!tap.contains("line:"));
    }

    #[test]
    fn skipped_test() {
        let st = test_source_table();
        let results = vec![skip_result("linux-only", "os:linux")];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        let lines: Vec<&str> = tap.lines().collect();
        assert_eq!(lines[2], "ok 1 - linux-only # SKIP os:linux");
        // No diagnostics block for skipped tests
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn mixed_results() {
        let st = test_source_table();
        let failure = Failure::ShellExited {
            shell: "main".into(),
            exit_code: Some(1),
            span: test_span(0, 5),
        };
        let results = vec![
            pass_result("test-a", 100, None),
            fail_result("test-b", 200, failure, None),
            skip_result("test-c", "os:macos"),
        ];
        let tap = render_tap(run_dir(), "suite", &results, &st);

        assert!(tap.starts_with("TAP version 14\n1..3\n"));
        assert!(tap.contains("ok 1 - test-a"));
        assert!(tap.contains("not ok 2 - test-b"));
        assert!(tap.contains("ok 3 - test-c # SKIP os:macos"));
    }

    #[test]
    fn message_with_quotes_is_escaped() {
        let st = test_source_table();
        let failure = Failure::FailPatternMatched {
            pattern: "/error/".into(),
            matched_line: "got \"error\" here".into(),
            span: test_span(0, 5),
            shell: "default".into(),
        };
        let results = vec![fail_result("quote-test", 100, failure, None)];
        let tap = render_tap(run_dir(), "suite", &results, &st);
        // Inner quotes should be escaped
        assert!(tap.contains("\\\"error\\\""));
    }

    #[test]
    fn line_number_computation() {
        let source = "line 1\nline 2\nline 3\n";
        assert_eq!(line_number(source, 0), 1); // start of line 1
        assert_eq!(line_number(source, 6), 1); // last char of line 1 (before \n at 6)
        assert_eq!(line_number(source, 7), 2); // start of line 2 (after \n)
        assert_eq!(line_number(source, 14), 3); // start of line 3
    }
}
