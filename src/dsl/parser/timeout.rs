use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::AstTimeoutKind;
use super::ParserInput;
use super::token::text;

/// Parser-internal: parsed timeout with kind and duration string.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTimeout {
    pub kind: AstTimeoutKind,
    pub duration: String,
}

/// `~duration` — tolerance timeout. Validates span contiguity.
pub fn timeout_tolerance<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<ParsedTimeout>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Tilde)
        .map_with(|_, e| e.span())
        .then(text())
        .try_map(
            |(tilde_span, (dur, dur_span)): (SimpleSpan, (&str, SimpleSpan)), _extra| {
                if tilde_span.end != dur_span.start {
                    return Err(Rich::custom(
                        dur_span,
                        "no whitespace allowed between `~` and duration",
                    ));
                }
                let full_span = Span::new(tilde_span.start, dur_span.end);
                Ok(Spanned::new(
                    ParsedTimeout {
                        kind: AstTimeoutKind::Tolerance { span: full_span },
                        duration: dur.to_string(),
                    },
                    full_span,
                ))
            },
        )
        .labelled("tolerance timeout (~Ns)")
}

/// `@duration` — assertion timeout. Validates span contiguity.
pub fn timeout_assert<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<ParsedTimeout>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::At)
        .map_with(|_, e| e.span())
        .then(text())
        .try_map(
            |(at_span, (dur, dur_span)): (SimpleSpan, (&str, SimpleSpan)), _extra| {
                if at_span.end != dur_span.start {
                    return Err(Rich::custom(
                        dur_span,
                        "no whitespace allowed between `@` and duration",
                    ));
                }
                let full_span = Span::new(at_span.start, dur_span.end);
                Ok(Spanned::new(
                    ParsedTimeout {
                        kind: AstTimeoutKind::Assertion { span: full_span },
                        duration: dur.to_string(),
                    },
                    full_span,
                ))
            },
        )
        .labelled("assertion timeout (@Ns)")
}

/// Tolerance or assertion timeout.
pub fn timeout<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<ParsedTimeout>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    choice((timeout_tolerance(), timeout_assert())).labelled("timeout (~Ns or @Ns)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    #[test]
    fn tolerance_timeout() {
        let source = "~5s";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = timeout().parse(input).into_result();
        assert!(result.is_ok());
        let t = result.unwrap();
        assert!(matches!(t.node.kind, AstTimeoutKind::Tolerance { .. }));
        assert_eq!(t.node.duration, "5s");
    }

    #[test]
    fn assertion_timeout() {
        let source = "@10s";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = timeout().parse(input).into_result();
        assert!(result.is_ok());
        let t = result.unwrap();
        assert!(matches!(t.node.kind, AstTimeoutKind::Assertion { .. }));
        assert_eq!(t.node.duration, "10s");
    }

    #[test]
    fn long_duration() {
        let source = "~2h30m12s";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = timeout().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node.duration, "2h30m12s");
    }

    #[test]
    fn rejects_whitespace_between_prefix_and_duration() {
        let source = "~ 5s";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(timeout().parse(input).into_result().is_err());
    }

    #[test]
    fn rejects_whitespace_between_at_and_duration() {
        let source = "@ 5s";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(timeout().parse(input).into_result().is_err());
    }

    #[test]
    fn millisecond_duration() {
        let source = "~500ms";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = timeout().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node.duration, "500ms");
    }

    #[test]
    fn minute_duration() {
        let source = "@2m";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = timeout().parse(input).into_result();
        assert!(result.is_ok());
        let t = result.unwrap();
        assert!(matches!(t.node.kind, AstTimeoutKind::Assertion { .. }));
        assert_eq!(t.node.duration, "2m");
    }
}
