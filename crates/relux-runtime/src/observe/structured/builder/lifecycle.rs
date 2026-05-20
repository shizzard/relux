//! Span and shell lifecycle emitters.
//!
//! Methods that open/close spans, mutate the spans glossary, and record
//! shell spawn/terminate timestamps. Also home to the effect-expose
//! emitters, which announce the rebinding of an effect output into the
//! consumer's scope.

use relux_core::diagnostics::IrSpan;

use super::SpanGuard;
use super::StructuredLogBuilder;
use crate::observe::progress::ProgressEvent;
use crate::observe::structured::event::EventKind;
use crate::observe::structured::event::EventSeq;
use crate::observe::structured::failure::StackFrame;
use crate::observe::structured::shell::ShellRecord;
use crate::observe::structured::span::Span;
use crate::observe::structured::span::SpanId;
use crate::observe::structured::span::SpanKind;

impl StructuredLogBuilder {
    // ─── Span lifecycle ───────────────────────────────────────────

    /// Open a span and return a guard that closes it on drop. The caller
    /// must keep the guard alive for the span's lifetime; passing the id
    /// (`guard.id()`) to children is fine. Drop on `?` propagation closes
    /// cleanly; for a tighter `end_ts`, use `SpanGuard::close()` explicitly.
    pub fn open_span(
        &self,
        kind: SpanKind,
        parent: Option<SpanId>,
        location: Option<&IrSpan>,
    ) -> SpanGuard {
        let location = location.and_then(|s| self.resolve_location(s));
        let start_ts = self.now();
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.next_span_id;
            inner.next_span_id += 1;
            inner.spans.insert(
                id,
                Span {
                    id,
                    kind,
                    parent,
                    start_ts,
                    end_ts: None,
                    location,
                },
            );
            id
        };
        SpanGuard::new(id, self.clone())
    }

    /// First close wins. The `SpanGuard`'s `Drop` always calls into here
    /// (so `?` early-returns still get an `end_ts`), but call sites may
    /// also close a span explicitly via id when its semantic end happens
    /// well before the guard would naturally drop — e.g., a failing
    /// effect's setup span needs to close before its `try_guards!`
    /// awaits `run_effect_cleanup`, or the guard would sit on the stack
    /// through the entire cleanup phase and end up with a misleading
    /// `end_ts` near test-end. Making this idempotent lets the guard
    /// drop later as a no-op.
    pub(super) fn close_span_inner(&self, id: SpanId) {
        let end_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(span) = inner.spans.get_mut(&id)
            && span.end_ts.is_none()
        {
            span.end_ts = Some(end_ts);
        }
    }

    /// Close a span by id. Idempotent — see `close_span_inner`. Used to
    /// pin a span's `end_ts` to its actual semantic boundary when the
    /// owning `SpanGuard`'s drop point would otherwise be deferred (the
    /// failing-effect path through `try_guards!` is the canonical case).
    pub fn close_span(&self, id: SpanId) {
        self.close_span_inner(id);
    }

    /// Attach a return value to an in-flight `FnCall` span. Called from
    /// `exec_call` on the success path before the span closes; failed calls
    /// leave `result` as `None` so the row title falls back to `name/arity`.
    pub fn set_fn_call_result(&self, id: SpanId, result: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(span) = inner.spans.get_mut(&id)
            && let SpanKind::FnCall { result: slot, .. } = &mut span.kind
        {
            *slot = Some(result.to_string());
        }
    }

    /// Walk parent pointers from `leaf` back to a root span and return the
    /// frames in root-to-leaf order. Used at failure-construction time to
    /// snapshot the active call chain.
    pub fn resolve_stack(&self, leaf: SpanId) -> Vec<StackFrame> {
        let inner = self.inner.lock().unwrap();
        let mut chain: Vec<StackFrame> = Vec::new();
        let mut next = Some(leaf);
        while let Some(id) = next {
            let Some(span) = inner.spans.get(&id) else {
                break;
            };
            let (name, args) = span.kind.frame_data();
            chain.push(StackFrame {
                span: id,
                kind: span.kind.kind_str().to_string(),
                name,
                args,
                alias: span.kind.frame_alias(),
                location: span.location.clone(),
            });
            next = span.parent;
        }
        chain.reverse();
        chain
    }

    /// Open the synthetic `markers` root span. Always opened (per
    /// design); viewer filters out empty markers roots.
    pub fn open_markers_span(&self, location: Option<&IrSpan>) -> SpanGuard {
        self.open_span(SpanKind::Markers, None, location)
    }

    /// Open a `marker-eval` span as a child of a `markers` root.
    pub fn open_marker_eval_span(
        &self,
        parent: SpanId,
        marker_kind: super::super::span::MarkerEvalKind,
        modifier: super::super::span::MarkerEvalModifier,
        decision: super::super::span::MarkerEvalDecision,
        location: Option<&IrSpan>,
    ) -> SpanGuard {
        self.open_span(
            SpanKind::MarkerEval {
                marker_kind,
                modifier,
                decision,
            },
            Some(parent),
            location,
        )
    }

    /// Emit the final truthy/falsy outcome event inside a marker-eval
    /// span. Mirrors the shape stored on `MarkerRecording.evaluation`.
    /// Returns the emitted event's `EventSeq` so callers (e.g. `replay_markers`)
    /// can use it as a focus pointer.
    pub fn emit_bool_check(
        &self,
        span: SpanId,
        evaluation: super::super::span::MarkerEvalDetail,
        location: Option<&IrSpan>,
    ) -> EventSeq {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::BoolCheck { evaluation },
        )
    }

    // ─── Shells glossary ──────────────────────────────────────────

    pub fn record_shell_spawn(&self, marker: &str, name: &str, command: &str) {
        let spawn_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        inner.shells.insert(
            marker.to_string(),
            ShellRecord {
                marker: marker.to_string(),
                name: name.to_string(),
                spawn_ts,
                terminate_ts: None,
                command: command.to_string(),
            },
        );
    }

    pub fn record_shell_terminate(&self, marker: &str) {
        let terminate_ts = self.now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(rec) = inner.shells.get_mut(marker) {
            rec.terminate_ts = Some(terminate_ts);
        }
    }

    // ─── Shell lifecycle emitters ─────────────────────────────────

    pub fn emit_shell_spawn(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        command: &str,
        location: Option<&IrSpan>,
    ) {
        self.record_shell_spawn(marker, shell, command);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellSpawn {
                name: shell.to_string(),
                command: command.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSpawn);
    }

    pub fn emit_shell_ready(
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
            EventKind::ShellReady {
                name: shell.to_string(),
            },
        );
    }

    pub fn emit_shell_switch(
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
            EventKind::ShellSwitch {
                name: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellSwitch(shell.to_string()));
    }

    pub fn emit_shell_terminate(
        &self,
        span: SpanId,
        shell: &str,
        marker: &str,
        location: Option<&IrSpan>,
    ) {
        self.record_shell_terminate(marker);
        self.push_event(
            span,
            Some(shell),
            Some(marker),
            location,
            EventKind::ShellTerminate {
                name: shell.to_string(),
            },
        );
        self.push_progress(ProgressEvent::ShellTerminate);
    }

    // ─── Progress-only emitters (no structured event; the surrounding
    // span already carries the full information). Used to surface
    // lifecycle brackets on the live progress line.

    pub fn push_fn_enter(&self, name: &str) {
        self.push_progress(ProgressEvent::FnEnter(name.to_string()));
    }

    pub fn push_fn_exit(&self) {
        self.push_progress(ProgressEvent::FnExit);
    }

    pub fn push_effect_setup(&self, name: &str) {
        self.push_progress(ProgressEvent::EffectSetup(name.to_string()));
    }

    pub fn push_effect_teardown(&self) {
        self.push_progress(ProgressEvent::EffectTeardown);
    }

    // ─── Effect exposes ───────────────────────────────────────────

    pub fn emit_effect_expose_shell(
        &self,
        span: SpanId,
        name: &str,
        target: &str,
        qualifier: Option<&str>,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::EffectExposeShell {
                name: name.to_string(),
                target: target.to_string(),
                qualifier: qualifier.map(String::from),
            },
        );
    }

    pub fn emit_effect_expose_var(
        &self,
        span: SpanId,
        name: &str,
        target: &str,
        qualifier: Option<&str>,
        value: &str,
        location: Option<&IrSpan>,
    ) {
        self.push_event(
            span,
            None,
            None,
            location,
            EventKind::EffectExposeVar {
                name: name.to_string(),
                target: target.to_string(),
                qualifier: qualifier.map(String::from),
                value: value.to_string(),
            },
        );
    }
}
