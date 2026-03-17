use chumsky::prelude::*;

use crate::dsl::lexer::Token;

use super::ParserInput;

/// Matches a `Text` or `Word` token. Returns `(source_slice, byte_span)`.
pub fn text<'a>()
-> impl Parser<'a, ParserInput<'a>, (&'a str, SimpleSpan), extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    select_ref! {
        Token::Text(s) => *s,
        Token::Word(s) => *s,
    }
    .map_with(|s, e| (s, e.span()))
    .labelled("text")
}

/// Matches a specific keyword token. Returns the byte span.
pub fn keyword<'a>(
    k: Token<'a>,
) -> impl Parser<'a, ParserInput<'a>, SimpleSpan, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(k).map_with(|_, e| e.span())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    #[test]
    fn text_matches_word() {
        let source = "hello";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = text().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "hello");
    }

    #[test]
    fn keyword_matches_fn() {
        let source = "fn";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = keyword(Token::Fn).parse(input).into_result();
        assert!(result.is_ok());
    }

    #[test]
    fn keyword_rejects_wrong_keyword() {
        let source = "let";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(keyword(Token::Fn).parse(input).into_result().is_err());
    }

    #[test]
    fn text_rejects_punctuation() {
        let source = ">";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(text().parse(input).into_result().is_err());
    }
}
