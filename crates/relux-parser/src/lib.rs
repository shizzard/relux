mod annotation;
mod block;
pub mod builder;
mod effect;
pub mod entry;
pub mod error;
mod expr;
mod fn_def;
pub mod grammar;
mod ident;
mod import;
mod interpolation;
mod module;
mod need;
mod operator;
mod overlay;
pub mod parser;
mod prefix;
mod punctuation;
mod stmt;
pub mod syntax_kind;
mod test_def;
mod timeout;
mod token;
mod ws;

pub use error::ParseError;
use error::SyntaxError;

use chumsky::error::RichPattern;
use chumsky::error::RichReason;
use chumsky::input::Input as _;
use chumsky::input::MappedInput;
use chumsky::prelude::*;

use relux_lexer::Token;

pub use syntax_kind::ReluxLanguage;
pub use syntax_kind::SyntaxElement;
pub use syntax_kind::SyntaxKind;
pub use syntax_kind::SyntaxNode;
pub use syntax_kind::SyntaxToken;

pub type Span = relux_core::Span;

// --- Parser Input Type -----------------------------------

/// The Chumsky input type for the parser: a slice of `(Token, SimpleSpan)` pairs
/// mapped so that chumsky tracks byte-offset spans from the source.
pub type ParserInput<'a> = MappedInput<'a, Token<'a>, SimpleSpan, &'a [(Token<'a>, SimpleSpan)]>;

/// Convert lexer output to the `(Token, SimpleSpan)` pairs that chumsky needs.
pub(crate) fn lex_to_pairs(source: &str) -> Vec<(Token<'_>, SimpleSpan)> {
    relux_lexer::lex(source)
        .into_iter()
        .map(|s| {
            let span = SimpleSpan::from(s.span.start()..s.span.end());
            (s.node, span)
        })
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

// --- Span Conversion Helpers -----------------------------

/// Convert a chumsky SimpleSpan to a relux_core::Span.
pub(crate) fn span_from_chumsky(s: chumsky::span::SimpleSpan) -> relux_core::Span {
    relux_core::Span::new(s.start, s.end)
}

/// Create a Spanned from a chumsky SimpleSpan.
pub(crate) fn spanned_from_chumsky<T>(
    node: T,
    s: chumsky::span::SimpleSpan,
) -> relux_core::Spanned<T> {
    relux_core::Spanned::new(node, span_from_chumsky(s))
}

// --- Error Formatting ------------------------------------

/// Format a Rich error, filtering out `SomethingElse` from the expected list.
fn format_rich_error(e: &Rich<'_, Token<'_>>) -> String {
    match e.reason() {
        RichReason::ExpectedFound { expected, found } => {
            let expected: Vec<_> = expected
                .iter()
                .filter(|p| !matches!(p, RichPattern::SomethingElse))
                .collect();

            let found_str = match found {
                Some(tok) => format!("found '{}'", **tok),
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

// --- Public API ------------------------------------------

/// The chumsky front end. Retained as the differential oracle for Story 0
/// (T05) and deleted in T27. Public unconditionally: under `cst-frontend`
/// nothing else calls `module::module()`, and a private version would make
/// every combinator below it dead code.
pub fn parse_chumsky(source: &str) -> Result<relux_ast::AstModule, ParseError> {
    let pairs = lex_to_pairs(source);
    let input = make_input(&pairs, source.len());
    module::module()
        .then_ignore(end())
        .parse(input)
        .into_result()
        .map_err(|errs| {
            // No recovery combinators are used, so chumsky always produces exactly one error.
            assert_eq!(
                errs.len(),
                1,
                "expected exactly one parse error without recovery"
            );
            let err = errs.into_iter().next().unwrap();
            let message = format_rich_error(&err);
            let span = *err.span();
            ParseError::Syntax {
                error: SyntaxError::custom(span, message),
            }
        })
}

/// Parse a Relux module.
///
/// Routes through the hand-written CST front end under the `cst-frontend`
/// feature and through chumsky otherwise. The signature stays stable for the
/// whole of Story 0 so the existing tests serve as a conformance harness
/// against either front end; T23 is what changes it to return `Parse`.
pub fn parse(source: &str) -> Result<relux_ast::AstModule, ParseError> {
    dispatch(source)
}

// Two gated free functions with one call site, rather than a `#[cfg]` block
// inside one body. Not for the reason usually given: `#[cfg]` strips before
// type checking, so an inactive arm may reference things that do not exist.
// The real reasons are that an attributed block does not work as a tail
// expression, and that `cfg!()` -- the macro it is easy to reach for instead --
// keeps both arms in the program and does require both to typecheck.
#[cfg(not(feature = "cst-frontend"))]
fn dispatch(source: &str) -> Result<relux_ast::AstModule, ParseError> {
    parse_chumsky(source)
}

#[cfg(feature = "cst-frontend")]
fn dispatch(source: &str) -> Result<relux_ast::AstModule, ParseError> {
    entry::module(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "fn dummy() {}\n";

    #[cfg(not(feature = "cst-frontend"))]
    #[test]
    fn parse_delegates_to_chumsky_when_the_feature_is_off() {
        assert_eq!(parse(SRC).unwrap(), parse_chumsky(SRC).unwrap());
    }

    #[cfg(feature = "cst-frontend")]
    #[test]
    fn parse_routes_through_the_cst_front_end_when_the_feature_is_on() {
        let m = parse(SRC).unwrap();
        assert!(
            m.items.is_empty(),
            "T04 lowers to an empty module; T06 is what fills it in"
        );
        assert_eq!(m.span, Span::new(0, SRC.len()));
    }

    #[cfg(feature = "cst-frontend")]
    #[test]
    fn the_chumsky_front_end_is_still_reachable_under_the_feature() {
        // T05's differential oracle calls this directly. Its absence would
        // also make twenty modules of combinators dead code, which fails
        // clippy only in the configuration a developer runs least often.
        assert!(parse_chumsky(SRC).is_ok());
    }
}
