//! The error raised by pure evaluation.
//!
//! Pure evaluation became fallible the moment a pure match could appear
//! inside a `pure fn` body: a no-match must stop evaluation and travel out
//! to the boundary as a test failure. `PureMatchFailed` carries the value
//! and pattern so the boundary can render a faithful `Failure::PureMatch`.
//! The pure-fn call chain is not carried on the error: the runtime already
//! opens a `FnCall` span per pure-fn call (via the `PureEvalSink`), so the
//! boundary reconstructs the chain by resolving the still-open span tree
//! from the innermost pure-fn span. `MalformedPattern` is the distinct
//! mode for an interpolated regex that fails to compile - it maps to a
//! runtime error, never folded into `PureMatchFailed`.

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
    },
    #[error("pure match pattern is malformed: {reason}")]
    MalformedPattern {
        pattern: String,
        reason: String,
        span: IrSpan,
    },
}
