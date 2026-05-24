//! Match, timeout, fail-pattern, and timeout-config emitters.
//!
//! Anything that participates in the "wait for output" half of the
//! send/match contract: arming/disarming fail patterns, opening a
//! match attempt, recording its resolution (matched or timed out), and
//! pushing a `TimeoutSet` event whenever the active match timeout changes.

use std::collections::HashMap;
use std::time::Duration;

use relux_core::diagnostics::IrSpan;
use relux_ir::IrTimeout;

use super::StructuredLogBuilder;
use crate::observe::progress::ProgressEvent;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::event::EventSeq;
use crate::observe::structured::span::SpanId;

impl StructuredLogBuilder {
    #[allow(clippy::too_many_arguments)]
    pub fn emit_match_start(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        effective: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let effective = self.timeout_value(effective);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MatchStart {
                pattern: pattern.to_string(),
                is_regex,
                effective,
            },
        );
        self.push_progress(ProgressEvent::MatchStart);
    }

    /// Record a structured `MatchDone` event referencing a buffer event
    /// that was pushed (atomically with the consume operation) by
    /// `OutputBuffer::consume_*`. The buffer event push is the consumer's
    /// responsibility - this method only emits the structured event +
    /// progress notification.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_match_done_record(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        matched: &str,
        elapsed: Duration,
        captures: Option<HashMap<String, String>>,
        buffer_seq: EventSeq,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MatchDone {
                matched: matched.to_string(),
                elapsed,
                captures,
                buffer_seq,
            },
        );
        self.push_progress(ProgressEvent::MatchDone);
    }

    pub fn emit_timeout(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        effective: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let effective = self.timeout_value(effective);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Timeout {
                pattern: pattern.to_string(),
                buffer_seq: None,
                effective,
            },
        );
        self.push_progress(ProgressEvent::Timeout);
    }

    pub fn emit_fail_pattern_set(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::FailPatternSet {
                pattern: pattern.to_string(),
                is_regex,
            },
        );
    }

    pub fn emit_fail_pattern_cleared(
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
            EventKind::FailPatternCleared,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_fail_pattern_triggered(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        pattern: &str,
        is_regex: bool,
        matched_line: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::FailPatternTriggered {
                pattern: pattern.to_string(),
                is_regex,
                matched_line: matched_line.to_string(),
                buffer_seq: None,
            },
        );
        self.push_progress(ProgressEvent::FailPattern);
    }

    pub fn emit_timeout_set(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        timeout: &IrTimeout,
        previous: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let timeout = self.timeout_value(timeout);
        let previous = self.timeout_value(previous);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::TimeoutSet { timeout, previous },
        );
    }

    /// Emit the `multi-match-start` event for `<{ ... }`. `patterns` is in
    /// source order; the runtime will reference these indices from later
    /// `multi-match-pattern-done` and `multi-match-timeout` events.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_multimatch_start(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        patterns: &[crate::observe::structured::event::MultiMatchPattern],
        effective: &IrTimeout,
        location: Option<&IrSpan>,
    ) {
        let effective = self.timeout_value(effective);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MultiMatchStart {
                effective,
                patterns: patterns.to_vec(),
            },
        );
    }

    /// Emit a `multi-match-pattern-done` event for a single pattern within
    /// an in-flight multimatch block. `buffer_seq` references the
    /// `BufferEventKind::Matched` event pushed atomically with the
    /// pattern's success under the buf lock; that event carries the
    /// matched text and offsets (single source of truth).
    #[allow(clippy::too_many_arguments)]
    pub fn emit_multimatch_pattern_done(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        index: usize,
        elapsed: Duration,
        buffer_seq: EventSeq,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MultiMatchPatternDone {
                index,
                elapsed,
                buffer_seq,
            },
        );
    }

    /// Emit a `multi-match-done` event - the block satisfied every
    /// pattern. `advance_to` references the per-pattern `Matched` buffer
    /// event whose match ends farthest in the buffer (the single drain
    /// the runtime applies at block exit advances the cursor by
    /// `len(before) + len(matched)` of that referenced event).
    pub fn emit_multimatch_done(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        advance_to: EventSeq,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MultiMatchDone { advance_to },
        );
    }

    /// Emit a `multi-match-timeout` event - the block deadline expired
    /// with at least one pattern still unmatched. `unmatched` is a list
    /// of indices into the original `MultiMatchStart.patterns` vec.
    pub fn emit_multimatch_timeout(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        unmatched: &[usize],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::MultiMatchTimeout {
                unmatched: unmatched.to_vec(),
            },
        );
        self.push_progress(ProgressEvent::Timeout);
    }
}
