use chumsky::prelude::SimpleSpan;
use thiserror::Error;

pub type SyntaxError = chumsky::error::Rich<'static, String, SimpleSpan>;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{error}")]
    Syntax { error: SyntaxError },
    #[error("unknown escape sequence `{sequence}`")]
    InvalidEscape { sequence: String, span: SimpleSpan },
    #[error("orphan marker not attached to any test or effect")]
    OrphanMarker { span: SimpleSpan },
}

impl ParseError {
    pub fn span(&self) -> &SimpleSpan {
        match self {
            ParseError::Syntax { error } => error.span(),
            ParseError::InvalidEscape { span, .. } => span,
            ParseError::OrphanMarker { span } => span,
        }
    }
}
