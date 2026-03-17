use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::ParserInput;
use super::annotation::comment;
use super::ast::{AstAssignStmt, AstInterpolation, AstLetStmt, AstStmt, AstStringPart};
use super::expr::expr;
use super::ident::ident_var;
use super::interpolation::{interp_literal, interp_regex};
use super::operator::{
    op_fail_literal, op_fail_regex, op_match_literal, op_match_regex, op_send, op_send_raw,
    op_timed_match_literal, op_timed_match_regex,
};
use super::timeout::timeout;
use super::token::keyword;
use super::ws::{leading_ws, newline, ws};

// ─── L4: Statement Combinators ──────────────────────────────

/// `> payload` → `AstStmt::Send`
fn stmt_send<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_send()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::Send {
                    payload: payload.node,
                    span,
                },
                span,
            )
        })
}

/// `=> payload` → `AstStmt::SendRaw`
fn stmt_send_raw<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_send_raw()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::SendRaw {
                    payload: payload.node,
                    span,
                },
                span,
            )
        })
}

/// `<? payload` → `AstStmt::MatchRegex`, or `<?` alone → `AstStmt::BufferReset`
fn stmt_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_match_regex()
        .ignore_then(ws())
        .ignore_then(interp_regex(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            let stmt = if is_empty_payload(&payload.node) {
                AstStmt::BufferReset { span }
            } else {
                AstStmt::MatchRegex {
                    pattern: payload.node,
                    span,
                }
            };
            Spanned::new(stmt, span)
        })
}

/// `<= payload` → `AstStmt::MatchLiteral`, or `<=` alone → `AstStmt::BufferReset`
fn stmt_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_match_literal()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            let stmt = if is_empty_payload(&payload.node) {
                AstStmt::BufferReset { span }
            } else {
                AstStmt::MatchLiteral {
                    pattern: payload.node,
                    span,
                }
            };
            Spanned::new(stmt, span)
        })
}

/// `!? payload` → `AstStmt::FailRegex`, or `!?` alone → `AstStmt::ClearFailPattern`
fn stmt_fail_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_fail_regex()
        .ignore_then(ws())
        .ignore_then(interp_regex(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            let stmt = if is_empty_payload(&payload.node) {
                AstStmt::ClearFailPattern { span }
            } else {
                AstStmt::FailRegex {
                    pattern: payload.node,
                    span,
                }
            };
            Spanned::new(stmt, span)
        })
}

/// `!= payload` → `AstStmt::FailLiteral`, or `!=` alone → `AstStmt::ClearFailPattern`
fn stmt_fail_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_fail_literal()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .then_ignore(newline())
        .map_with(|payload, e| {
            let span = Span::from(e.span());
            let stmt = if is_empty_payload(&payload.node) {
                AstStmt::ClearFailPattern { span }
            } else {
                AstStmt::FailLiteral {
                    pattern: payload.node,
                    span,
                }
            };
            Spanned::new(stmt, span)
        })
}

/// `<~5s= payload` or `<@2s= payload` → `AstStmt::TimedMatchLiteral`
fn stmt_timed_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_timed_match_literal()
        .then_ignore(ws())
        .then(interp_literal(Token::Newline))
        .then_ignore(newline())
        .map_with(|(t, payload), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::TimedMatchLiteral {
                    timeout_kind: t.node.kind,
                    duration: t.node.duration,
                    pattern: payload,
                    span,
                },
                span,
            )
        })
}

/// `<~5s? payload` or `<@2s? payload` → `AstStmt::TimedMatchRegex`
fn stmt_timed_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_timed_match_regex()
        .then_ignore(ws())
        .then(interp_regex(Token::Newline))
        .then_ignore(newline())
        .map_with(|(t, payload), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::TimedMatchRegex {
                    timeout_kind: t.node.kind,
                    duration: t.node.duration,
                    pattern: payload,
                    span,
                },
                span,
            )
        })
}

/// `~5s` or `@10s` followed by newline → `AstStmt::Timeout`
fn stmt_timeout<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    timeout().then_ignore(newline()).map_with(|t, e| {
        let span = Span::from(e.span());
        Spanned::new(
            AstStmt::Timeout {
                kind: t.node.kind,
                duration: t.node.duration,
                span,
            },
            span,
        )
    })
}

/// `let name [= expr]` → `AstStmt::Let`
fn stmt_let<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    keyword(Token::Let)
        .ignore_then(ws())
        .ignore_then(ident_var())
        .then(
            ws().ignore_then(just(Token::Eq))
                .ignore_then(ws())
                .ignore_then(expr())
                .or_not(),
        )
        .then_ignore(newline())
        .map_with(|(name, value), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::Let {
                    stmt: AstLetStmt { name, value, span },
                    span,
                },
                span,
            )
        })
}

/// `name = expr` → `AstStmt::Assign`
fn stmt_assign<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ident_var()
        .then_ignore(ws().then(just(Token::Eq)).then(ws()))
        .then(expr())
        .then_ignore(newline())
        .map_with(|(name, value), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstStmt::Assign {
                    stmt: AstAssignStmt { name, value, span },
                    span,
                },
                span,
            )
        })
}

/// `expr` → `AstStmt::Expr` (catch-all for bare function calls)
fn stmt_expr<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    expr().then_ignore(newline()).map_with(|e, extra| {
        let span = Span::from(extra.span());
        Spanned::new(AstStmt::Expr { expr: e.node, span }, span)
    })
}

/// Full statement combinator: `leading_ws()` then ordered choice.
pub fn stmt<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    let stmt_comment = comment().map_with(|s, e| {
        let span = Span::from(e.span());
        Spanned::new(AstStmt::Comment { text: s, span }, span)
    });

    leading_ws().ignore_then(
        choice((
            stmt_comment,
            stmt_timed_match_literal(),
            stmt_timed_match_regex(),
            stmt_match_regex(),
            stmt_match_literal(),
            stmt_send_raw(),
            stmt_send(),
            stmt_fail_regex(),
            stmt_fail_literal(),
            stmt_timeout(),
            stmt_let(),
            stmt_assign(),
            stmt_expr(),
        ))
        .labelled("statement"),
    )
}

/// `stmt_let()` exported for use in effect/test body sections.
pub fn stmt_let_standalone<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    stmt_let()
}

// ─── Helpers ────────────────────────────────────────────────

fn is_empty_payload(interp: &AstInterpolation) -> bool {
    interp.parts.is_empty()
        || interp
            .parts
            .iter()
            .all(|p| matches!(p, AstStringPart::Literal { value, .. } if value.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::ast::{AstExpr, AstStmt, AstTimeoutKind};
    use crate::dsl::parser::{lex_to_pairs, make_input};

    fn parse_stmt(source: &str) -> AstStmt {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        stmt().parse(input).into_result().unwrap().node
    }

    #[test]
    fn send_statement() {
        let s = parse_stmt("> echo hello\n");
        match s {
            AstStmt::Send { payload, .. } => {
                assert_eq!(payload.parts.len(), 1);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::Literal { value, .. } if value == "echo hello")
                );
            }
            _ => panic!("expected Send, got {s:?}"),
        }
    }

    #[test]
    fn send_raw_statement() {
        let s = parse_stmt("=> raw data\n");
        match s {
            AstStmt::SendRaw { payload, .. } => {
                assert_eq!(payload.parts.len(), 1);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::Literal { value, .. } if value == "raw data")
                );
            }
            _ => panic!("expected SendRaw, got {s:?}"),
        }
    }

    #[test]
    fn match_regex_statement() {
        let s = parse_stmt("<? \\d+\n");
        match s {
            AstStmt::MatchRegex { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::Literal { value, .. } if value == r"\d+")
                );
            }
            _ => panic!("expected MatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn match_literal_statement() {
        let s = parse_stmt("<= hello world\n");
        match s {
            AstStmt::MatchLiteral { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::Literal { value, .. } if value == "hello world")
                );
            }
            _ => panic!("expected MatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn fail_regex_statement() {
        let s = parse_stmt("!? error.*\n");
        match s {
            AstStmt::FailRegex { pattern, .. } => {
                assert!(!pattern.parts.is_empty());
            }
            _ => panic!("expected FailRegex, got {s:?}"),
        }
    }

    #[test]
    fn fail_literal_statement() {
        let s = parse_stmt("!= bad output\n");
        match s {
            AstStmt::FailLiteral { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::Literal { value, .. } if value == "bad output")
                );
            }
            _ => panic!("expected FailLiteral, got {s:?}"),
        }
    }

    #[test]
    fn buffer_reset_from_match_regex() {
        let s = parse_stmt("<?\n");
        assert!(matches!(s, AstStmt::BufferReset { .. }));
    }

    #[test]
    fn buffer_reset_from_match_literal() {
        let s = parse_stmt("<=\n");
        assert!(matches!(s, AstStmt::BufferReset { .. }));
    }

    #[test]
    fn clear_fail_from_fail_regex() {
        let s = parse_stmt("!?\n");
        assert!(matches!(s, AstStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn clear_fail_from_fail_literal() {
        let s = parse_stmt("!=\n");
        assert!(matches!(s, AstStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn timed_match_literal() {
        let s = parse_stmt("<~5s= expected\n");
        match s {
            AstStmt::TimedMatchLiteral {
                timeout_kind,
                duration,
                ..
            } => {
                assert!(matches!(timeout_kind, AstTimeoutKind::Tolerance { .. }));
                assert_eq!(duration, "5s");
            }
            _ => panic!("expected TimedMatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn timed_match_regex() {
        let s = parse_stmt("<@2s? \\d+\n");
        match s {
            AstStmt::TimedMatchRegex {
                timeout_kind,
                duration,
                ..
            } => {
                assert!(matches!(timeout_kind, AstTimeoutKind::Assertion { .. }));
                assert_eq!(duration, "2s");
            }
            _ => panic!("expected TimedMatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn timeout_statement() {
        let s = parse_stmt("~10s\n");
        match s {
            AstStmt::Timeout { kind, duration, .. } => {
                assert!(matches!(kind, AstTimeoutKind::Tolerance { .. }));
                assert_eq!(duration, "10s");
            }
            _ => panic!("expected Timeout, got {s:?}"),
        }
    }

    #[test]
    fn let_without_value() {
        let s = parse_stmt("let x\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node, "x");
                assert!(l.value.is_none());
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn let_with_value() {
        let s = parse_stmt("let x = my_var\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node, "x");
                assert!(l.value.is_some());
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn assign_statement() {
        let s = parse_stmt("x = my_var\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node, "x");
            }
            _ => panic!("expected Assign, got {s:?}"),
        }
    }

    #[test]
    fn expr_statement() {
        let s = parse_stmt("foo()\n");
        match s {
            AstStmt::Expr {
                expr: AstExpr::Call { call, .. },
                ..
            } => {
                assert_eq!(call.name.node, "foo");
            }
            _ => panic!("expected Expr(Call), got {s:?}"),
        }
    }

    #[test]
    fn comment_statement() {
        let s = parse_stmt("// my comment\n");
        match s {
            AstStmt::Comment { text, .. } => assert_eq!(text, "my comment"),
            _ => panic!("expected Comment, got {s:?}"),
        }
    }

    #[test]
    fn leading_whitespace_is_consumed() {
        let s = parse_stmt("  > echo hello\n");
        assert!(matches!(s, AstStmt::Send { .. }));
    }

    #[test]
    fn send_with_interpolation() {
        let s = parse_stmt("> echo ${name}\n");
        match s {
            AstStmt::Send { payload, .. } => {
                assert_eq!(payload.parts.len(), 2);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::Literal { value, .. } if value == "echo ")
                );
                assert!(
                    matches!(&payload.parts[1], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected Send, got {s:?}"),
        }
    }

    #[test]
    fn assertion_timeout_statement() {
        let s = parse_stmt("@5s\n");
        match s {
            AstStmt::Timeout { kind, duration, .. } => {
                assert!(matches!(kind, AstTimeoutKind::Assertion { .. }));
                assert_eq!(duration, "5s");
            }
            _ => panic!("expected Timeout, got {s:?}"),
        }
    }

    #[test]
    fn let_with_string_value() {
        let s = parse_stmt("let x = \"hello\"\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node, "x");
                assert!(l.value.is_some());
                assert!(matches!(l.value.unwrap().node, AstExpr::String { .. }));
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn let_with_call_value() {
        let s = parse_stmt("let x = foo()\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node, "x");
                assert!(l.value.is_some());
                assert!(matches!(l.value.unwrap().node, AstExpr::Call { .. }));
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn match_regex_with_interpolation() {
        let s = parse_stmt("<? ${name}.*\n");
        match s {
            AstStmt::MatchRegex { pattern, .. } => {
                assert!(pattern.parts.len() >= 2);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected MatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn fail_regex_with_interpolation() {
        let s = parse_stmt("!? ${err}.*\n");
        match s {
            AstStmt::FailRegex { pattern, .. } => {
                assert!(pattern.parts.len() >= 2);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "err")
                );
            }
            _ => panic!("expected FailRegex, got {s:?}"),
        }
    }

    #[test]
    fn timed_match_literal_with_interpolation() {
        let s = parse_stmt("<~5s= hello ${name}\n");
        match s {
            AstStmt::TimedMatchLiteral { pattern, .. } => {
                assert!(pattern.node.parts.len() >= 2);
                assert!(
                    matches!(&pattern.node.parts[0], AstStringPart::Literal { value, .. } if value == "hello ")
                );
                assert!(
                    matches!(&pattern.node.parts[1], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected TimedMatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn send_raw_with_interpolation() {
        let s = parse_stmt("=> ${var} data\n");
        match s {
            AstStmt::SendRaw { payload, .. } => {
                assert!(payload.parts.len() >= 2);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::VarRef { name, .. } if name == "var")
                );
            }
            _ => panic!("expected SendRaw, got {s:?}"),
        }
    }

    #[test]
    fn match_literal_with_interpolation() {
        let s = parse_stmt("<= hello ${name}\n");
        match s {
            AstStmt::MatchLiteral { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 2);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::Literal { value, .. } if value == "hello ")
                );
                assert!(
                    matches!(&pattern.parts[1], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected MatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn fail_literal_with_interpolation() {
        let s = parse_stmt("!= ${err} happened\n");
        match s {
            AstStmt::FailLiteral { pattern, .. } => {
                assert!(pattern.parts.len() >= 2);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "err")
                );
            }
            _ => panic!("expected FailLiteral, got {s:?}"),
        }
    }

    #[test]
    fn assign_with_string_value() {
        let s = parse_stmt("x = \"hello\"\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node, "x");
                assert!(matches!(a.value.node, AstExpr::String { .. }));
            }
            _ => panic!("expected Assign, got {s:?}"),
        }
    }

    #[test]
    fn assign_with_call_value() {
        let s = parse_stmt("x = foo()\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node, "x");
                assert!(matches!(a.value.node, AstExpr::Call { .. }));
            }
            _ => panic!("expected Assign, got {s:?}"),
        }
    }

    #[test]
    fn timed_match_regex_with_interpolation() {
        let s = parse_stmt("<@3s? ${pat}.*\n");
        match s {
            AstStmt::TimedMatchRegex {
                timeout_kind,
                duration,
                pattern,
                ..
            } => {
                assert!(matches!(timeout_kind, AstTimeoutKind::Assertion { .. }));
                assert_eq!(duration, "3s");
                assert!(pattern.node.parts.len() >= 2);
                assert!(
                    matches!(&pattern.node.parts[0], AstStringPart::VarRef { name, .. } if name == "pat")
                );
            }
            _ => panic!("expected TimedMatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn buffer_reset_whitespace_only_regex() {
        let s = parse_stmt("<?   \n");
        assert!(matches!(s, AstStmt::BufferReset { .. }));
    }

    #[test]
    fn buffer_reset_whitespace_only_literal() {
        let s = parse_stmt("<=   \n");
        assert!(matches!(s, AstStmt::BufferReset { .. }));
    }

    #[test]
    fn clear_fail_whitespace_only_regex() {
        let s = parse_stmt("!?   \n");
        assert!(matches!(s, AstStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn clear_fail_whitespace_only_literal() {
        let s = parse_stmt("!=   \n");
        assert!(matches!(s, AstStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn timed_match_regex_tolerance() {
        let s = parse_stmt("<~5s? \\d+\n");
        match s {
            AstStmt::TimedMatchRegex {
                timeout_kind,
                duration,
                ..
            } => {
                assert!(matches!(timeout_kind, AstTimeoutKind::Tolerance { .. }));
                assert_eq!(duration, "5s");
            }
            _ => panic!("expected TimedMatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn tab_indentation() {
        let s = parse_stmt("\t> echo hello\n");
        assert!(matches!(s, AstStmt::Send { .. }));
    }

    #[test]
    fn expr_statement_with_args() {
        let s = parse_stmt("foo(\"hello\")\n");
        match s {
            AstStmt::Expr {
                expr: AstExpr::Call { call, .. },
                ..
            } => {
                assert_eq!(call.name.node, "foo");
                assert_eq!(call.args.len(), 1);
            }
            _ => panic!("expected Expr(Call), got {s:?}"),
        }
    }

    #[test]
    fn match_literal_only_var_ref() {
        let s = parse_stmt("<= ${var}\n");
        match s {
            AstStmt::MatchLiteral { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "var")
                );
            }
            _ => panic!("expected MatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn let_underscore_variable() {
        let s = parse_stmt("let _private = \"secret\"\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node, "_private");
                assert!(l.value.is_some());
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn send_no_space_after_operator() {
        let s = parse_stmt(">hello\n");
        match s {
            AstStmt::Send { payload, .. } => {
                assert_eq!(payload.parts.len(), 1);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::Literal { value, .. } if value == "hello")
                );
            }
            _ => panic!("expected Send, got {s:?}"),
        }
    }

    #[test]
    fn match_regex_only_var_ref() {
        let s = parse_stmt("<? ${pat}\n");
        match s {
            AstStmt::MatchRegex { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "pat")
                );
            }
            _ => panic!("expected MatchRegex, got {s:?}"),
        }
    }
}
