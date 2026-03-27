use chumsky::prelude::*;

use crate::dsl::lexer::Token;

use super::ParserInput;

/// Zero or more `Space`/`Tab` tokens, returns `()`.
pub fn ws<'a>() -> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    select_ref! {
        Token::Space(_) => (),
        Token::Tab(_) => (),
    }
    .repeated()
    .collect::<Vec<()>>()
    .ignored()
}

/// Same as `ws()`, semantic alias for start-of-line indentation.
pub fn leading_ws<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ws()
}

/// Matches a `Newline` token, returns `()`.
pub fn newline<'a>() -> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Newline).ignored().labelled("newline")
}

/// Zero or more `Space`/`Tab`/`Newline` tokens, returns `()`.
/// Use inside blocks where newlines are non-significant (e.g. overlays).
pub fn flex_ws<'a>() -> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    select_ref! {
        Token::Space(_) => (),
        Token::Tab(_) => (),
        Token::Newline => (),
    }
    .repeated()
    .collect::<Vec<()>>()
    .ignored()
}

/// Matches three consecutive `Quote` tokens (docstring delimiter `"""`).
pub fn docstring_delim<'a>()
-> impl Parser<'a, ParserInput<'a>, (), extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Quote)
        .then(just(Token::Quote))
        .then(just(Token::Quote))
        .ignored()
        .labelled("docstring delimiter (\"\"\")")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    #[test]
    fn ws_matches_spaces() {
        let source = "   ";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn ws_matches_tabs() {
        let source = "\t\t";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn ws_matches_mixed() {
        let source = " \t ";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn ws_matches_empty() {
        let source = "";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn newline_matches() {
        let source = "\n";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(newline().parse(input).into_result().is_ok());
    }

    #[test]
    fn docstring_delim_matches_triple_quote() {
        let source = r#"""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(docstring_delim().parse(input).into_result().is_ok());
    }

    #[test]
    fn flex_ws_matches_spaces_tabs_newlines() {
        let source = " \t\n \t";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(flex_ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn flex_ws_matches_empty() {
        let source = "";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(flex_ws().parse(input).into_result().is_ok());
    }

    #[test]
    fn newline_rejects_space() {
        let source = " ";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(newline().parse(input).into_result().is_err());
    }

    #[test]
    fn docstring_delim_rejects_two_quotes() {
        let source = r#""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(docstring_delim().parse(input).into_result().is_err());
    }
}
