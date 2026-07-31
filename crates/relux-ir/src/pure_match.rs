//! `eval_pure_match` - the single site that runs a pure match and records
//! its event trio. Wraps `relux_core::pure::matching::pure_match`, emitting
//! `PureMatchStart` before the attempt and `PureMatchDone`/`PureMatchFailed`
//! after, so the timeline always shows the attempt. Every consumer (markers
//! now, pure-match statements later) goes through here.

use relux_core::diagnostics::IrSpan;
use relux_core::pure::matching::PureMatchError;
use relux_core::pure::matching::PureMatchHit;
use relux_core::pure::matching::pure_match;

use crate::pure_sink::PureEvalSink;

pub fn eval_pure_match(
    sink: &mut impl PureEvalSink,
    value: &str,
    pattern: &str,
    is_regex: bool,
    span: &IrSpan,
) -> Result<Option<PureMatchHit>, PureMatchError> {
    sink.record_pure_match_start(value, pattern, is_regex, span);
    match pure_match(value, pattern, is_regex) {
        Ok(Some(hit)) => {
            sink.record_pure_match_done(&hit.matched_text, &hit.captures, span);
            Ok(Some(hit))
        }
        Ok(None) => {
            sink.record_pure_match_failed(span);
            Ok(None)
        }
        // Malformed regex: the start event stands; the caller turns this
        // into a lowering-invalid, so no done/failed event is emitted.
        Err(e) => Err(e),
    }
}
