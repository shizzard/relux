use chumsky::prelude::*;

use crate::Span;
use crate::Spanned;
use crate::dsl::lexer::Token;

use super::ParserInput;
use super::ast::AstCleanupBlock;
use super::ast::AstShellBlock;
use super::ast::AstStmt;
use super::ident::ident_var;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::stmt::stmt;
use super::ws::leading_ws;
use super::ws::newline;
use super::ws::ws;

/// Sentinel span for dummy blank-line comments (filtered out after collection).
const SENTINEL: Span = Span::new(0, 0);

// ─── L5: Block Combinators ─────────────────────────────────

/// `shell name { stmts }` — shell block.
pub fn shell_block<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstShellBlock>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Shell)
        .ignore_then(ws())
        .ignore_then(ident_var())
        .then_ignore(ws())
        .then_ignore(punctuation_brace_open())
        .then(
            stmt()
                // Fragile: SENTINEL comment must be filtered below — edit with caution.
                .or(newline().map_with(|_, _| {
                    Spanned::new(
                        AstStmt::Comment { text: String::new(), span: SENTINEL },
                        SENTINEL,
                    )
                }))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(leading_ws())
        .then_ignore(punctuation_brace_close())
        .map_with(|(name, stmts), e| {
            let stmts = stmts
                .into_iter()
                .filter(|s| {
                    !matches!(&s.node, AstStmt::Comment { text, .. } if text.is_empty() && s.span == SENTINEL)
                })
                .collect();
            let span = Span::from(e.span());
            Spanned::new(AstShellBlock { qualifier: None, name, stmts, span }, span)
        })
        .labelled("shell block")
}

/// `shell qualifier.name { stmts }` — qualified shell block (dot-access to effect-exported shell).
pub fn qualified_shell_block<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstShellBlock>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Shell)
        .ignore_then(ws())
        .ignore_then(ident_var())
        .then_ignore(just(Token::Dot))
        .then(ident_var())
        .then_ignore(ws())
        .then_ignore(punctuation_brace_open())
        .then(
            stmt()
                // Fragile: SENTINEL comment must be filtered below — edit with caution.
                .or(newline().map_with(|_, _| {
                    Spanned::new(
                        AstStmt::Comment { text: String::new(), span: SENTINEL },
                        SENTINEL,
                    )
                }))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(leading_ws())
        .then_ignore(punctuation_brace_close())
        .map_with(|((qualifier, name), stmts), e| {
            let stmts = stmts
                .into_iter()
                .filter(|s| {
                    !matches!(&s.node, AstStmt::Comment { text, .. } if text.is_empty() && s.span == SENTINEL)
                })
                .collect();
            let span = Span::from(e.span());
            Spanned::new(AstShellBlock { qualifier: Some(qualifier), name, stmts, span }, span)
        })
        .labelled("qualified shell block")
}

/// `cleanup { stmts }` — cleanup block.
pub fn cleanup_block<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstCleanupBlock>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    just(Token::Cleanup)
        .ignore_then(ws())
        .ignore_then(punctuation_brace_open())
        .ignore_then(
            stmt()
                // Fragile: SENTINEL comment must be filtered below — edit with caution.
                .or(newline().map_with(|_, _| {
                    Spanned::new(
                        AstStmt::Comment { text: String::new(), span: SENTINEL },
                        SENTINEL,
                    )
                }))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(leading_ws())
        .then_ignore(punctuation_brace_close())
        .map_with(|stmts, e| {
            let stmts = stmts
                .into_iter()
                .filter(|s| {
                    !matches!(&s.node, AstStmt::Comment { text, .. } if text.is_empty() && s.span == SENTINEL)
                })
                .collect();
            let span = Span::from(e.span());
            Spanned::new(AstCleanupBlock { stmts, span }, span)
        })
        .labelled("cleanup block")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    fn parse_shell(source: &str) -> AstShellBlock {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        shell_block().parse(input).into_result().unwrap().node
    }

    fn parse_cleanup(source: &str) -> AstCleanupBlock {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        cleanup_block().parse(input).into_result().unwrap().node
    }

    #[test]
    fn empty_shell_block() {
        let sb = parse_shell(
            r#"shell main {
}"#,
        );
        assert_eq!(sb.name.node.name, "main");
        assert!(sb.stmts.is_empty());
    }

    #[test]
    fn shell_block_with_stmts() {
        let sb = parse_shell(
            r#"shell main {
  > echo hello
  <= hello
}"#,
        );
        assert_eq!(sb.name.node.name, "main");
        assert_eq!(sb.stmts.len(), 2);
        assert!(matches!(&sb.stmts[0].node, AstStmt::Send { .. }));
        assert!(matches!(&sb.stmts[1].node, AstStmt::MatchLiteral { .. }));
    }

    #[test]
    fn shell_block_with_blank_lines() {
        let sb = parse_shell(
            r#"shell main {
  > echo hello

  <= hello
}"#,
        );
        assert_eq!(sb.stmts.len(), 2);
    }

    #[test]
    fn empty_cleanup_block() {
        let cb = parse_cleanup(
            r#"cleanup {
}"#,
        );
        assert!(cb.stmts.is_empty());
    }

    #[test]
    fn cleanup_block_with_stmts() {
        let cb = parse_cleanup(
            r#"cleanup {
  > exit
}"#,
        );
        assert_eq!(cb.stmts.len(), 1);
        assert!(matches!(&cb.stmts[0].node, AstStmt::Send { .. }));
    }

    #[test]
    fn cleanup_block_with_blank_lines() {
        let cb = parse_cleanup(
            r#"cleanup {
  > exit

  > done
}"#,
        );
        assert_eq!(cb.stmts.len(), 2);
    }

    #[test]
    fn shell_block_with_comments() {
        let sb = parse_shell(
            r#"shell main {
  // a comment
  > echo hello
}"#,
        );
        assert_eq!(sb.stmts.len(), 2);
        assert!(matches!(&sb.stmts[0].node, AstStmt::Comment { .. }));
        assert!(matches!(&sb.stmts[1].node, AstStmt::Send { .. }));
    }

    #[test]
    fn cleanup_block_with_comments() {
        let cb = parse_cleanup(
            r#"cleanup {
  // cleanup comment
  > exit
}"#,
        );
        assert_eq!(cb.stmts.len(), 2);
        assert!(matches!(&cb.stmts[0].node, AstStmt::Comment { .. }));
        assert!(matches!(&cb.stmts[1].node, AstStmt::Send { .. }));
    }

    #[test]
    fn shell_block_diverse_stmts() {
        let sb = parse_shell(
            r#"shell main {
  let x = "hello"
  > echo ${x}
  <= hello
  <? \d+
}"#,
        );
        assert_eq!(sb.stmts.len(), 4);
        assert!(matches!(&sb.stmts[0].node, AstStmt::Let { .. }));
        assert!(matches!(&sb.stmts[1].node, AstStmt::Send { .. }));
        assert!(matches!(&sb.stmts[2].node, AstStmt::MatchLiteral { .. }));
        assert!(matches!(&sb.stmts[3].node, AstStmt::MatchRegex { .. }));
    }

    #[test]
    fn shell_block_leading_trailing_blank_lines() {
        let sb = parse_shell(
            r#"shell main {

  > echo hello

}"#,
        );
        assert_eq!(sb.stmts.len(), 1);
        assert!(matches!(&sb.stmts[0].node, AstStmt::Send { .. }));
    }

    #[test]
    fn shell_block_name_with_digits() {
        let sb = parse_shell(
            r#"shell main_2 {
  > echo hello
}"#,
        );
        assert_eq!(sb.name.node.name, "main_2");
        assert_eq!(sb.stmts.len(), 1);
    }

    #[test]
    fn cleanup_only_comments() {
        let cb = parse_cleanup(
            r#"cleanup {
  // first comment
  // second comment
}"#,
        );
        assert_eq!(cb.stmts.len(), 2);
        assert!(
            cb.stmts
                .iter()
                .all(|s| matches!(&s.node, AstStmt::Comment { .. }))
        );
    }

    // ── Qualified shell blocks ──────────────────────────────

    fn parse_qualified(source: &str) -> AstShellBlock {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        qualified_shell_block()
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    #[test]
    fn qualified_shell_block_basic() {
        let sb = parse_qualified(
            r#"shell n.node {
  > echo hello
}"#,
        );
        assert_eq!(sb.qualifier.as_ref().unwrap().node.name, "n");
        assert_eq!(sb.name.node.name, "node");
        assert_eq!(sb.stmts.len(), 1);
    }

    #[test]
    fn qualified_shell_block_empty() {
        let sb = parse_qualified(
            r#"shell db.main {
}"#,
        );
        assert_eq!(sb.qualifier.as_ref().unwrap().node.name, "db");
        assert_eq!(sb.name.node.name, "main");
        assert!(sb.stmts.is_empty());
    }
}
