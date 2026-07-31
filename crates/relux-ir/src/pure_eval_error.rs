//! The error raised by pure evaluation.
//!
//! In this milestone the enum is uninhabited: pure evaluation cannot
//! fail, so `Result<String, PureEvalError>` is always `Ok`, and the
//! empty enum lets the compiler prove it. A later milestone adds the
//! `PureMatchFailed` variant (and its call-stack frames) when a pure
//! match inside a `pure fn` body becomes the first construct that can
//! fail during pure evaluation.

/// Error raised while evaluating a pure expression.
///
/// Currently uninhabited - see the module docs.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PureEvalError {}
