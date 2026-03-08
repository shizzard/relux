use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;

use crate::dsl::resolver::ir::{SourceMap, Span};

#[derive(Debug, Clone)]
pub enum Failure {
    MatchTimeout {
        pattern: String,
        span: Span,
        shell: String,
    },
    FailPatternMatched {
        pattern: String,
        matched_line: String,
        span: Span,
        shell: String,
    },
    NegativeMatchFailed {
        pattern: String,
        matched_text: String,
        span: Span,
        shell: String,
    },
    ShellExited {
        shell: String,
        exit_code: Option<i32>,
        span: Span,
    },
    Runtime {
        message: String,
        span: Option<Span>,
        shell: Option<String>,
    },
}

impl Failure {
    pub fn summary(&self) -> String {
        match self {
            Failure::MatchTimeout { pattern, shell, .. } => {
                format!("match timeout in shell '{shell}': timed out waiting for {pattern}")
            }
            Failure::FailPatternMatched { pattern, matched_line, shell, .. } => {
                format!(
                    "fail pattern matched in shell '{shell}': pattern {pattern} triggered, matched: \"{matched_line}\""
                )
            }
            Failure::NegativeMatchFailed { pattern, matched_text, shell, .. } => {
                format!(
                    "negative match failed in shell '{shell}': pattern {pattern} was found, matched: \"{matched_text}\""
                )
            }
            Failure::ShellExited { shell, exit_code: Some(code), .. } => {
                format!("shell '{shell}' exited unexpectedly with exit code {code}")
            }
            Failure::ShellExited { shell, exit_code: None, .. } => {
                format!("shell '{shell}' exited unexpectedly without an exit code")
            }
            Failure::Runtime { message, shell: Some(shell), .. } => {
                format!("runtime error in shell '{shell}': {message}")
            }
            Failure::Runtime { message, shell: None, .. } => {
                format!("runtime error: {message}")
            }
        }
    }

    pub fn failure_type(&self) -> &'static str {
        match self {
            Failure::MatchTimeout { .. } => "MatchTimeout",
            Failure::FailPatternMatched { .. } => "FailPatternMatched",
            Failure::NegativeMatchFailed { .. } => "NegativeMatchFailed",
            Failure::ShellExited { .. } => "ShellExited",
            Failure::Runtime { .. } => "Runtime",
        }
    }
}

pub fn log_link(run_dir: &Path, result: &TestResult) -> Option<String> {
    let log_dir = result.log_dir.as_ref()?;
    let relative = log_dir.strip_prefix(run_dir).ok()?;
    Some(format!("{}/event.html", relative.display()))
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub test_path: String,
    pub outcome: Outcome,
    pub duration: Duration,
    pub shell_logs: HashMap<String, Vec<u8>>,
    pub progress: String,
    pub log_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Pass,
    Fail(Failure),
    Skipped(String),
}

pub struct Reporter;

impl Reporter {
    pub fn print(results: &[TestResult], source_map: &SourceMap) {
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        let mut total_duration = Duration::ZERO;

        for result in results {
            total_duration += result.duration;
            match &result.outcome {
                Outcome::Pass => passed += 1,
                Outcome::Fail(f) => {
                    failed += 1;
                    Self::print_failure(f, source_map);
                    Self::print_shell_logs(&result.shell_logs);
                }
                Outcome::Skipped(_) => skipped += 1,
            }
        }

        let status = if failed > 0 {
            "FAILED".red().to_string()
        } else {
            "ok".green().to_string()
        };

        let mut summary = format!(
            "\ntest result: {status}. {passed} passed; {failed} failed",
        );
        if skipped > 0 {
            summary.push_str(&format!("; {skipped} skipped"));
        }
        summary.push_str(&format!("; finished in {}\n", format_duration(total_duration)));
        eprint!("{summary}");
        let _ = std::io::stderr().flush();
    }

    fn print_failure(failure: &Failure, source_map: &SourceMap) {
        crate::dsl::report::print_failure(failure, source_map);
    }

    fn print_shell_logs(shell_logs: &HashMap<String, Vec<u8>>) {
        if shell_logs.is_empty() {
            return;
        }
        eprintln!("  shell logs:");
        for (shell, bytes) in shell_logs {
            eprintln!("  --- {shell} ---");
            let text = String::from_utf8_lossy(bytes);
            for line in text.lines() {
                eprintln!("    {line}");
            }
        }
    }
}

pub fn format_duration(d: Duration) -> String {
    let total_ms = d.as_secs_f64() * 1000.0;
    if total_ms < 1000.0 {
        format!("{:.1}ms", total_ms)
    } else {
        format!("{:.1}s", total_ms / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dummy_span() -> Span {
        Span::new(0, 0..1)
    }

    #[test]
    fn summary_match_timeout() {
        let f = Failure::MatchTimeout {
            pattern: "/ready/".into(),
            shell: "default".into(),
            span: dummy_span(),
        };
        assert_eq!(
            f.summary(),
            "match timeout in shell 'default': timed out waiting for /ready/"
        );
    }

    #[test]
    fn summary_fail_pattern_matched() {
        let f = Failure::FailPatternMatched {
            pattern: "/error/".into(),
            matched_line: "error: connection refused".into(),
            shell: "default".into(),
            span: dummy_span(),
        };
        assert_eq!(
            f.summary(),
            "fail pattern matched in shell 'default': pattern /error/ triggered, matched: \"error: connection refused\""
        );
    }

    #[test]
    fn summary_negative_match_failed() {
        let f = Failure::NegativeMatchFailed {
            pattern: "/warning/".into(),
            matched_text: "warning: deprecated".into(),
            shell: "default".into(),
            span: dummy_span(),
        };
        assert_eq!(
            f.summary(),
            "negative match failed in shell 'default': pattern /warning/ was found, matched: \"warning: deprecated\""
        );
    }

    #[test]
    fn summary_shell_exited_with_code() {
        let f = Failure::ShellExited {
            shell: "default".into(),
            exit_code: Some(1),
            span: dummy_span(),
        };
        assert_eq!(
            f.summary(),
            "shell 'default' exited unexpectedly with exit code 1"
        );
    }

    #[test]
    fn summary_shell_exited_without_code() {
        let f = Failure::ShellExited {
            shell: "default".into(),
            exit_code: None,
            span: dummy_span(),
        };
        assert_eq!(
            f.summary(),
            "shell 'default' exited unexpectedly without an exit code"
        );
    }

    #[test]
    fn summary_runtime_with_shell() {
        let f = Failure::Runtime {
            message: "something broke".into(),
            shell: Some("default".into()),
            span: None,
        };
        assert_eq!(
            f.summary(),
            "runtime error in shell 'default': something broke"
        );
    }

    #[test]
    fn summary_runtime_without_shell() {
        let f = Failure::Runtime {
            message: "something broke".into(),
            shell: None,
            span: None,
        };
        assert_eq!(f.summary(), "runtime error: something broke");
    }

    #[test]
    fn log_link_with_log_dir() {
        let run_dir = Path::new("/tmp/runs/run-001");
        let result = TestResult {
            test_name: "my_test".into(),
            test_path: "tests/my_test.relux".into(),
            outcome: Outcome::Pass,
            duration: Duration::from_millis(100),
            shell_logs: HashMap::new(),
            progress: String::new(),
            log_dir: Some(PathBuf::from("/tmp/runs/run-001/my_test")),
        };
        assert_eq!(
            log_link(run_dir, &result),
            Some("my_test/event.html".to_string())
        );
    }

    #[test]
    fn log_link_without_log_dir() {
        let run_dir = Path::new("/tmp/runs/run-001");
        let result = TestResult {
            test_name: "my_test".into(),
            test_path: "tests/my_test.relux".into(),
            outcome: Outcome::Pass,
            duration: Duration::from_millis(100),
            shell_logs: HashMap::new(),
            progress: String::new(),
            log_dir: None,
        };
        assert_eq!(log_link(run_dir, &result), None);
    }
}
