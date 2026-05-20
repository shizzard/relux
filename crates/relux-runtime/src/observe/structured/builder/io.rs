//! Send / recv / sleep event emitters.
//!
//! These are the thinnest emitters in the builder: bytes moving in or
//! out of a PTY, plus the structural start/end of a `sleep` BIF call.

use std::time::Duration;

use relux_core::diagnostics::IrSpan;

use super::StructuredLogBuilder;
use crate::observe::progress::ProgressEvent;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::span::SpanId;

impl StructuredLogBuilder {
    pub fn emit_send(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        data: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Send {
                data: data.to_string(),
            },
        );
        self.push_progress(ProgressEvent::Send);
    }

    pub fn emit_recv(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        data: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Recv {
                data: data.to_string(),
            },
        );
    }

    pub fn emit_sleep_start(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        duration: Duration,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::SleepStart { duration },
        );
        self.push_progress(ProgressEvent::SleepStart);
    }

    pub fn emit_sleep_done(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::SleepDone,
        );
        self.push_progress(ProgressEvent::SleepDone);
    }
}
