use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
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
