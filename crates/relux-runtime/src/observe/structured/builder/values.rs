//! Variable / interpolation / pure-eval event emitters.
//!
//! These cover both the shell-bound surface (`var-let` / `var-assign`
//! during shell-block execution, runtime interpolation, runtime string
//! eval) and the pure surface used by `LogSink` and marker replay
//! (`pure-interpolation`, `var-read`, `pure-match`).

use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;

use super::StructuredLogBuilder;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::span::MatchKind;
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

    #[allow(clippy::too_many_arguments)]
    pub fn emit_interpolation(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::Interpolation {
                template: template.to_string(),
                result: result.to_string(),
                bindings: bindings.to_vec(),
            },
        );
    }

    /// Interpolation event emitted from a pure-eval context (no shell,
    /// no shell marker). Used by `LogSink` for marker replay and for
    /// test/effect-level lets.
    pub fn emit_pure_interpolation(
        &self,
        span: SpanId,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
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

    /// Pure string-match event. Used by `LogSink` for marker `?`
    /// regex conditions and the future runtime string-match syntax.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_pure_match(
        &self,
        span: SpanId,
        match_kind: MatchKind,
        value: &str,
        pattern: &str,
        result: &str,
        captures: &HashMap<String, String>,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::PureMatch {
                match_kind,
                value: value.to_string(),
                pattern: pattern.to_string(),
                result: result.to_string(),
                captures: captures.clone(),
            },
        );
    }
}
