//! `eval_pure_match` - the single site that runs a pure match and records
//! its event trio. Wraps `relux_core::pure::matching::pure_match`, emitting
//! `PureMatchStart` paired with `PureMatchDone`/`PureMatchFailed` around a
//! completed attempt, and nothing on a malformed pattern. Every consumer
//! (markers now, pure-match statements later) goes through here.

use relux_core::diagnostics::IrSpan;
use relux_core::pure::matching::PureMatchError;
use relux_core::pure::matching::PureMatchHit;
use relux_core::pure::matching::pure_match;

use crate::pure_sink::PureEvalSink;

pub fn eval_pure_match(
    sink: &mut dyn PureEvalSink,
    value: &str,
    pattern: &str,
    is_regex: bool,
    span: &IrSpan,
) -> Result<Option<PureMatchHit>, PureMatchError> {
    match pure_match(value, pattern, is_regex) {
        Ok(Some(hit)) => {
            sink.record_pure_match_start(value, pattern, is_regex, span);
            sink.record_pure_match_done(&hit.matched_text, &hit.captures, span);
            Ok(Some(hit))
        }
        Ok(None) => {
            sink.record_pure_match_start(value, pattern, is_regex, span);
            sink.record_pure_match_failed(span);
            Ok(None)
        }
        // Malformed regex (interpolated patterns only): emit nothing, so no
        // orphan `PureMatchStart` lands in the log. The caller surfaces the
        // pattern as a failure (statements) or lowering-invalid (markers).
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pure_sink::RecordingSink;
    use crate::pure_sink::SinkOp;
    use relux_core::table::FileId;

    fn span() -> IrSpan {
        IrSpan::new(
            FileId::new(std::path::PathBuf::from("/t.relux")),
            relux_core::Span::new(0, 1),
        )
    }

    #[test]
    fn malformed_regex_emits_no_events() {
        let mut sink = RecordingSink::default();
        let err = eval_pure_match(&mut sink, "abc", "(", true, &span()).unwrap_err();
        assert_eq!(err.pattern, "(");
        assert!(
            sink.ops.is_empty(),
            "no start/done/failed on a malformed pattern"
        );
    }

    #[test]
    fn no_match_emits_start_then_failed() {
        let mut sink = RecordingSink::default();
        assert!(
            eval_pure_match(&mut sink, "abc", "xyz", false, &span())
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            sink.ops.as_slice(),
            [
                SinkOp::PureMatchStart { .. },
                SinkOp::PureMatchFailed { .. }
            ]
        ));
    }

    #[test]
    fn hit_emits_start_then_done() {
        let mut sink = RecordingSink::default();
        assert!(
            eval_pure_match(&mut sink, "abc", "abc", false, &span())
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            sink.ops.as_slice(),
            [SinkOp::PureMatchStart { .. }, SinkOp::PureMatchDone { .. }]
        ));
    }
}
