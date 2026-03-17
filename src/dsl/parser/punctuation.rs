use chumsky::prelude::*;

use crate::dsl::lexer::Token;

use super::ParserInput;

/// `->` (Dash Gt)
pub fn punctuation_arrow<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Dash)
        .then(just(Token::Gt))
        .ignored()
        .labelled("arrow (->)")
}

/// `{`
pub fn punctuation_brace_open<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::BraceOpen)
        .ignored()
        .labelled("opening brace ({)")
}

/// `}`
pub fn punctuation_brace_close<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::BraceClose)
        .ignored()
        .labelled("closing brace (})")
}

/// `(`
pub fn punctuation_paren_open<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::ParenOpen)
        .ignored()
        .labelled("opening paren (()")
}

/// `)`
pub fn punctuation_paren_close<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::ParenClose)
        .ignored()
        .labelled("closing paren ())")
}

/// `,`
pub fn punctuation_comma<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Comma).ignored().labelled("comma (,)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    #[test]
    fn arrow() {
        let source = "->";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_arrow().parse(input).into_result().is_ok());
    }

    #[test]
    fn braces() {
        let source = "{";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_brace_open().parse(input).into_result().is_ok());

        let source = "}";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_brace_close().parse(input).into_result().is_ok());
    }

    #[test]
    fn parens() {
        let source = "(";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_paren_open().parse(input).into_result().is_ok());

        let source = ")";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_paren_close().parse(input).into_result().is_ok());
    }

    #[test]
    fn comma() {
        let source = ",";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(punctuation_comma().parse(input).into_result().is_ok());
    }
}
