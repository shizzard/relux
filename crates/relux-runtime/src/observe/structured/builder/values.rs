//! Variable / interpolation / pure-eval event emitters.
//!
//! These cover both the shell-bound surface (`var-let` / `var-assign`
//! during shell-block execution, runtime interpolation, runtime string
//! eval) and the pure surface used by `LogSink` and marker replay
//! (interpolation (shell and pure surfaces), `var-read`, `pure-match`).

use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;

use super::StructuredLogBuilder;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::span::SpanId;

impl StructuredLogBuilder {
    pub fn emit_var_let(
        &self,
        span: SpanId,
        shell: Option<&str>,
        marker: Option<&str>,
        name: &str,
        value: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            shell,
            marker,
            location,
            EventKind::VarLet {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_var_assign(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        name: &str,
        value: &str,
        previous: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::VarAssign {
                name: name.to_string(),
                value: value.to_string(),
                previous: previous.to_string(),
            },
        );
    }

    pub fn emit_string_eval(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        result: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::StringEval {
                result: result.to_string(),
            },
        );
    }

    /// Emit an interpolation event. Shell callers pass `Some(shell)` /
    /// `Some(marker)`; pure callers (LogSink) pass `None` / `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_interpolation(
        &self,
        span: SpanId,
        shell: Option<&str>,
        marker: Option<&str>,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            shell,
            marker,
            location,
            EventKind::Interpolation {
                template: template.to_string(),
                result: result.to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    /// Pure variable-read event. Used by `LogSink` to surface bare
    /// `${X}`-style reads that resolve against scope/env. The result
    /// is the resolved string (`""` when the var is undefined).
    pub fn emit_var_read(&self, span: SpanId, name: &str, value: &str, location: Option<&IrSpan>) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::VarRead {
                name: name.to_string(),
                value: value.to_string(),
            },
        );
    }

    /// Pure string-match attempt. Emitted by `LogSink` before a marker `?`/`=`
    /// (and future pure-match statements) runs.
    pub fn emit_pure_match_start(
        &self,
        span: SpanId,
        value: &str,
        pattern: &str,
        is_regex: bool,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::PureMatchStart {
                value: value.to_string(),
                pattern: pattern.to_string(),
                is_regex,
            },
        );
    }

    /// Pure string-match success.
    pub fn emit_pure_match_done(
        &self,
        span: SpanId,
        matched: &str,
        captures: &HashMap<String, String>,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::PureMatchDone {
                matched: matched.to_string(),
                captures: captures.clone(),
            },
        );
    }

    /// Pure string-match failure (no match).
    pub fn emit_pure_match_failed(&self, span: SpanId, location: Option<&IrSpan>) {
        self.push_event(span, None, None, location, EventKind::PureMatchFailed {});
    }
}
