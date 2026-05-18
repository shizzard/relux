use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use colored::Colorize;

use relux_core::diagnostics::IrSpan;
use relux_ir::IrTimeout;

use crate::cancel::CancelReason;
use crate::observe::structured::EventSeq;
use crate::observe::structured::SpanId;
use crate::observe::structured::StackFrame;

/// Diagnostic context captured at failure-construction time. Travels with
/// every `Failure` so that downstream consumers (structured-log artifact,
/// console error renderer) can render the call site, what arrived in the
/// shell, and which user vars were live — without needing to reach back
/// into a VM that is about to be dropped.
///
/// The variant makes the failure's provenance explicit. `Vm` carries the
/// full diagnostic picture; `PreVm` represents failures raised outside any
/// VM (effect resolution, shell-block lookup, cleanup-shell spawn,
/// pre-init PTY spawn) and carries only the surrounding span, when one is
/// known. The structured-log builder flattens both variants into a single
/// on-disk shape via the accessor methods.
#[derive(Debug, Clone)]
pub enum FailureContext {
    /// Captured by a running VM at failure-construction time.
    Vm {
        span: SpanId,
        event_seq: EventSeq,
        call_stack: Vec<StackFrame>,
        buffer_tail: String,
        vars_in_scope: Vec<(String, String)>,
    },
    /// Failure raised before/around any VM. `span` points at the
    /// surrounding span when one is known (effect-setup span,
    /// shell-block span, cleanup-block span), `None` otherwise.
    PreVm { span: Option<SpanId> },
}

impl FailureContext {
    /// Construct a `PreVm` context with no surrounding span.
    pub fn pre_vm() -> Self {
        Self::PreVm { span: None }
    }

    /// Construct a `PreVm` context tied to a known surrounding span.
    pub fn pre_vm_with_span(span: SpanId) -> Self {
        Self::PreVm { span: Some(span) }
    }

    pub fn span(&self) -> Option<SpanId> {
        match self {
            Self::Vm { span, .. } => Some(*span),
            Self::PreVm { span } => *span,
        }
    }

    pub fn event_seq(&self) -> Option<EventSeq> {
        match self {
            Self::Vm { event_seq, .. } => Some(*event_seq),
            Self::PreVm { .. } => None,
        }
    }

    pub fn call_stack(&self) -> &[StackFrame] {
        match self {
            Self::Vm { call_stack, .. } => call_stack,
            Self::PreVm { .. } => &[],
        }
    }

    pub fn buffer_tail(&self) -> &str {
        match self {
            Self::Vm { buffer_tail, .. } => buffer_tail,
            Self::PreVm { .. } => "",
        }
    }

    pub fn vars_in_scope(&self) -> &[(String, String)] {
        match self {
            Self::Vm { vars_in_scope, .. } => vars_in_scope,
            Self::PreVm { .. } => &[],
        }
    }
}

#[derive(Debug, Clone)]
pub enum Failure {
    MatchTimeout {
        pattern: String,
        span: IrSpan,
        shell: String,
        /// The timeout that fired. Boxed to keep `Failure`'s variant size
        /// comparable to the others (avoids `clippy::large_enum_variant`).
        effective: Box<IrTimeout>,
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
        }
    }

    pub fn failure_type(&self) -> &'static str {
        match self {
            Failure::MatchTimeout { .. } => "MatchTimeout",
            Failure::FailPatternMatched { .. } => "FailPatternMatched",
            Failure::ShellExited { .. } => "ShellExited",
            Failure::Runtime { .. } => "Runtime",
        }
    }

    pub fn context(&self) -> &FailureContext {
        match self {
            Failure::MatchTimeout { context, .. }
            | Failure::FailPatternMatched { context, .. }
            | Failure::ShellExited { context, .. }
            | Failure::Runtime { context, .. } => context,
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
        }
    }
}

pub fn log_link(run_dir: &Path, result: &TestResult) -> Option<String> {
    let log_dir = result.log_dir.as_ref()?;
    let relative = log_dir.strip_prefix(run_dir).ok()?;
    Some(format!("{}/event.html", relative.display()))
}

/// Companion to `log_link` for the canonical structured artifact.
/// Returns `<log_dir>/events.json` relative to `run_dir`. Machine
/// consumers (custom reporters, dashboards) prefer this over the
/// human-targeted `event.html`.
pub fn events_json_link(run_dir: &Path, result: &TestResult) -> Option<String> {
    let log_dir = result.log_dir.as_ref()?;
    let relative = log_dir.strip_prefix(run_dir).ok()?;
    Some(format!("{}/events.json", relative.display()))
}

/// Top-level marker that the test was interrupted before completing.
/// Distinct from `Failure` because the test did not misbehave — it was
/// stopped by an external event (the per-test watchdog, the suite-wide
/// watchdog, fail-fast, or SIGINT).
#[derive(Debug, Clone)]
pub struct Cancellation {
    pub reason: CancelReason,
    pub context: FailureContext,
}

impl Cancellation {
    pub fn summary(&self) -> String {
        match &self.reason {
            CancelReason::TestTimeout { duration } => {
                format!("cancelled: test timed out after {duration:?}")
            }
            CancelReason::SuiteTimeout { duration } => {
                format!("cancelled: suite timed out after {duration:?}")
            }
            CancelReason::FailFast { trigger_test } => {
                format!("cancelled: suite stopped after `{trigger_test}` failed (fail-fast)")
            }
            CancelReason::Sigint => "cancelled: interrupted (SIGINT)".to_string(),
        }
    }

    pub fn reason_tag(&self) -> &'static str {
        match &self.reason {
            CancelReason::TestTimeout { .. } => "test-timeout",
            CancelReason::SuiteTimeout { .. } => "suite-timeout",
            CancelReason::FailFast { .. } => "fail-fast",
            CancelReason::Sigint => "sigint",
        }
    }
}

impl From<&Cancellation> for relux_core::error::DiagnosticReport {
    fn from(c: &Cancellation) -> Self {
        relux_core::error::DiagnosticReport {
            severity: relux_core::error::Severity::Error,
            message: c.summary(),
            labels: vec![],
            help: None,
            note: None,
        }
    }
}

/// Internal error type used by the VM / BIF / effect machinery while a test
/// is running. `Failure` is "the test misbehaved"; `Cancelled` is "we were
/// stopped from the outside". `run_test` maps each variant onto the
/// corresponding `Outcome`.
#[derive(Debug, Clone)]
pub enum ExecError {
    Failure(Failure),
    Cancelled(Cancellation),
}

impl From<Failure> for ExecError {
    fn from(f: Failure) -> Self {
        ExecError::Failure(f)
    }
}

impl From<Cancellation> for ExecError {
    fn from(c: Cancellation) -> Self {
        ExecError::Cancelled(c)
    }
}

impl ExecError {
    pub fn summary(&self) -> String {
        match self {
            ExecError::Failure(f) => f.summary(),
            ExecError::Cancelled(c) => c.summary(),
        }
    }
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

    pub fn is_cancelled(&self) -> bool {
        matches!(self.outcome, Outcome::Cancelled(_))
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Pass,
    Fail(Failure),
    Cancelled(Cancellation),
    Skipped(String),
    Invalid(String),
}

impl Outcome {
    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::Fail(_))
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Outcome::Cancelled(_))
    }

    pub fn is_nonzero_outcome(&self) -> bool {
        matches!(
            self,
            Outcome::Fail(_) | Outcome::Cancelled(_) | Outcome::Invalid(_)
        )
    }

    /// Whether the flaky-retry loop should retry on this outcome. Real
    /// failures and per-test-timeout cancellations are retryable (those are
    /// the test's own clock running out — exactly what flaky retries with
    /// scaled timeouts target). External cancellations (suite-timeout,
    /// fail-fast, SIGINT) are not retryable: rerunning the same test isn't
    /// going to make the external trigger disappear.
    pub fn is_retryable(&self) -> bool {
        match self {
            Outcome::Fail(_) => true,
            Outcome::Cancelled(c) => {
                matches!(c.reason, crate::cancel::CancelReason::TestTimeout { .. })
            }
            _ => false,
        }
    }
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
        let mut cancelled = 0usize;
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
                Outcome::Cancelled(_) => cancelled += 1,
                Outcome::Skipped(_) => skipped += 1,
                Outcome::Invalid(_) => invalid += 1,
            }
        }

        let has_problems = failed > 0 || cancelled > 0 || invalid > 0;
        let status = if has_problems {
            "FAILED".red().to_string()
        } else {
            "ok".green().to_string()
        };

        let mut summary = format!("\ntest result: {status}. {passed} passed; {failed} failed");
        if cancelled > 0 {
            summary.push_str(&format!("; {cancelled} cancelled"));
        }
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
            effective: Box::new(IrTimeout::tolerance(std::time::Duration::from_secs(5))),
            context: FailureContext::pre_vm(),
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
            context: FailureContext::pre_vm(),
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
            context: FailureContext::pre_vm(),
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
            context: FailureContext::pre_vm(),
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
            context: FailureContext::pre_vm(),
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
            context: FailureContext::pre_vm(),
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
            Some("my_test/event.html".to_string())
        );
    }

    #[test]
    fn cancellation_summary_test_timeout() {
        let c = Cancellation {
            reason: CancelReason::TestTimeout {
                duration: Duration::from_millis(300),
            },
            context: FailureContext::pre_vm(),
        };
        assert_eq!(c.reason_tag(), "test-timeout");
        assert!(c.summary().starts_with("cancelled: test timed out after"));
    }

    #[test]
    fn cancellation_summary_fail_fast() {
        let c = Cancellation {
            reason: CancelReason::FailFast {
                trigger_test: "foo".into(),
            },
            context: FailureContext::pre_vm(),
        };
        assert_eq!(c.reason_tag(), "fail-fast");
        assert!(c.summary().contains("`foo`"));
    }

    #[test]
    fn exec_error_from_conversions() {
        let f = Failure::Runtime {
            message: "x".into(),
            span: None,
            shell: None,
            context: FailureContext::pre_vm(),
        };
        let e: ExecError = f.into();
        assert!(matches!(e, ExecError::Failure(_)));

        let c = Cancellation {
            reason: CancelReason::Sigint,
            context: FailureContext::pre_vm(),
        };
        let e: ExecError = c.into();
        assert!(matches!(e, ExecError::Cancelled(_)));
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
