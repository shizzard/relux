use chumsky::prelude::*;

use relux_core::Span;
use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::annotation::comment;
use super::annotation::docstring;
use super::annotation::marker;
use super::block::cleanup_block;
use super::block::qualified_shell_block;
use super::block::shell_block;
use super::expr::plain_string;
use super::need::start_decl;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::stmt::stmt_let_standalone;
use super::timeout::timeout;
use super::token::keyword;
use super::ws::leading_ws;
use super::ws::newline;
use super::ws::ws;
use relux_ast::AstMarkerDecl;
use relux_ast::AstNode;
use relux_ast::AstStmt;
use relux_ast::AstTestDef;
use relux_ast::AstTestItem;

/// Sentinel span for dummy blank-line comments.
const SENTINEL: Span = Span::new(0, 0);

// ─── Helpers ────────────────────────────────────────────────

/// Preamble: markers/comments/blank lines before the `test` keyword.
fn test_preamble<'a>()
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

// ─── L6: Test Definition ───────────────────────────────────

/// `[preamble] test "name" [timeout] { docstring, lets, starts, shells, cleanup }` — test definition.
pub fn def_test<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstTestDef>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    // Header: test "name" [~5s] {
    let header = test_preamble()
        .then_ignore(leading_ws())
        .then_ignore(keyword(Token::Test))
        .then_ignore(ws())
        .then(plain_string())
        .then(ws().ignore_then(timeout()).or_not())
        .then_ignore(ws())
        .then_ignore(punctuation_brace_open());

    // Docstring section (optional, at most one)
    let doc_item = leading_ws().ignore_then(docstring()).map_with(|d, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::DocString { text: d.node, span }
    });
    let doc_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Comment { text: c, span }
    });
    let doc_section = choice((
        doc_item.map(Some),
        doc_comment.map(Some),
        newline().to(None),
    ))
    .repeated()
    .collect::<Vec<_>>()
    .map(|items| items.into_iter().flatten().collect::<Vec<_>>());

    // Start section
    let start_item = leading_ws().ignore_then(start_decl()).map_with(|n, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Start { decl: n.node, span }
    });
    let start_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Comment { text: c, span }
    });
    let start_section = choice((
        start_item,
        start_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstTestItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    // Let section
    let let_item = leading_ws()
        .ignore_then(stmt_let_standalone())
        .map_with(|s, e| {
            let span = crate::span_from_chumsky(e.span());
            match s.node {
                AstStmt::Let { stmt, .. } => AstTestItem::Let { stmt, span },
                _ => unreachable!(),
            }
        });
    let let_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Comment { text: c, span }
    });
    let let_section = choice((
        let_item,
        let_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstTestItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    // Shell section (both `shell name { }` and `qualifier.name { }`)
    let shell_item = leading_ws().ignore_then(shell_block()).map_with(|sb, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Shell {
            block: sb.node,
            span,
        }
    });
    let qualified_shell_item =
        leading_ws()
            .ignore_then(qualified_shell_block())
            .map_with(|sb, e| {
                let span = crate::span_from_chumsky(e.span());
                AstTestItem::Shell {
                    block: sb.node,
                    span,
                }
            });
    let shell_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Comment { text: c, span }
    });
    let shell_section = choice((
        shell_item,
        qualified_shell_item,
        shell_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstTestItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .repeated()
    .collect::<Vec<_>>();

    // Cleanup section (optional)
    let cleanup_item = leading_ws().ignore_then(cleanup_block()).map_with(|cb, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Cleanup {
            block: cb.node,
            span,
        }
    });
    let cleanup_comment = leading_ws().ignore_then(comment()).map_with(|c, e| {
        let span = crate::span_from_chumsky(e.span());
        AstTestItem::Comment { text: c, span }
    });
    let cleanup_section = choice((
        cleanup_item,
        cleanup_comment,
        // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
        newline().to(AstTestItem::Comment {
            text: String::new(),
            span: SENTINEL,
        }),
    ))
    .or_not()
    // Fragile: SENTINEL comment must be filtered by is_sentinel_comment — edit with caution.
    .map(|opt| {
        opt.unwrap_or(AstTestItem::Comment {
            text: String::new(),
            span: SENTINEL,
        })
    });

    let stage1 = header
        .then(doc_section)
        .then(let_section)
        .map(|((((markers, name), timeout_opt), docs), lets)| {
            (markers, name, timeout_opt, docs, lets)
        })
        .boxed();

    stage1
        .then(start_section)
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
            |((((markers, name, timeout_opt, docs, lets), starts), shells), cleanup), e| {
                let outer_span = crate::span_from_chumsky(e.span());

                let timeout = timeout_opt;

                let mut body = Vec::new();
                for item in docs {
                    let item_span = *item.span();
                    body.push(Spanned::new(item, item_span));
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
                    AstTestDef {
                        name,
                        timeout,
                        markers,
                        body,
                        span: outer_span,
                    },
                    outer_span,
                )
            },
        )
        .labelled("test definition")
}

fn is_sentinel_comment(item: &AstTestItem) -> bool {
    matches!(item, AstTestItem::Comment { text, span } if text.is_empty() && *span == SENTINEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::lex_to_pairs;
    use crate::make_input;
    use relux_ast::AstTimeout;

    fn parse_test(source: &str) -> AstTestDef {
        try_parse_test(source).expect("parse failed")
    }

    fn try_parse_test(source: &str) -> Option<AstTestDef> {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        def_test()
            .then_ignore(any().repeated())
            .parse(input)
            .into_result()
            .ok()
            .map(|s| s.node)
    }

    #[test]
    fn empty_test_body_parses() {
        let t = try_parse_test("test \"t\" {}\n");
        assert!(t.is_some(), "empty test body must parse");
        assert!(t.unwrap().body.is_empty());
    }

    #[test]
    fn comment_only_test_body_parses() {
        let t = try_parse_test("test \"t\" {\n  // just a comment\n}\n");
        assert!(t.is_some(), "comment-only test body must parse");
    }

    #[test]
    fn minimal_test() {
        let t = parse_test(
            r#"test "my test" {
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.name.node, "my test");
        assert!(t.timeout.is_none());
        assert!(t.markers.is_empty());
    }

    #[test]
    fn test_with_timeout() {
        let t = parse_test(
            r#"test "my test" ~5s {
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.name.node, "my test");
        let timeout = t.timeout.unwrap();
        assert!(matches!(timeout.node, AstTimeout::Tolerance { .. }));
        assert_eq!(timeout.node.duration(), Duration::from_secs(5));
    }

    #[test]
    fn test_with_marker() {
        let t = parse_test(
            r#"# skip
test "my test" {
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.markers.len(), 1);
    }

    #[test]
    fn test_with_docstring() {
        let t = parse_test(
            r#"test "my test" {
  """this is a docstring"""
  shell main {
    > echo hello
  }
}
"#,
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::DocString { .. }))
        );
    }

    #[test]
    fn test_with_start() {
        let t = parse_test(
            r#"test "my test" {
  start Db
  shell main {
    > echo hello
  }
}
"#,
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Start { .. }))
        );
    }

    #[test]
    fn test_with_cleanup() {
        let t = parse_test(
            r#"test "my test" {
  shell main {
    > echo hello
  }
  cleanup {
    > echo bye
  }
}
"#,
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Cleanup { .. }))
        );
    }

    #[test]
    fn test_with_let() {
        let t = parse_test(
            r#"test "my test" {
  let x = "hello"
  shell main {
    > echo hello
  }
}
"#,
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Let { .. }))
        );
    }

    #[test]
    fn test_with_multiple_shells() {
        let t = parse_test(
            r#"test "my test" {
  shell main {
    > echo hello
  }
  shell aux {
    > echo world
  }
}
"#,
        );
        let shell_count = t
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstTestItem::Shell { .. }))
            .count();
        assert_eq!(shell_count, 2);
    }

    #[test]
    fn test_with_assertion_timeout() {
        let t = parse_test(
            r#"test "my test" @5s {
  shell main {
    > echo hello
  }
}
"#,
        );
        let timeout = t.timeout.unwrap();
        assert!(matches!(timeout.node, AstTimeout::Assertion { .. }));
        assert_eq!(timeout.node.duration(), Duration::from_secs(5));
    }

    #[test]
    fn test_all_sections() {
        let t = parse_test(
            r#"# skip
test "full test" ~10s {
  """docstring here"""
  let port = "5432"
  start Db
  shell main {
    > echo hello
  }
  cleanup {
    > echo bye
  }
}
"#,
        );
        assert_eq!(t.name.node, "full test");
        assert_eq!(t.markers.len(), 1);
        assert!(t.timeout.is_some());
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::DocString { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Start { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Let { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Shell { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Cleanup { .. }))
        );
    }

    #[test]
    fn test_with_multiple_markers() {
        let t = parse_test(
            r#"# skip
# flaky
test "my test" {
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.markers.len(), 2);
    }

    #[test]
    fn test_with_comments_between_sections() {
        let t = parse_test(
            r#"test "my test" {
  // let section
  let x = "val"
  // need section
  start Db
  // shell section
  shell main {
    > echo hello
  }
}
"#,
        );
        let comment_count = t
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstTestItem::Comment { .. }))
            .count();
        assert!(comment_count >= 3);
    }

    #[test]
    fn test_start_with_alias() {
        let t = parse_test(
            r#"test "my test" {
  start Db as db
  shell main {
    > echo hello
  }
}
"#,
        );
        let start = t
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstTestItem::Start { decl, .. } => Some(decl),
                _ => None,
            })
            .unwrap();
        assert_eq!(start.effect.node.name, "Db");
        assert_eq!(start.alias.as_ref().unwrap().node.name, "db");
    }

    #[test]
    fn test_start_with_overlay() {
        let t = parse_test(
            r#"test "my test" {
  start Db { PORT = "5433" }
  shell main {
    > echo hello
  }
}
"#,
        );
        let start = t
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstTestItem::Start { decl, .. } => Some(decl),
                _ => None,
            })
            .unwrap();
        assert_eq!(start.effect.node.name, "Db");
        assert_eq!(start.overlay.len(), 1);
    }

    #[test]
    fn test_blank_lines_between_sections() {
        let t = parse_test(
            r#"test "my test" {

  let x = "val"

  start Db

  shell main {
    > echo hello
  }

}
"#,
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Start { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Let { .. }))
        );
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::Shell { .. }))
        );
    }

    #[test]
    fn test_with_special_char_name() {
        let t = parse_test(
            r#"test "hello@world: (test #1)" {
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.name.node, "hello@world: (test #1)");
    }

    #[test]
    fn test_with_docstring_and_timeout() {
        let t = parse_test(
            r#"# skip
test "full" ~5s {
  """docstring"""
  shell main {
    > echo hello
  }
}
"#,
        );
        assert_eq!(t.markers.len(), 1);
        assert!(t.timeout.is_some());
        assert!(
            t.body
                .iter()
                .any(|item| matches!(&item.node, AstTestItem::DocString { .. }))
        );
    }

    #[test]
    fn test_with_multiple_starts() {
        let t = parse_test(
            r#"test "my test" {
  start Db
  start Cache
  shell main {
    > echo hello
  }
}
"#,
        );
        let need_count = t
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstTestItem::Start { .. }))
            .count();
        assert_eq!(need_count, 2);
    }

    #[test]
    fn test_with_multiple_lets() {
        let t = parse_test(
            r#"test "my test" {
  let x = "a"
  let y = "b"
  shell main {
    > echo hello
  }
}
"#,
        );
        let let_count = t
            .body
            .iter()
            .filter(|item| matches!(&item.node, AstTestItem::Let { .. }))
            .count();
        assert_eq!(let_count, 2);
    }

    #[test]
    fn test_with_qualified_shell_block() {
        let t = parse_test(
            r#"test "my test" {
  start Db as db
  shell db.main {
    > echo hello
  }
}
"#,
        );
        let shell = t
            .body
            .iter()
            .find_map(|item| match &item.node {
                AstTestItem::Shell { block, .. } => Some(block),
                _ => None,
            })
            .unwrap();
        assert_eq!(shell.qualifier.as_ref().unwrap().node.name, "db");
        assert_eq!(shell.name.node.name, "main");
    }

    #[test]
    fn test_with_mixed_shell_blocks() {
        let t = parse_test(
            r#"test "my test" {
  start Db as db
  shell local {
    > echo local
  }
  shell db.main {
    > echo remote
  }
}
"#,
        );
        let shells: Vec<_> = t
            .body
            .iter()
            .filter_map(|item| match &item.node {
                AstTestItem::Shell { block, .. } => Some(block),
                _ => None,
            })
            .collect();
        assert_eq!(shells.len(), 2);
        assert!(shells[0].qualifier.is_none());
        assert_eq!(shells[0].name.node.name, "local");
        assert!(shells[1].qualifier.is_some());
        assert_eq!(shells[1].qualifier.as_ref().unwrap().node.name, "db");
        assert_eq!(shells[1].name.node.name, "main");
    }
}
