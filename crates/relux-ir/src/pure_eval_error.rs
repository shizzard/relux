//! The error raised by pure evaluation.
//!
//! Pure evaluation became fallible the moment a pure match could appear
//! inside a `pure fn` body: a no-match must stop evaluation and travel out
//! to the boundary as a test failure. `PureMatchFailed` carries the value,
//! pattern, and (eventually) the pure-fn call stack so the boundary can
//! render a faithful `Failure::PureMatch`. `MalformedPattern` is the
//! distinct mode for an interpolated regex that fails to compile - it maps
//! to a runtime error, never folded into `PureMatchFailed`.

use relux_core::diagnostics::IrSpan;

/// Error raised while evaluating a pure expression.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PureEvalError {
    #[error("pure match failed: value did not satisfy pattern")]
    PureMatchFailed {
        value: String,
        pattern: String,
        is_regex: bool,
        span: IrSpan,
        /// Innermost-first; reversed for outermost-first display. Empty
        /// until a later milestone populates pure-fn frames.
        call_stack: Vec<PureFrame>,
    },
    #[error("pure match pattern is malformed: {reason}")]
    MalformedPattern {
        pattern: String,
        reason: String,
        span: IrSpan,
    },
}

/// A single pure-fn frame on the pure-evaluation call stack.
#[derive(Debug, Clone)]
pub struct PureFrame {
    pub name: String,
    pub call_site: IrSpan,
}
