use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::ParserInput;
use super::ast::AstNeedDecl;
use super::ident::ident_aliased_effect_shell;
use super::overlay::overlay;
use super::ws::{newline, ws};

// ─── L5: Need Combinator ───────────────────────────────────

/// `need Effect [as shell] [{ overlay }]` — need declaration.
pub fn need<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstNeedDecl>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Need)
        .ignore_then(ws())
        .ignore_then(ident_aliased_effect_shell())
        .then(ws().ignore_then(overlay()).or_not())
        .map_with(|(aliased, overlay), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstNeedDecl {
                    effect: aliased.name,
                    alias: aliased.alias,
                    overlay: overlay.unwrap_or_default(),
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
        .labelled("need declaration")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    fn parse_need(source: &str) -> AstNeedDecl {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        need().parse(input).into_result().unwrap().node
    }

    #[test]
    fn bare_need() {
        let n = parse_need("need Db\n");
        assert_eq!(n.effect.node.name, "Db");
        assert!(n.alias.is_none());
        assert!(n.overlay.is_empty());
    }

    #[test]
    fn need_with_alias() {
        let n = parse_need("need Db as db\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db");
        assert!(n.overlay.is_empty());
    }

    #[test]
    fn need_with_overlay() {
        let n = parse_need("need Db { PORT = \"5433\" }\n");
        assert_eq!(n.effect.node.name, "Db");
        assert!(n.alias.is_none());
        assert_eq!(n.overlay.len(), 1);
        assert_eq!(n.overlay[0].node.key.node.name, "PORT");
    }

    #[test]
    fn need_with_alias_and_overlay() {
        let n = parse_need("need Db as db { PORT = \"5433\" }\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db");
        assert_eq!(n.overlay.len(), 1);
    }

    #[test]
    fn need_with_trailing_comma_overlay() {
        let n = parse_need("need Db as db { PORT = \"5433\", HOST = \"localhost\", }\n");
        assert_eq!(n.overlay.len(), 2);
    }

    #[test]
    fn need_rejects_snake_case_effect() {
        let source = "need my_db\n";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(need().parse(input).into_result().is_err());
    }

    #[test]
    fn need_with_multiline_overlay() {
        let n = parse_need(
            r#"need Db {
  PORT = "5433"
  HOST = "localhost"
}
"#,
        );
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.overlay.len(), 2);
    }

    #[test]
    fn need_alias_with_digit() {
        let n = parse_need("need Db as db2\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db2");
    }
}
