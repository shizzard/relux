use chumsky::prelude::*;

use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::ident::ident_aliased_effect_shell;
use super::overlay::overlay;
use super::ws::newline;
use super::ws::ws;
use relux_ast::AstStartDecl;

// ─── L5: Start Combinator ──────────────────────────────────

/// `start Effect [as alias] [{ overlay }]` — start declaration.
pub fn start_decl<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStartDecl>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Start)
        .ignore_then(ws())
        .ignore_then(ident_aliased_effect_shell())
        .then(ws().ignore_then(overlay()).or_not())
        .map_with(|(aliased, overlay), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStartDecl {
                    effect: aliased.name,
                    alias: aliased.alias,
                    overlay: overlay.unwrap_or_default(),
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
        .labelled("start declaration")
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;

    fn parse_start(source: &str) -> AstStartDecl {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        start_decl().parse(input).into_result().unwrap().node
    }

    #[test]
    fn bare_start() {
        let n = parse_start("start Db\n");
        assert_eq!(n.effect.node.name, "Db");
        assert!(n.alias.is_none());
        assert!(n.overlay.is_empty());
    }

    #[test]
    fn start_with_alias() {
        let n = parse_start("start Db as db\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db");
        assert!(n.overlay.is_empty());
    }

    #[test]
    fn start_with_overlay() {
        let n = parse_start("start Db { PORT = \"5433\" }\n");
        assert_eq!(n.effect.node.name, "Db");
        assert!(n.alias.is_none());
        assert_eq!(n.overlay.len(), 1);
        assert_eq!(n.overlay[0].node.key.node.name, "PORT");
    }

    #[test]
    fn start_with_alias_and_overlay() {
        let n = parse_start("start Db as db { PORT = \"5433\" }\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db");
        assert_eq!(n.overlay.len(), 1);
    }

    #[test]
    fn start_with_trailing_comma_overlay() {
        let n = parse_start("start Db as db { PORT = \"5433\", HOST = \"localhost\", }\n");
        assert_eq!(n.overlay.len(), 2);
    }

    #[test]
    fn start_rejects_snake_case_effect() {
        let source = "start my_db\n";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(start_decl().parse(input).into_result().is_err());
    }

    #[test]
    fn start_with_multiline_overlay() {
        let n = parse_start(
            r#"start Db {
  PORT = "5433"
  HOST = "localhost"
}
"#,
        );
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.overlay.len(), 2);
    }

    #[test]
    fn start_alias_with_digit() {
        let n = parse_start("start Db as db2\n");
        assert_eq!(n.effect.node.name, "Db");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "db2");
    }

    #[test]
    fn start_with_shorthand_overlay() {
        let n = parse_start(
            r#"start Node as n1 {
  NODE_PORT
  NODE_NAME = "node1"
}
"#,
        );
        assert_eq!(n.effect.node.name, "Node");
        assert_eq!(n.alias.as_ref().unwrap().node.name, "n1");
        assert_eq!(n.overlay.len(), 2);
        // First entry is shorthand: NODE_PORT desugared to NODE_PORT = NODE_PORT
        assert_eq!(n.overlay[0].node.key.node.name, "NODE_PORT");
        assert!(matches!(
            n.overlay[0].node.value.node,
            relux_ast::AstExpr::Var { .. }
        ));
    }
}
