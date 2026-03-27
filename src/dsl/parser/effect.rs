use chumsky::prelude::*;

use crate::Span;
use crate::Spanned;
use crate::dsl::lexer::Token;

use super::ParserInput;
use super::annotation::comment;
use super::annotation::marker;
use super::ast::AstEffectDef;
use super::ast::AstEffectItem;
use super::ast::AstMarkerDecl;
use super::ast::AstNode;
use super::ast::AstStmt;
use super::block::cleanup_block;
use super::block::shell_block;
use super::ident::ident_effect;
use super::ident::ident_var;
use super::need::need;
use super::punctuation::punctuation_arrow;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::stmt::stmt_let_standalone;
use super::token::keyword;
use super::ws::leading_ws;
use super::ws::newline;
use super::ws::ws;

/// Sentinel span for dummy blank-line comments.
const SENTINEL: Span = Span::new(0, 0);

// ─── Helpers ────────────────────────────────────────────────

/// Preamble: markers/comments/blank lines before the `effect` keyword.
fn effect_preamble<'a>()
-> impl Parser<'a, ParserInput<'a>, Vec<Spanned<AstMarkerDecl>>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    let marker_item = leading_ws().ignore_then(marker());
    let comment_item = leading_ws().ignore_then(comment()).to(());
    let blank = newline().to(());

    choice((marker_item.map(Some), comment_item.to(None), blank.to(None)))
        .repeated()
        .collect::<Vec<_>>()
        .map(|items| items.into_iter().flatten().collect())
}

// ─── L6: Effect Definition ─────────────────────────────────

/// `[preamble] effect Name -> shell { lets, needs, shells, cleanup }` — effect definition.
pub fn def_effect<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstEffectDef>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    // Header: effect Name -> shell_name {
    let header = effect_preamble()
        .then_ignore(leading_ws())
        .then_ignore(keyword(Token::Effect))
        .then_ignore(ws())
        .then(ident_effect())
        .then_ignore(ws())
        .then_ignore(punctuation_arrow())
        .then_ignore(ws())
        .then(ident_var())
        .then_ignore(ws())
        .then_ignore(punctuation_brace_open());

    // Body sections (order-enforced): needs, lets, shells, cleanup
    let need_item = leading_ws().ignore_then(need()).map_with(|n, e| {
        let span = Span::from(e.span());
        AstEffectItem::Need { decl: n.node, span }
    });
    let need_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = Span::from(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let need_section = choice((
        need_item,
        need_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    let let_item = leading_ws()
        .ignore_then(stmt_let_standalone())
        .map_with(|s, e| {
            let span = Span::from(e.span());
            match s.node {
                AstStmt::Let { stmt, .. } => AstEffectItem::Let { stmt, span },
                _ => unreachable!(),
            }
        });
    let let_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = Span::from(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let let_section = choice((
        let_item,
        let_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    let shell_item = leading_ws().ignore_then(shell_block()).map_with(|sb, e| {
        let span = Span::from(e.span());
        AstEffectItem::Shell {
            block: sb.node,
            span,
        }
    });
    let shell_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = Span::from(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let shell_section = choice((
        shell_item,
        shell_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    let cleanup_item = leading_ws().ignore_then(cleanup_block()).map_with(|cb, e| {
        let span = Span::from(e.span());
        AstEffectItem::Cleanup {
            block: cb.node,
            span,
        }
    });
    let cleanup_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = Span::from(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let cleanup_section = choice((
        cleanup_item,
        cleanup_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .or_not()
    // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
    .map(|opt| {
        opt.unwrap_or(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        })
    });

    header
        .then(let_section)
        .then(need_section)
        .then(shell_section)
        .then(cleanup_section)
        .then_ignore(
            select_ref! {
                Token::Newline => (),
                Token::Space(_) => (),
                Token::Tab(_) => (),
            }
            .repeated(),
        )
        .then_ignore(punctuation_brace_close())
        .map_with(
            |((((((markers, name), exported_shell), lets), needs), shells), cleanup), e| {
                let outer_span = Span::from(e.span());
                let mut body = Vec::new();
                for item in lets {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                for item in needs {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                for item in shells {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                if !is_sentinel_comment(&cleanup) {
                    let item_span = *cleanup.span();
                    body.push(Spanned::new(cleanup, item_span));
                }
                Spanned::new(
                    AstEffectDef {
                        name,
                        exported_shell,
                        markers,
                        body,
                        span: outer_span,
                    },
                    outer_span,
                )
            },
        )
        .labelled("effect definition")
}

fn is_sentinel_comment(item: &AstEffectItem) -> bool {
    matches!(item, AstEffectItem::Comment { text, span } if text.is_empty() && *span == SENTINEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    fn parse_effect(source: &str) -> AstEffectDef {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        def_effect()
            .then_ignore(any().repeated())
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    #[test]
    fn minimal_effect() {
        let e = parse_effect(
            r#"effect Db -> db {
  shell db {
    > echo start
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "Db");
        assert_eq!(e.exported_shell.node.name, "db");
        assert!(e.markers.is_empty());
    }

    #[test]
    fn effect_with_need() {
        let e = parse_effect(
            r#"effect App -> app {
  need Db
  shell app {
    > echo start
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "App");
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Need { .. }))
        );
    }

    #[test]
    fn effect_with_marker() {
        let e = parse_effect(
            r#"# skip
effect Db -> db {
  shell db {
    > echo start
  }
}
"#,
        );
        assert_eq!(e.markers.len(), 1);
    }

    #[test]
    fn effect_with_let() {
        let e = parse_effect(
            r#"effect Db -> db {
  let port = "5432"
  shell db {
    > echo start
  }
}
"#,
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Let { .. }))
        );
    }

    #[test]
    fn effect_with_cleanup() {
        let e = parse_effect(
            r#"effect Db -> db {
  shell db {
    > echo start
  }
  cleanup {
    > echo stop
  }
}
"#,
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Cleanup { .. }))
        );
    }

    #[test]
    fn effect_all_sections() {
        let e = parse_effect(
            r#"effect App -> app {
  let port = "8080"
  need Db
  shell app {
    > echo start
  }
  cleanup {
    > echo stop
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "App");
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Need { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Let { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Shell { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Cleanup { .. }))
        );
    }

    #[test]
    fn effect_with_multiple_needs() {
        let e = parse_effect(
            r#"effect App -> app {
  need Db
  need Cache
  shell app {
    > echo start
  }
}
"#,
        );
        let need_count = e
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstEffectItem::Need { .. }))
            .count();
        assert_eq!(need_count, 2);
    }

    #[test]
    fn effect_with_comments_in_body() {
        let e = parse_effect(
            r#"effect Db -> db {
  // setup comment
  shell db {
    > echo start
  }
  // cleanup comment
  cleanup {
    > echo stop
  }
}
"#,
        );
        let comment_count = e
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstEffectItem::Comment { .. }))
            .count();
        assert!(comment_count >= 2);
    }

    #[test]
    fn effect_blank_lines_between_sections() {
        let e = parse_effect(
            r#"effect App -> app {

  let port = "8080"

  need Db

  shell app {
    > echo start
  }

  cleanup {
    > echo stop
  }

}
"#,
        );
        assert_eq!(e.name.node.name, "App");
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Need { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Let { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Shell { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Cleanup { .. }))
        );
    }

    #[test]
    fn effect_with_multiple_shells() {
        let e = parse_effect(
            r#"effect App -> app {
  shell app {
    > echo start1
  }
  shell bg {
    > echo start2
  }
}
"#,
        );
        let shell_count = e
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstEffectItem::Shell { .. }))
            .count();
        assert_eq!(shell_count, 2);
    }

    #[test]
    fn effect_with_need_overlay() {
        let e = parse_effect(
            r#"effect App -> app {
  need Db { PORT = "5433" }
  shell app {
    > echo start
  }
}
"#,
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Need { .. }))
        );
    }
}
