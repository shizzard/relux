use chumsky::prelude::*;

use relux_lexer::Token;

use super::ParserInput;

/// `//` — comment prefix (two consecutive Slash tokens).
pub fn prefix_comment<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Slash)
        .then(just(Token::Slash))
        .ignored()
        .labelled("comment prefix (//)")
}

/// `#` — marker prefix.
pub fn prefix_marker<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Hash).ignored().labelled("marker prefix (#)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;

    #[test]
    fn comment_prefix() {
        let source = "//";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(prefix_comment().parse(input).into_result().is_ok());
    }

    #[test]
    fn single_slash_is_not_comment() {
        let source = "/";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(prefix_comment().parse(input).into_result().is_err());
    }

    #[test]
    fn marker_prefix() {
        let source = "#";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(prefix_marker().parse(input).into_result().is_ok());
    }
}
