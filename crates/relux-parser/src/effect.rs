use chumsky::prelude::*;

use relux_core::Span;
use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::annotation::comment;
use super::annotation::marker;
use super::block::cleanup_block;
use super::block::qualified_shell_block;
use super::block::shell_block;
use super::ident::ident_effect;
use super::ident::ident_var;
use super::need::start_decl;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::stmt::stmt_let_standalone;
use super::token::keyword;
use super::ws::leading_ws;
use super::ws::newline;
use super::ws::ws;
use relux_ast::AstEffectDef;
use relux_ast::AstEffectItem;
use relux_ast::AstExpectDecl;
use relux_ast::AstExposeDecl;
use relux_ast::AstMarkerDecl;
use relux_ast::AstNode;
use relux_ast::AstStmt;

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

/// `[preamble] effect Name { expect, lets, starts, expose, shells, cleanup }` — effect definition.
pub fn def_effect<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstEffectDef>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    // Header: effect Name {
    let header = effect_preamble()
        .then_ignore(leading_ws())
        .then_ignore(keyword(Token::Effect))
        .then_ignore(ws())
        .then(ident_effect())
        .then_ignore(ws())
        .then_ignore(punctuation_brace_open());

    // ── Expect section ─────────────────────────────────────
    // `expect VAR1, VAR2, VAR3`
    let expect_item = leading_ws()
        .ignore_then(keyword(Token::Expect))
        .ignore_then(ws())
        .ignore_then(
            ident_var()
                .separated_by(
                    select_ref! {
                        Token::Space(_) => (),
                        Token::Tab(_) => (),
                        Token::Comma => (),
                    }
                    .repeated()
                    .at_least(1)
                    .ignored(),
                )
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then_ignore(newline())
        .map_with(|vars, e| {
            let span = crate::span_from_chumsky(e.span());
            AstEffectItem::Expect {
                decl: AstExpectDecl { vars, span },
                span,
            }
        });
    let expect_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let expect_section = choice((
        expect_item,
        expect_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    // ── Start section ──────────────────────────────────────
    let start_item = leading_ws().ignore_then(start_decl()).map_with(|n, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Start { decl: n.node, span }
    });
    let start_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let start_section = choice((
        start_item,
        start_comment,
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
            let span = crate::span_from_chumsky(e.span());
            match s.node {
                AstStmt::Let { stmt, .. } => AstEffectItem::Let { stmt, span },
                _ => unreachable!(),
            }
        });
    let let_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
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

    // ── Expose section ──────────────────────────────────────
    // `expose shell` or `expose qualifier.shell as alias`
    let expose_item = leading_ws()
        .ignore_then(keyword(Token::Expose))
        .ignore_then(ws())
        .ignore_then(ident_var())
        .then(just(Token::Dot).ignore_then(ident_var()).or_not())
        .then(
            ws().ignore_then(just(Token::As))
                .ignore_then(ws())
                .ignore_then(ident_var())
                .or_not(),
        )
        .then_ignore(newline())
        .map_with(|((first, dot_second), alias), e| {
            let span = crate::span_from_chumsky(e.span());
            let (qualifier, shell) = match dot_second {
                Some(second) => (Some(first), second),
                None => (None, first),
            };
            AstEffectItem::Expose {
                decl: AstExposeDecl {
                    qualifier,
                    shell,
                    alias,
                    span,
                },
                span,
            }
        });
    let expose_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let expose_section = choice((
        expose_item,
        expose_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstEffectItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    // ── Shell section (both `shell name { }` and `qualifier.name { }`) ──
    let shell_item = leading_ws().ignore_then(shell_block()).map_with(|sb, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Shell {
            block: sb.node,
            span,
        }
    });
    let qualified_shell_item =
        leading_ws()
            .ignore_then(qualified_shell_block())
            .map_with(|sb, e| {
                let span = crate::span_from_chumsky(e.span());
                AstEffectItem::Shell {
                    block: sb.node,
                    span,
                }
            });
    let shell_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Comment { text: c, span }
    });
    let shell_section = choice((
        shell_item,
        qualified_shell_item,
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
        let span = crate::span_from_chumsky(e.span());
        AstEffectItem::Cleanup {
            block: cb.node,
            span,
        }
    });
    let cleanup_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
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
        .then(expect_section)
        .then(let_section)
        .then(start_section)
        .then(expose_section)
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
            |(((((((markers, name), expects), lets), starts), exposes), shells), cleanup), e| {
                let outer_span = crate::span_from_chumsky(e.span());
                let mut body = Vec::new();
                for item in expects {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                for item in lets {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                for item in starts {
                    if !is_sentinel_comment(&item) {
                        let item_span = *item.span();
                        body.push(Spanned::new(item, item_span));
                    }
                }
                for item in exposes {
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
    use crate::lex_to_pairs;
    use crate::make_input;

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
            r#"effect Db {
  shell db {
    > echo start
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "Db");
        assert!(e.markers.is_empty());
    }

    #[test]
    fn effect_with_start() {
        let e = parse_effect(
            r#"effect App {
  start Db
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
                .any(|item| matches!(&item.node, AstEffectItem::Start { .. }))
        );
    }

    #[test]
    fn effect_with_marker() {
        let e = parse_effect(
            r#"# skip
effect Db {
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
            r#"effect Db {
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
            r#"effect Db {
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
            r#"effect App {
  let port = "8080"
  start Db
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
                .any(|item| matches!(&item.node, AstEffectItem::Start { .. }))
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
    fn effect_with_multiple_starts() {
        let e = parse_effect(
            r#"effect App {
  start Db
  start Cache
  shell app {
    > echo start
  }
}
"#,
        );
        let start_count = e
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstEffectItem::Start { .. }))
            .count();
        assert_eq!(start_count, 2);
    }

    #[test]
    fn effect_with_comments_in_body() {
        let e = parse_effect(
            r#"effect Db {
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
            r#"effect App {

  let port = "8080"

  start Db

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
                .any(|item| matches!(&item.node, AstEffectItem::Start { .. }))
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
            r#"effect App {
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
    fn effect_with_start_overlay() {
        let e = parse_effect(
            r#"effect App {
  start Db { PORT = "5433" }
  shell app {
    > echo start
  }
}
"#,
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Start { .. }))
        );
    }

    // ── Expect tests ────────────────────────────────────────

    #[test]
    fn effect_with_expect() {
        let e = parse_effect(
            r#"effect Db {
  expect DB_PORT
  shell db {
    > echo start
  }
}
"#,
        );
        let expect = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expect { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expect");
        assert_eq!(expect.vars.len(), 1);
        assert_eq!(expect.vars[0].node.name, "DB_PORT");
    }

    #[test]
    fn effect_with_expect_multiple_vars() {
        let e = parse_effect(
            r#"effect Db {
  expect DB_PORT, DB_HOST, DB_NAME
  shell db {
    > echo start
  }
}
"#,
        );
        let expect = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expect { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expect");
        assert_eq!(expect.vars.len(), 3);
        assert_eq!(expect.vars[0].node.name, "DB_PORT");
        assert_eq!(expect.vars[1].node.name, "DB_HOST");
        assert_eq!(expect.vars[2].node.name, "DB_NAME");
    }

    // ── Expose tests ────────────────────────────────────────

    #[test]
    fn effect_with_expose_simple() {
        let e = parse_effect(
            r#"effect Db {
  expose db
  shell db {
    > echo start
  }
}
"#,
        );
        let expose = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expose { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expose");
        assert!(expose.qualifier.is_none());
        assert_eq!(expose.shell.node.name, "db");
        assert!(expose.alias.is_none());
    }

    #[test]
    fn effect_with_expose_qualified() {
        let e = parse_effect(
            r#"effect Cluster {
  start Node as n1
  expose n1.node as primary
  shell setup {
    > echo setup
  }
}
"#,
        );
        let expose = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expose { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expose");
        assert_eq!(expose.qualifier.as_ref().unwrap().node.name, "n1");
        assert_eq!(expose.shell.node.name, "node");
        assert_eq!(expose.alias.as_ref().unwrap().node.name, "primary");
    }

    #[test]
    fn effect_with_multiple_exposes() {
        let e = parse_effect(
            r#"effect Cluster {
  start Node as n1
  start Node as n2
  expose n1.node as primary
  expose n2.node as secondary
  shell setup {
    > echo setup
  }
}
"#,
        );
        let expose_count = e
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstEffectItem::Expose { .. }))
            .count();
        assert_eq!(expose_count, 2);
    }

    // ── Full R008 effect ────────────────────────────────────

    #[test]
    fn effect_full_r008() {
        let e = parse_effect(
            r#"effect Node {
  expect NODE_PORT, NODE_NAME
  let data_dir = "${__RELUX_TEST_ARTIFACTS}/node"
  start DependencyEffect as dep
  expose node
  shell node {
    > start-node --port ${NODE_PORT}
  }
  cleanup {
    > stop-node
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "Node");
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Expect { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Let { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Start { .. }))
        );
        assert!(
            e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Expose { .. }))
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
    fn effect_with_expose_aliased_local() {
        let e = parse_effect(
            r#"effect Auth {
  expose auth as svc
  shell auth {
    > echo start
  }
}
"#,
        );
        let expose = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expose { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expose");
        assert!(expose.qualifier.is_none());
        assert_eq!(expose.shell.node.name, "auth");
        assert_eq!(expose.alias.as_ref().unwrap().node.name, "svc");
    }

    #[test]
    fn effect_no_expose_is_valid() {
        let e = parse_effect(
            r#"effect SideEffect {
  shell setup {
    > echo side effect
  }
}
"#,
        );
        assert_eq!(e.name.node.name, "SideEffect");
        assert!(
            !e.body
                .iter()
                .any(|item| matches!(&item.node, AstEffectItem::Expose { .. }))
        );
    }

    #[test]
    fn effect_expose_qualified_no_alias() {
        let e = parse_effect(
            r#"effect Wrapper {
  start Base as b
  expose b.shell_name
  shell local {
    > echo setup
  }
}
"#,
        );
        let expose = e
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstEffectItem::Expose { decl, .. } => Some(decl),
                _ => None,
            })
            .expect("should have expose");
        assert_eq!(expose.qualifier.as_ref().unwrap().node.name, "b");
        assert_eq!(expose.shell.node.name, "shell_name");
        assert!(expose.alias.is_none());
    }
}
