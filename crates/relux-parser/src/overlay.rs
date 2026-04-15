use chumsky::prelude::*;

use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::expr::expr;
use super::ident::ident_var;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::ws::flex_ws;
use super::ws::ws;
use relux_ast::AstExpr;
use relux_ast::AstOverlayEntry;

// ─── L4: Overlay Combinators ────────────────────────────────

/// `var = expr` — single overlay entry, or bare `var` (shorthand for `var = var`).
fn overlay_entry<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstOverlayEntry>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    // Full form: `KEY = expr`
    let full = ident_var()
        .then_ignore(ws().then(just(Token::Eq)).then(ws()))
        .then(expr())
        .map_with(|(key, value), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(AstOverlayEntry { key, value, span }, span)
        });

    // Shorthand: bare `KEY` desugars to `KEY = KEY`
    let shorthand = ident_var().map_with(|key, e| {
        let span = crate::span_from_chumsky(e.span());
        let var_expr = AstExpr::Var {
            name: key.node.name.clone(),
            span: key.span,
        };
        let value = Spanned::new(var_expr, key.span);
        Spanned::new(AstOverlayEntry { key, value, span }, span)
    });

    choice((full, shorthand)).labelled("overlay entry")
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
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;
    use relux_ast::AstExpr;

    fn parse_overlay(source: &str) -> Vec<Spanned<AstOverlayEntry>> {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        overlay().parse(input).into_result().unwrap()
    }

    #[test]
    fn single_entry() {
        let entries = parse_overlay(r#"{ PORT = "5433" }"#);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node.name, "PORT");
    }

    #[test]
    fn multiple_entries() {
        let entries = parse_overlay(r#"{ PORT = "5433", HOST = "localhost" }"#);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].node.key.node.name, "PORT");
        assert_eq!(entries[1].node.key.node.name, "HOST");
    }

    #[test]
    fn trailing_comma() {
        let entries = parse_overlay(r#"{ PORT = "5433", }"#);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node.name, "PORT");
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
        assert_eq!(entries[0].node.key.node.name, "PORT");
        assert_eq!(entries[1].node.key.node.name, "HOST");
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
        assert_eq!(entries[0].node.key.node.name, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::Var { .. }));
    }

    #[test]
    fn overlay_with_call_value() {
        let entries = parse_overlay("{ PORT = get_port() }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node.name, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::Call { .. }));
    }

    #[test]
    fn overlay_with_numeric_value() {
        let entries = parse_overlay("{ PORT = 5432 }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node.name, "PORT");
        assert!(matches!(entries[0].node.value.node, AstExpr::String { .. }));
    }

    #[test]
    fn overlay_shorthand() {
        let entries = parse_overlay("{ PORT }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node.key.node.name, "PORT");
        // Shorthand desugars to Var with same name
        match &entries[0].node.value.node {
            AstExpr::Var { name, .. } => assert_eq!(name, "PORT"),
            other => panic!("expected Var, got {other:?}"),
        }
    }

    #[test]
    fn overlay_shorthand_mixed() {
        let entries = parse_overlay(
            r#"{
  NODE_PORT
  NODE_NAME = "node1"
}"#,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].node.key.node.name, "NODE_PORT");
        match &entries[0].node.value.node {
            AstExpr::Var { name, .. } => assert_eq!(name, "NODE_PORT"),
            other => panic!("expected Var, got {other:?}"),
        }
        assert_eq!(entries[1].node.key.node.name, "NODE_NAME");
        match &entries[1].node.value.node {
            AstExpr::String { .. } => {}
            other => panic!("expected String, got {other:?}"),
        }
    }
}
