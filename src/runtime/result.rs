use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

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

        for result in results {
            match &result.outcome {
                Outcome::Pass => {
                    passed += 1;
                    println!("✓ {} ({:?})", result.test_name, result.duration);
                }
                Outcome::Fail(f) => {
                    failed += 1;
                    println!("✗ {} ({:?})", result.test_name, result.duration);
                    Self::print_failure(f, source_map);
                    Self::print_shell_logs(&result.shell_logs);
                }
                Outcome::Skipped(reason) => {
                    skipped += 1;
                    println!("- {} (skipped: {})", result.test_name, reason);
                }
            }
        }

        if skipped > 0 {
            println!("{passed} passed, {failed} failed, {skipped} skipped");
        } else {
            println!("{passed} passed, {failed} failed");
        }
    }

    fn print_failure(failure: &Failure, source_map: &SourceMap) {
        crate::dsl::report::print_failure(failure, source_map);
    }

    fn print_shell_logs(shell_logs: &HashMap<String, Vec<u8>>) {
        if shell_logs.is_empty() {
            return;
        }
        println!("  shell logs:");
        for (shell, bytes) in shell_logs {
            println!("  --- {shell} ---");
            let text = String::from_utf8_lossy(bytes);
            for line in text.lines() {
                println!("    {line}");
            }
        }
    }
}
