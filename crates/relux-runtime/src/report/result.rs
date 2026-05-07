use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use colored::Colorize;

use relux_core::diagnostics::IrSpan;

use crate::observe::structured::EventSeq;
use crate::observe::structured::SpanId;
use crate::observe::structured::StackFrame;

/// Diagnostic context captured at failure-construction time. Travels with
/// every `Failure` so that downstream consumers (structured-log artifact,
/// console error renderer) can render the call site, what arrived in the
/// shell, and which user vars were live — without needing to reach back
/// into a VM that is about to be dropped.
#[derive(Debug, Clone, Default)]
pub struct FailureContext {
    pub span: Option<SpanId>,
    pub event_seq: Option<EventSeq>,
    pub call_stack: Vec<StackFrame>,
    pub buffer_tail: String,
    pub vars_in_scope: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum Failure {
    MatchTimeout {
        pattern: String,
        span: IrSpan,
        shell: String,
        context: FailureContext,
    },
    FailPatternMatched {
        pattern: String,
        matched_line: String,
        span: IrSpan,
        shell: String,
        context: FailureContext,
    },
    ShellExited {
        shell: String,
        exit_code: Option<i32>,
        span: IrSpan,
        context: FailureContext,
    },
    Runtime {
        message: String,
        span: Option<IrSpan>,
        shell: Option<String>,
        context: FailureContext,
    },
    Cancelled {
        span: Option<IrSpan>,
        shell: Option<String>,
        context: FailureContext,
    },
}

impl Failure {
    pub fn summary(&self) -> String {
        match self {
            Failure::MatchTimeout { pattern, shell, .. } => {
                format!("match timeout in shell '{shell}': timed out waiting for {pattern}")
            }
            Failure::FailPatternMatched {
                pattern,
                matched_line,
                shell,
                ..
            } => {
                format!(
                    "fail pattern matched in shell '{shell}': pattern {pattern} triggered, matched: \"{matched_line}\""
                )
            }
            Failure::ShellExited {
                shell,
                exit_code: Some(code),
                ..
            } => {
                format!("shell '{shell}' exited unexpectedly with exit code {code}")
            }
            Failure::ShellExited {
                shell,
                exit_code: None,
                ..
            } => {
                format!("shell '{shell}' exited unexpectedly without an exit code")
            }
            Failure::Runtime {
                message,
                shell: Some(shell),
                ..
            } => {
                format!("runtime error in shell '{shell}': {message}")
            }
            Failure::Runtime {
                message,
                shell: None,
                ..
            } => {
                format!("runtime error: {message}")
            }
            Failure::Cancelled {
                shell: Some(shell), ..
            } => {
                format!("cancelled in shell '{shell}'")
            }
            Failure::Cancelled { shell: None, .. } => "cancelled".to_string(),
        }
    }

    pub fn failure_type(&self) -> &'static str {
        match self {
            Failure::MatchTimeout { .. } => "MatchTimeout",
            Failure::FailPatternMatched { .. } => "FailPatternMatched",
            Failure::ShellExited { .. } => "ShellExited",
            Failure::Runtime { .. } => "Runtime",
            Failure::Cancelled { .. } => "Cancelled",
        }
    }

    pub fn context(&self) -> &FailureContext {
        match self {
            Failure::MatchTimeout { context, .. }
            | Failure::FailPatternMatched { context, .. }
            | Failure::ShellExited { context, .. }
            | Failure::Runtime { context, .. }
            | Failure::Cancelled { context, .. } => context,
        }
    }
}

impl From<&Failure> for relux_core::error::DiagnosticReport {
    fn from(failure: &Failure) -> Self {
        use relux_core::error::DiagnosticReport;
        use relux_core::error::Severity;
        match failure {
            Failure::MatchTimeout {
                pattern,
                span,
                shell,
                ..
            } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("match timeout in shell `{shell}`"),
                labels: vec![(span.clone(), format!("timed out waiting for `{pattern}`")).into()],
                help: None,
                note: None,
            },
            Failure::FailPatternMatched {
                pattern,
                matched_line,
                span,
                shell,
                ..
            } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("fail pattern matched in shell `{shell}`"),
                labels: vec![(span.clone(), format!("pattern `{pattern}` triggered here")).into()],
                help: None,
                note: Some(format!("matched output: {matched_line}")),
            },
            Failure::ShellExited {
                shell,
                exit_code,
                span,
                ..
            } => {
                let code_msg = match exit_code {
                    Some(c) => format!("with exit code {c}"),
                    None => "without an exit code".to_string(),
                };
                DiagnosticReport {
                    severity: Severity::Error,
                    message: format!("shell `{shell}` exited unexpectedly"),
                    labels: vec![(span.clone(), code_msg).into()],
                    help: None,
                    note: None,
                }
            }
            Failure::Runtime {
                message,
                span,
                shell,
                ..
            } => {
                let msg = match shell {
                    Some(s) => format!("runtime error in shell `{s}`"),
                    None => "runtime error".to_string(),
                };
                let first_line = message.lines().next().unwrap_or(message);
                let has_detail = message.contains('\n');
                match span {
                    Some(span) => DiagnosticReport {
                        severity: Severity::Error,
                        message: msg,
                        labels: vec![(span.clone(), first_line.to_string()).into()],
                        help: None,
                        note: if has_detail {
                            Some(message.clone())
                        } else {
                            None
                        },
                    },
                    None => DiagnosticReport {
                        severity: Severity::Error,
                        message: format!("{msg}: {first_line}"),
                        labels: vec![],
                        help: None,
                        note: if has_detail {
                            Some(message.clone())
                        } else {
                            None
                        },
                    },
                }
            }
            Failure::Cancelled { span, shell, .. } => {
                let msg = match shell {
                    Some(s) => format!("cancelled in shell `{s}`"),
                    None => "cancelled".to_string(),
                };
                match span {
                    Some(span) => DiagnosticReport {
                        severity: Severity::Error,
                        message: msg,
                        labels: vec![(span.clone(), "cancelled here".to_string()).into()],
                        help: None,
                        note: None,
                    },
                    None => DiagnosticReport {
                        severity: Severity::Error,
                        message: msg,
                        labels: vec![],
                        help: None,
                        note: None,
                    },
                }
            }
        }
    }
}

pub fn log_link(run_dir: &Path, result: &TestResult) -> Option<String> {
    let log_dir = result.log_dir.as_ref()?;
    let relative = log_dir.strip_prefix(run_dir).ok()?;
    Some(format!("{}/events.json", relative.display()))
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub test_path: String,
    pub outcome: Outcome,
    pub duration: Duration,
    pub progress: String,
    pub log_dir: Option<PathBuf>,
    pub warnings: Vec<crate::effect::Warning>,
    pub flaky_retries: u32,
}

impl TestResult {
    pub fn is_failure(&self) -> bool {
        matches!(self.outcome, Outcome::Fail(_))
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Pass,
    Fail(Failure),
    Skipped(String),
    Invalid(String),
}

// ─── Run Report ─────────────────────────────────────────────

pub struct RunReport<'a> {
    pub results: &'a [TestResult],
    pub run_dir: &'a Path,
    pub wall_duration: Duration,
    pub jobs: usize,
}

impl RunReport<'_> {
    pub fn eprint(&self) {
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        let mut invalid = 0usize;
        let mut flaky_retries = 0u32;
        let mut total_duration = Duration::ZERO;

        for result in self.results {
            total_duration += result.duration;
            flaky_retries += result.flaky_retries;
            match &result.outcome {
                Outcome::Pass => passed += 1,
                Outcome::Fail(_) => failed += 1,
                Outcome::Skipped(_) => skipped += 1,
                Outcome::Invalid(_) => invalid += 1,
            }
        }

        let has_problems = failed > 0 || invalid > 0;
        let status = if has_problems {
            "FAILED".red().to_string()
        } else {
            "ok".green().to_string()
        };

        let mut summary = format!("\ntest result: {status}. {passed} passed; {failed} failed");
        if invalid > 0 {
            summary.push_str(&format!("; {invalid} invalid"));
        }
        if skipped > 0 {
            summary.push_str(&format!("; {skipped} skipped"));
        }
        if flaky_retries > 0 {
            summary.push_str(&format!("; {flaky_retries} flaky retries"));
        }
        if self.jobs > 1 {
            summary.push_str(&format!(
                "; finished in {} ({} cumulative)\n",
                format_duration(self.wall_duration),
                format_duration(total_duration)
            ));
        } else {
            summary.push_str(&format!(
                "; finished in {}\n",
                format_duration(self.wall_duration)
            ));
        }
        eprint!("{summary}");
        eprintln!(
            "  Test logs: file://{}",
            self.run_dir.join("index.html").display()
        );
        let _ = std::io::stderr().flush();
    }
}

pub fn format_duration(d: Duration) -> String {
    let total_ms = d.as_secs_f64() * 1000.0;
    if total_ms < 1000.0 {
        format!("{:.1} ms", total_ms)
    } else {
        format!("{:.1} s", total_ms / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dummy_span() -> IrSpan {
        IrSpan::synthetic()
    }

    #[test]
    fn summary_match_timeout() {
        let f = Failure::MatchTimeout {
            pattern: "/ready/".into(),
            shell: "default".into(),
            span: dummy_span(),
            context: FailureContext::default(),
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
            context: FailureContext::default(),
        };
        assert_eq!(
            f.summary(),
            "fail pattern matched in shell 'default': pattern /error/ triggered, matched: \"error: connection refused\""
        );
    }

    #[test]
    fn summary_shell_exited_with_code() {
        let f = Failure::ShellExited {
            shell: "default".into(),
            exit_code: Some(1),
            span: dummy_span(),
            context: FailureContext::default(),
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
            context: FailureContext::default(),
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
            context: FailureContext::default(),
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
            context: FailureContext::default(),
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

            progress: String::new(),
            log_dir: Some(PathBuf::from("/tmp/runs/run-001/my_test")),
            warnings: Vec::new(),
            flaky_retries: 0,
        };
        assert_eq!(
            log_link(run_dir, &result),
            Some("my_test/events.json".to_string())
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

            progress: String::new(),
            log_dir: None,
            warnings: Vec::new(),
            flaky_retries: 0,
        };
        assert_eq!(log_link(run_dir, &result), None);
    }
}
