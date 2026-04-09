use chumsky::prelude::SimpleSpan;
use thiserror::Error;

pub type SyntaxError = chumsky::error::Rich<'static, String, SimpleSpan>;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{0}")]
    Syntax(SyntaxError),
    #[error("unknown escape sequence `{sequence}`")]
    InvalidEscape { sequence: String, span: SimpleSpan },
    #[error("orphan marker not attached to any test or effect")]
    OrphanMarker { span: SimpleSpan },
    #[error("{0}")]
    Multiple(String),
}

impl ParseError {
    pub fn span(&self) -> &SimpleSpan {
        match self {
            ParseError::Syntax(e) => e.span(),
            ParseError::InvalidEscape { span, .. } => span,
            ParseError::OrphanMarker { span } => span,
            ParseError::Multiple(_) => {
                static ZERO: std::sync::LazyLock<SimpleSpan> =
                    std::sync::LazyLock::new(|| SimpleSpan::from(0..0));
                &ZERO
            }
        }
    }
}
