use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::ParserInput;
use super::ast::AstOverlayEntry;
use super::expr::expr;
use super::ident::ident_var;
use super::punctuation::{punctuation_brace_close, punctuation_brace_open};
use super::ws::{flex_ws, ws};

// ─── L4: Overlay Combinators ────────────────────────────────

/// `var = expr` — single overlay entry.
fn overlay_entry<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstOverlayEntry>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    ident_var()
        .then_ignore(ws().then(just(Token::Eq)).then(ws()))
        .then(expr())
        .map_with(|(key, value), e| {
            let span = Span::from(e.span());
            Spanned::new(AstOverlayEntry { key, value, span }, span)
        })
        .labelled("overlay entry (key = value)")
}

/// `{ key = val, key = val }` — overlay block with optional trailing comma.
pub fn overlay<'a>()
-> impl Parser<'a, ParserInput<'a>, Vec<Spanned<AstOverlayEntry>>, extra::Err<Rich<'a, Token<'a>>>>
+ Clone {
    let sep = select_ref! {
        Token::Space(_) => (),
        Token::Tab(_) => (),
        Token::Newline => (),
        Token::Comma => (),
    }
    .repeated()
    .at_least(1)
    .ignored();

    punctuation_brace_open()
        .ignore_then(flex_ws())
        .ignore_then(
            overlay_entry()
                .separated_by(sep)
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(flex_ws())
        .then_ignore(punctuation_brace_close())
        .labelled("overlay block")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::ast::AstExpr;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    fn parse_overlay(source: &str) -> Vec<Spanned<AstOverlayEntry>> {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        overlay().parse(input).into_result().unwrap()
    }

    #[test]
    fn single_entry() {
        let entries = parse_overlay(r#"{ PORT = "5433" }"#);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node, "PORT");
    }

    #[test]
    fn multiple_entries() {
        let entries = parse_overlay(r#"{ PORT = "5433", HOST = "localhost" }"#);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].node.key.node, "PORT");
        assert_eq!(entries[1].node.key.node, "HOST");
    }

    #[test]
    fn trailing_comma() {
        let entries = parse_overlay(r#"{ PORT = "5433", }"#);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node, "PORT");
    }

    #[test]
    fn multiline_overlay() {
        let entries = parse_overlay(
            r#"{
  PORT = "5433"
  HOST = "localhost"
}"#,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].node.key.node, "PORT");
        assert_eq!(entries[1].node.key.node, "HOST");
    }

    #[test]
    fn empty_overlay() {
        let entries = parse_overlay("{}");
        assert!(entries.is_empty());
    }

    #[test]
    fn overlay_with_var_value() {
        let entries = parse_overlay("{ PORT = port_var }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::Var { .. }));
    }

    #[test]
    fn overlay_with_call_value() {
        let entries = parse_overlay("{ PORT = get_port() }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::Call { .. }));
    }

    #[test]
    fn overlay_with_numeric_value() {
        let entries = parse_overlay("{ PORT = 5432 }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::String { .. }));
    }
}
