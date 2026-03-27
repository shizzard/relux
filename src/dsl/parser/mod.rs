mod annotation;
pub mod ast;
mod block;
mod effect;
pub mod error;
mod expr;
mod fn_def;
mod ident;
mod import;
mod interpolation;
mod module;
mod need;
mod operator;
mod overlay;
mod prefix;
mod punctuation;
mod stmt;
mod test_def;
mod timeout;
mod token;
mod ws;

pub use ast::*;
pub use error::ParseError;

use chumsky::error::RichPattern;
use chumsky::error::RichReason;
use chumsky::input::Input as _;
use chumsky::input::MappedInput;
use chumsky::prelude::*;

use crate::dsl::lexer::Token;

pub type Span = crate::Span;

// ─── Parser Input Type ──────────────────────────────────────

/// The Chumsky input type for the parser: a slice of `(Token, SimpleSpan)` pairs
/// mapped so that chumsky tracks byte-offset spans from the source.
pub type ParserInput<'a> = MappedInput<'a, Token<'a>, SimpleSpan, &'a [(Token<'a>, SimpleSpan)]>;

/// Convert lexer output to the `(Token, SimpleSpan)` pairs that chumsky needs.
pub(crate) fn lex_to_pairs(source: &str) -> Vec<(Token<'_>, SimpleSpan)> {
    crate::dsl::lexer::lex(source)
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Create the chumsky `MappedInput` from token pairs and source length.
pub(crate) fn make_input<'a>(
    tokens: &'a [(Token<'a>, SimpleSpan)],
    source_len: usize,
) -> ParserInput<'a> {
    let eoi = SimpleSpan::from(source_len..source_len);
    tokens.split_token_span(eoi)
}

// ─── Error Formatting ───────────────────────────────────────

/// Format a Rich error, filtering out `SomethingElse` from the expected list.
fn format_rich_error(e: &Rich<'_, Token<'_>>) -> String {
    match e.reason() {
        RichReason::ExpectedFound { expected, found } => {
            let expected: Vec<_> = expected
                .iter()
                .filter(|p| !matches!(p, RichPattern::SomethingElse))
                .collect();

            let found_str = match found {
                Some(tok) => format!("found '{}'", &**tok),
                None => "found end of input".to_string(),
            };

            if expected.is_empty() {
                format!("{found_str} expected something else")
            } else {
                let mut parts: Vec<String> = expected.iter().map(|p| format!("{p}")).collect();
                parts.sort();
                parts.dedup();
                let expected_str = match &parts[..] {
                    [one] => one.clone(),
                    _ => {
                        let last = parts.last().unwrap().clone();
                        let rest = &parts[..parts.len() - 1];
                        format!("{}, or {last}", rest.join(", "))
                    }
                };
                format!("{found_str} expected {expected_str}")
            }
        }
        RichReason::Custom(msg) => msg.clone(),
    }
}

// ─── Public API ─────────────────────────────────────────────

pub fn parse(source: &str) -> Result<AstModule, ParseError> {
    let pairs = lex_to_pairs(source);
    let input = make_input(&pairs, source.len());
    module::module()
        .then_ignore(end())
        .parse(input)
        .into_result()
        .map_err(|errs| {
            let msgs: Vec<String> = errs.iter().map(format_rich_error).collect();
            ParseError::Multiple(msgs.join("; "))
        })
}
