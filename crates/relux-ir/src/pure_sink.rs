use std::collections::HashMap;

use relux_core::diagnostics::IrSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Regex,
    Literal,
}

pub trait PureEvalSink {
    fn enter_pure_fn(
        &mut self,
        name: &str,
        args: &[(String, String)],
        is_builtin: bool,
        span: &IrSpan,
    );
    fn leave_pure_fn(&mut self, result: &str);
    fn record_interpolation(
        &mut self,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        span: &IrSpan,
    );
    fn record_match(
        &mut self,
        kind: MatchKind,
        value: &str,
        pattern: &str,
        result: &str,
        captures: &HashMap<String, String>,
        span: &IrSpan,
    );
    /// Top-level bare variable read: the name being resolved and the
    /// final string value (`""` when the var is undefined). Only fired
    /// when the var is the *whole* expression; bindings already inside
    /// an interpolation are captured by `record_interpolation` so we
    /// don't double-emit.
    fn record_var_read(&mut self, name: &str, value: &str, span: &IrSpan);
}

pub struct NoOpSink;

impl PureEvalSink for NoOpSink {
    fn enter_pure_fn(&mut self, _: &str, _: &[(String, String)], _: bool, _: &IrSpan) {}
    fn leave_pure_fn(&mut self, _: &str) {}
    fn record_interpolation(&mut self, _: &str, _: &str, _: &[(String, String)], _: &IrSpan) {}
    fn record_match(
        &mut self,
        _: MatchKind,
        _: &str,
        _: &str,
        _: &str,
        _: &HashMap<String, String>,
        _: &IrSpan,
    ) {
    }
    fn record_var_read(&mut self, _: &str, _: &str, _: &IrSpan) {}
}

#[derive(Debug, Clone)]
pub enum SinkOp {
    EnterPureFn {
        name: String,
        args: Vec<(String, String)>,
        is_builtin: bool,
        span: IrSpan,
    },
    LeavePureFn {
        result: String,
    },
    RecordInterpolation {
        template: String,
        result: String,
        bindings: Vec<(String, String)>,
        span: IrSpan,
    },
    Match {
        kind: MatchKind,
        value: String,
        pattern: String,
        result: String,
        captures: HashMap<String, String>,
        span: IrSpan,
    },
    VarRead {
        name: String,
        value: String,
        span: IrSpan,
    },
}

#[derive(Debug, Default, Clone)]
pub struct RecordingSink {
    pub ops: Vec<SinkOp>,
}

impl PureEvalSink for RecordingSink {
    fn enter_pure_fn(
        &mut self,
        name: &str,
        args: &[(String, String)],
        is_builtin: bool,
        span: &IrSpan,
    ) {
        self.ops.push(SinkOp::EnterPureFn {
            name: name.to_string(),
            args: args.to_vec(),
            is_builtin,
            span: span.clone(),
        });
    }

    fn leave_pure_fn(&mut self, result: &str) {
        self.ops.push(SinkOp::LeavePureFn {
            result: result.to_string(),
        });
    }

    fn record_interpolation(
        &mut self,
        template: &str,
        result: &str,
        bindings: &[(String, String)],
        span: &IrSpan,
    ) {
        self.ops.push(SinkOp::RecordInterpolation {
            template: template.to_string(),
            result: result.to_string(),
            bindings: bindings.to_vec(),
            span: span.clone(),
        });
    }

    fn record_match(
        &mut self,
        kind: MatchKind,
        value: &str,
        pattern: &str,
        result: &str,
        captures: &HashMap<String, String>,
        span: &IrSpan,
    ) {
        self.ops.push(SinkOp::Match {
            kind,
            value: value.to_string(),
            pattern: pattern.to_string(),
            result: result.to_string(),
            captures: captures.clone(),
            span: span.clone(),
        });
    }

    fn record_var_read(&mut self, name: &str, value: &str, span: &IrSpan) {
        self.ops.push(SinkOp::VarRead {
            name: name.to_string(),
            value: value.to_string(),
            span: span.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_sink_captures_all_op_kinds_in_order() {
        let mut sink = RecordingSink::default();
        sink.enter_pure_fn(
            "trim",
            &[("$0".into(), "hi".into())],
            true,
            &IrSpan::synthetic(),
        );
        sink.leave_pure_fn("hi");
        sink.record_interpolation(
            "${x}",
            "hi",
            &[("x".into(), "hi".into())],
            &IrSpan::synthetic(),
        );
        let mut caps = HashMap::new();
        caps.insert("0".to_string(), "matched".to_string());
        sink.record_match(
            MatchKind::Regex,
            "the value",
            "^the .*$",
            "the value",
            &caps,
            &IrSpan::synthetic(),
        );
        assert_eq!(sink.ops.len(), 4);
        assert!(matches!(sink.ops[0], SinkOp::EnterPureFn { .. }));
        assert!(matches!(sink.ops[1], SinkOp::LeavePureFn { .. }));
        assert!(matches!(sink.ops[2], SinkOp::RecordInterpolation { .. }));
        assert!(matches!(
            sink.ops[3],
            SinkOp::Match {
                kind: MatchKind::Regex,
                ..
            }
        ));
    }

    #[test]
    fn noop_sink_is_silent() {
        let mut sink = NoOpSink;
        sink.enter_pure_fn("trim", &[], true, &IrSpan::synthetic());
        sink.leave_pure_fn("");
        sink.record_interpolation("x", "x", &[], &IrSpan::synthetic());
        sink.record_match(
            MatchKind::Regex,
            "",
            "",
            "",
            &HashMap::new(),
            &IrSpan::synthetic(),
        );
        sink.record_var_read("x", "", &IrSpan::synthetic());
    }

    #[test]
    fn recording_sink_captures_span_on_each_op() {
        use relux_core::Span as CoreSpan;
        use relux_core::table::FileId;
        use std::path::PathBuf;

        let mut sink = RecordingSink::default();
        let fid = FileId::new(PathBuf::from("x.relux"));
        let span1 = IrSpan::new(fid.clone(), CoreSpan::new(0, 1));
        let span2 = IrSpan::new(fid, CoreSpan::new(5, 9));

        sink.record_interpolation("${x}", "v", &[("x".into(), "v".into())], &span1);
        sink.record_var_read("y", "z", &span2);

        match &sink.ops[0] {
            SinkOp::RecordInterpolation { span, .. } => assert_eq!(span.span().start(), 0),
            other => panic!("expected RecordInterpolation, got {other:?}"),
        }
        match &sink.ops[1] {
            SinkOp::VarRead { span, .. } => assert_eq!(span.span().start(), 5),
            other => panic!("expected VarRead, got {other:?}"),
        }
    }
}
