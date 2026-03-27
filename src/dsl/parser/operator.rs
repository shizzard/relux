use chumsky::prelude::*;

use crate::Span;
use crate::Spanned;
use crate::dsl::lexer::Token;

use super::ParserInput;
use super::ast::AstTimeout;
use super::timeout::timeout;

// ─── L2: Untimed Operators ──────────────────────────────────

/// `>` — send
pub fn op_send<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Gt)
        .map_with(|_, e| e.span())
        .labelled("send operator (>)")
}

/// `=>` — send raw
pub fn op_send_raw<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Eq)
        .map_with(|_, e| e.span())
        .then(just(Token::Gt).map_with(|_, e| e.span()))
        .map(|(a, b): (SimpleSpan, SimpleSpan)| SimpleSpan::from(a.start..b.end))
        .labelled("send raw operator (=>)")
}

/// `<?` — match regex
pub fn op_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Lt)
        .map_with(|_, e| e.span())
        .then(just(Token::Question).map_with(|_, e| e.span()))
        .map(|(a, b): (SimpleSpan, SimpleSpan)| SimpleSpan::from(a.start..b.end))
        .labelled("match regex operator (<?)")
}

/// `<=` — match literal
pub fn op_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Lt)
        .map_with(|_, e| e.span())
        .then(just(Token::Eq).map_with(|_, e| e.span()))
        .map(|(a, b): (SimpleSpan, SimpleSpan)| SimpleSpan::from(a.start..b.end))
        .labelled("match literal operator (<=)")
}

/// `!?` — fail regex
pub fn op_fail_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Bang)
        .map_with(|_, e| e.span())
        .then(just(Token::Question).map_with(|_, e| e.span()))
        .map(|(a, b): (SimpleSpan, SimpleSpan)| SimpleSpan::from(a.start..b.end))
        .labelled("fail regex operator (!?)")
}

/// `!=` — fail literal
pub fn op_fail_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Bang)
        .map_with(|_, e| e.span())
        .then(just(Token::Eq).map_with(|_, e| e.span()))
        .map(|(a, b): (SimpleSpan, SimpleSpan)| SimpleSpan::from(a.start..b.end))
        .labelled("fail literal operator (!=)")
}

// ─── L3: Timed Operators ────────────────────────────────────

/// `<~5s=` or `<@2s=` — timed match literal
pub fn op_timed_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstTimeout>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Lt)
        .map_with(|_, e| e.span())
        .then(timeout())
        .then(just(Token::Eq).map_with(|_, e| e.span()))
        .map(|((lt_span, t), eq_span)| {
            let full_span = Span::new(lt_span.start, eq_span.end);
            Spanned::new(t.node, full_span)
        })
        .labelled("timed match literal operator (<~Ns= or <@Ns=)")
}

/// `<~5s?` or `<@2s?` — timed match regex
pub fn op_timed_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstTimeout>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Lt)
        .map_with(|_, e| e.span())
        .then(timeout())
        .then(just(Token::Question).map_with(|_, e| e.span()))
        .map(|((lt_span, t), q_span)| {
            let full_span = Span::new(lt_span.start, q_span.end);
            Spanned::new(t.node, full_span)
        })
        .labelled("timed match regex operator (<~Ns? or <@Ns?)")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    #[test]
    fn send_operator() {
        let source = ">";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_send().parse(input).into_result().is_ok());
    }

    #[test]
    fn send_raw_operator() {
        let source = "=>";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_send_raw().parse(input).into_result().is_ok());
    }

    #[test]
    fn match_regex_operator() {
        let source = "<?";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_match_regex().parse(input).into_result().is_ok());
    }

    #[test]
    fn match_literal_operator() {
        let source = "<=";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_match_literal().parse(input).into_result().is_ok());
    }

    #[test]
    fn fail_regex_operator() {
        let source = "!?";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_fail_regex().parse(input).into_result().is_ok());
    }

    #[test]
    fn fail_literal_operator() {
        let source = "!=";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_fail_literal().parse(input).into_result().is_ok());
    }

    #[test]
    fn timed_match_regex() {
        let source = "<~5s?";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = op_timed_match_regex().parse(input).into_result();
        assert!(result.is_ok());
        let t = result.unwrap();
        assert!(matches!(t.node, AstTimeout::Tolerance { .. }));
        assert_eq!(t.node.duration(), Duration::from_secs(5));
        assert_eq!(t.span, Span::new(0, 5));
    }

    #[test]
    fn send_raw_rejects_single_eq() {
        let source = "=";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_send_raw().parse(input).into_result().is_err());
    }

    #[test]
    fn match_regex_rejects_single_lt() {
        let source = "<";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_match_regex().parse(input).into_result().is_err());
    }

    #[test]
    fn fail_regex_rejects_single_bang() {
        let source = "!";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(op_fail_regex().parse(input).into_result().is_err());
    }

    #[test]
    fn timed_match_literal() {
        let source = "<@2s=";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = op_timed_match_literal().parse(input).into_result();
        assert!(result.is_ok());
        let t = result.unwrap();
        assert!(matches!(t.node, AstTimeout::Assertion { .. }));
        assert_eq!(t.node.duration(), Duration::from_secs(2));
        assert_eq!(t.span, Span::new(0, 5));
    }
}
