//! Diagnostic and failure-translation emitters.
//!
//! Two-part module:
//!
//! - Per-event diagnostic pushers (`emit_annotate` / `emit_log` /
//!   `emit_warning` / `emit_error` / `emit_cancelled` /
//!   `emit_failure_progress`) that record a single diagnostic into the
//!   structured stream and post a corresponding progress sigil.
//! - The two translators (`failure_record` / `cancellation_record`)
//!   that flatten runtime `Failure` / `Cancellation` types into the
//!   on-disk `FailureRecord` / `CancellationRecord` shapes used by the
//!   viewer.

use relux_core::diagnostics::IrSpan;

use super::StructuredLogBuilder;
use crate::observe::progress::ProgressEvent;
use crate::observe::structured::event::CancelReasonRecord;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::failure::CancellationRecord;
use crate::observe::structured::failure::FailureRecord;
use crate::observe::structured::span::SpanId;

impl StructuredLogBuilder {
    pub fn emit_annotate(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        text: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Annotate {
                text: text.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Annotation(text.to_string()));
    }

    pub fn emit_log(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Log {
                message: message.to_string(),
            },
        );
    }

    pub fn emit_warning(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Warning {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Warning(message.to_string()));
    }

    pub fn emit_error(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        message: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Error {
                message: message.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Error(message.to_string()));
    }

    /// Emit a `cancelled` event on the span the VM was in when it observed
    /// the cancel token flipping. Carries the reason recorded by whoever
    /// called `cancel_with(...)`. Pushes a `C` sigil into the per-test
    /// progress sliding window so live TUI viewers see the cancel land
    /// in the same place errors and timeouts do.
    pub fn emit_cancelled(
        &self,
        span: SpanId,
        shell: Option<&str>,
        shell_marker: Option<&str>,
        reason: &crate::cancel::CancelReason,
    ) {
        self.push_event(
            span,
            shell,
            shell_marker,
            None,
            EventKind::Cancelled {
                reason: CancelReasonRecord::from(reason),
            },
        );
        self.push_progress(ProgressEvent::Cancellation);
    }

    /// Push a `Failure` progress notification only. The structured failure
    /// information is carried in the `FailureRecord` passed to `build()`.
    pub fn emit_failure_progress(&self) {
        self.push_progress(ProgressEvent::Failure);
    }

    /// Translate a runtime `Failure` into a `FailureRecord`, flattening the
    /// `FailureContext` enum into the on-disk shape via its accessor
    /// methods. `Vm` failures produce full diagnostic context; `PreVm`
    /// failures (effect-resolution errors, pre-VM init, cleanup-shell
    /// spawn) land with the surrounding span and empty stack / tail /
    /// vars — the artifact stays well-formed.
    pub fn failure_record(&self, failure: &crate::report::result::Failure) -> FailureRecord {
        use crate::report::result::Failure;
        match failure {
            Failure::MatchTimeout {
                pattern,
                shell,
                effective,
                context,
                ..
            } => FailureRecord::MatchTimeout {
                span: context.span().unwrap_or(0),
                event_seq: context.event_seq().unwrap_or(0),
                shell: shell.clone(),
                pattern: pattern.clone(),
                effective: self.timeout_value(effective),
                call_stack: context.call_stack().to_vec(),
                buffer_tail: context.buffer_tail().to_string(),
                vars_in_scope: context.vars_in_scope().to_vec(),
            },
            Failure::FailPatternMatched {
                pattern,
                matched_line,
                shell,
                context,
                ..
            } => FailureRecord::FailPatternMatched {
                span: context.span().unwrap_or(0),
                event_seq: context.event_seq().unwrap_or(0),
                shell: shell.clone(),
                pattern: pattern.clone(),
                matched_line: matched_line.clone(),
                call_stack: context.call_stack().to_vec(),
                buffer_tail: context.buffer_tail().to_string(),
                vars_in_scope: context.vars_in_scope().to_vec(),
            },
            Failure::ShellExited {
                shell,
                exit_code,
                context,
                ..
            } => FailureRecord::ShellExited {
                span: context.span().unwrap_or(0),
                event_seq: context.event_seq().unwrap_or(0),
                shell: shell.clone(),
                exit_code: *exit_code,
                call_stack: context.call_stack().to_vec(),
                buffer_tail: context.buffer_tail().to_string(),
                vars_in_scope: context.vars_in_scope().to_vec(),
            },
            Failure::Runtime {
                message,
                shell,
                context,
                ..
            } => FailureRecord::Runtime {
                span: context.span(),
                event_seq: context.event_seq(),
                shell: shell.clone(),
                message: message.clone(),
                call_stack: context.call_stack().to_vec(),
                vars_in_scope: context.vars_in_scope().to_vec(),
            },
        }
    }

    /// Translate a runtime `Cancellation` into a `CancellationRecord`.
    pub fn cancellation_record(
        &self,
        c: &crate::report::result::Cancellation,
    ) -> CancellationRecord {
        let ctx = &c.context;
        CancellationRecord {
            reason: CancelReasonRecord::from(&c.reason),
            span: ctx.span(),
            event_seq: ctx.event_seq(),
            shell: None,
            call_stack: ctx.call_stack().to_vec(),
        }
    }
}
