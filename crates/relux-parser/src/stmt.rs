use chumsky::prelude::*;

use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::annotation::comment;
use super::expr::expr;
use super::ident::ident_var;
use super::interpolation::interp_literal;
use super::interpolation::interp_regex;
use super::operator::legacy_assign_err;
use super::operator::op_bind;
use super::operator::op_fail_literal;
use super::operator::op_fail_regex;
use super::operator::op_match_literal;
use super::operator::op_match_regex;
use super::operator::op_multimatch_open;
use super::operator::op_send;
use super::operator::op_send_raw;
use super::operator::op_timed_match_literal;
use super::operator::op_timed_match_regex;
use super::operator::op_timed_multimatch_open;
use super::punctuation::punctuation_brace_close;
use super::timeout::timeout;
use super::token::keyword;
use super::ws::leading_ws;
use super::ws::newline;
use super::ws::ws;
use relux_ast::AstAssignStmt;
use relux_ast::AstInterpolation;
use relux_ast::AstLetStmt;
use relux_ast::AstMultiMatchPattern;
use relux_ast::AstStmt;
use relux_ast::AstStringPart;

// --- L4: Statement Combinators ---------------------------

/// `> payload` -> `AstStmt::Send`
fn stmt_send<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_send()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::Send {
                    payload: payload.node,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `=> payload` -> `AstStmt::SendRaw`
fn stmt_send_raw<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_send_raw()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::SendRaw {
                    payload: payload.node,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `<? payload` -> `AstStmt::MatchRegex`, or `<?` alone -> `AstStmt::BufferReset`
fn stmt_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_match_regex()
        .ignore_then(ws())
        .ignore_then(interp_regex(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
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
        .then_ignore(newline())
}

/// `<= payload` -> `AstStmt::MatchLiteral`, or `<=` alone -> `AstStmt::BufferReset`
fn stmt_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_match_literal()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
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
        .then_ignore(newline())
}

/// `!? payload` -> `AstStmt::FailRegex`, or `!?` alone -> `AstStmt::ClearFailPattern`
fn stmt_fail_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_fail_regex()
        .ignore_then(ws())
        .ignore_then(interp_regex(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
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
        .then_ignore(newline())
}

/// `!= payload` -> `AstStmt::FailLiteral`, or `!=` alone -> `AstStmt::ClearFailPattern`
fn stmt_fail_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_fail_literal()
        .ignore_then(ws())
        .ignore_then(interp_literal(Token::Newline))
        .map_with(|payload, e| {
            let span = crate::span_from_chumsky(e.span());
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
        .then_ignore(newline())
}

/// `<~5s= payload` or `<@2s= payload` -> `AstStmt::TimedMatchLiteral`
fn stmt_timed_match_literal<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_timed_match_literal()
        .then_ignore(ws())
        .then(interp_literal(Token::Newline))
        .map_with(|(t, payload), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::TimedMatchLiteral {
                    timeout: t.node,
                    pattern: payload,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `<~5s? payload` or `<@2s? payload` -> `AstStmt::TimedMatchRegex`
fn stmt_timed_match_regex<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    op_timed_match_regex()
        .then_ignore(ws())
        .then(interp_regex(Token::Newline))
        .map_with(|(t, payload), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::TimedMatchRegex {
                    timeout: t.node,
                    pattern: payload,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// One inner line of a multimatch block: `? pat` or `= pat`, terminated by newline.
fn multimatch_inner_line<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstMultiMatchPattern>, extra::Err<Rich<'a, Token<'a>>>>
+ Clone {
    let regex_line = leading_ws()
        .ignore_then(just(Token::Question).map_with(|_, e| e.span()))
        .then_ignore(ws())
        .then(interp_regex(Token::Newline))
        .map_with(|(_, payload), e| {
            let span = crate::span_from_chumsky(e.span());
            let pat = AstMultiMatchPattern {
                pattern: payload.node,
                is_regex: true,
                span,
            };
            Spanned::new(pat, span)
        })
        .then_ignore(newline());

    let literal_line = leading_ws()
        .ignore_then(just(Token::Eq).map_with(|_, e| e.span()))
        .then_ignore(ws())
        .then(interp_literal(Token::Newline))
        .map_with(|(_, payload), e| {
            let span = crate::span_from_chumsky(e.span());
            let pat = AstMultiMatchPattern {
                pattern: payload.node,
                is_regex: false,
                span,
            };
            Spanned::new(pat, span)
        })
        .then_ignore(newline());

    choice((regex_line, literal_line))
        .labelled("multimatch inner pattern (? <regex> or = <literal>)")
}

/// `<{ <line>+ }` or `<~Ns{ <line>+ }` or `<@Ns{ <line>+ }` -> `AstStmt::MultiMatch`
fn stmt_multimatch<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    // Either a real inner line or a blank/comment line. Blank/comment lines
    // produce `None`; the collector filters them out so they do not count
    // toward the at-least-one-pattern requirement.
    let inner = choice((
        multimatch_inner_line().map(Some),
        leading_ws().ignore_then(comment()).map(|_| None),
        ws().ignore_then(newline()).map(|_| None),
    ));

    let untimed = op_multimatch_open()
        .ignore_then(ws())
        .ignore_then(newline().or_not())
        .ignore_then(inner.clone().repeated().collect::<Vec<_>>())
        .then_ignore(leading_ws())
        .then_ignore(punctuation_brace_close())
        .map_with(|patterns, e| {
            let span = crate::span_from_chumsky(e.span());
            let patterns: Vec<_> = patterns.into_iter().flatten().collect();
            (None, patterns, span)
        });

    let timed = op_timed_multimatch_open()
        .then_ignore(ws())
        .then_ignore(newline().or_not())
        .then(inner.repeated().collect::<Vec<_>>())
        .then_ignore(leading_ws())
        .then_ignore(punctuation_brace_close())
        .map_with(|(t, patterns), e| {
            let span = crate::span_from_chumsky(e.span());
            let patterns: Vec<_> = patterns.into_iter().flatten().collect();
            (Some(t.node), patterns, span)
        });

    choice((timed, untimed))
        .try_map(|(timeout, patterns, span), chumsky_span| {
            if patterns.is_empty() {
                Err(Rich::custom(
                    chumsky_span,
                    "multimatch block must contain at least one pattern",
                ))
            } else {
                Ok(Spanned::new(
                    AstStmt::MultiMatch {
                        timeout,
                        patterns,
                        span,
                    },
                    span,
                ))
            }
        })
        .then_ignore(newline().or_not())
        .labelled("multimatch statement (<{ ... } or <~Ns{ ... } or <@Ns{ ... })")
}

/// `~5s` or `@10s` followed by newline -> `AstStmt::Timeout`
fn stmt_timeout<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    timeout()
        .map_with(|t, e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::Timeout {
                    timeout: t.node,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `let name [:= expr]` -> `AstStmt::Let`
fn stmt_let<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    let initializer = ws()
        .ignore_then(choice((
            op_bind().ignore_then(ws()).ignore_then(expr()).map(Some),
            // Legacy `let x = e`: emit the migration hint.
            just(Token::Eq)
                .map_with(|_, e| e.span())
                .try_map(|span, _| Err(legacy_assign_err(span))),
        )))
        .or_not()
        .map(Option::flatten);

    keyword(Token::Let)
        .ignore_then(ws())
        .ignore_then(ident_var())
        .then(initializer)
        .map_with(|(name, value), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::Let {
                    stmt: AstLetStmt { name, value, span },
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `name := expr` -> `AstStmt::Assign`. A bare `name = expr` emits the R013
/// migration hint: the legacy `=` path lives in the inner `choice` (after the
/// key is committed and with `ws()` outside the choice) so its custom error is
/// the furthest alternative and is surfaced, mirroring `stmt_let`.
fn stmt_assign<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ident_var()
        .then(
            ws().ignore_then(choice((
                op_bind().ignore_then(ws()).ignore_then(expr()),
                just(Token::Eq)
                    .map_with(|_, e| e.span())
                    .try_map(|span, _| Err(legacy_assign_err(span))),
            ))),
        )
        .map_with(|(name, value), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstStmt::Assign {
                    stmt: AstAssignStmt { name, value, span },
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
}

/// `expr` -> `AstStmt::Expr` (catch-all for bare function calls)
fn stmt_expr<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    expr()
        .map_with(|e, extra| {
            let span = crate::span_from_chumsky(extra.span());
            Spanned::new(AstStmt::Expr { expr: e.node, span }, span)
        })
        .then_ignore(newline())
}

/// Full statement combinator: `leading_ws()` then ordered choice.
pub fn stmt<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    let stmt_comment = comment().map_with(|s, e| {
        let span = crate::span_from_chumsky(e.span());
        Spanned::new(AstStmt::Comment { text: s, span }, span)
    });

    leading_ws()
        .ignore_then(
            choice((
                stmt_comment,
                stmt_multimatch(),
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
        .boxed()
}

/// `stmt_let()` exported for use in effect/test body sections.
pub fn stmt_let_standalone<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstStmt>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    stmt_let()
}

// --- Helpers ---------------------------------------------

fn is_empty_payload(interp: &AstInterpolation) -> bool {
    interp.parts.is_empty()
        || interp
            .parts
            .iter()
            .all(|p| matches!(p, AstStringPart::Literal { value, .. } if value.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;
    use relux_ast::AstExpr;
    use relux_ast::AstStmt;
    use relux_ast::AstTimeout;

    fn parse_stmt(source: &str) -> AstStmt {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        stmt().parse(input).into_result().unwrap().node
    }

    fn parse_stmt_err(source: &str) -> String {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        stmt()
            .parse(input)
            .into_result()
            .unwrap_err()
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
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
            AstStmt::TimedMatchLiteral { timeout, .. } => {
                assert!(matches!(timeout, AstTimeout::Tolerance { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(5));
            }
            _ => panic!("expected TimedMatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn timed_match_regex() {
        let s = parse_stmt("<@2s? \\d+\n");
        match s {
            AstStmt::TimedMatchRegex { timeout, .. } => {
                assert!(matches!(timeout, AstTimeout::Assertion { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(2));
            }
            _ => panic!("expected TimedMatchRegex, got {s:?}"),
        }
    }

    #[test]
    fn timeout_statement() {
        let s = parse_stmt("~10s\n");
        match s {
            AstStmt::Timeout { timeout, .. } => {
                assert!(matches!(timeout, AstTimeout::Tolerance { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(10));
            }
            _ => panic!("expected Timeout, got {s:?}"),
        }
    }

    #[test]
    fn let_binds_with_walrus() {
        let s = parse_stmt("let x := \"v\"\n");
        assert!(matches!(s, AstStmt::Let { .. }));
    }

    #[test]
    fn legacy_let_eq_reports_migration_hint() {
        let err = parse_stmt_err("let x = \"v\"\n");
        assert!(
            err.contains("write `name := value`"),
            "expected := migration hint, got: {err}"
        );
    }

    #[test]
    fn let_without_value() {
        let s = parse_stmt("let x\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node.name, "x");
                assert!(l.value.is_none());
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn let_with_value() {
        let s = parse_stmt("let x := my_var\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node.name, "x");
                assert!(l.value.is_some());
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn assign_statement() {
        let s = parse_stmt("x := my_var\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node.name, "x");
            }
            _ => panic!("expected Assign, got {s:?}"),
        }
    }

    #[test]
    fn reassign_uses_walrus() {
        let stmt = parse_stmt("x := \"v\"\n");
        assert!(matches!(stmt, AstStmt::Assign { .. }));
    }

    #[test]
    fn legacy_reassign_eq_reports_migration_hint() {
        let err = parse_stmt_err("x = \"v\"\n");
        assert!(
            err.contains("write `name := value`"),
            "expected := migration hint, got: {err}"
        );
    }

    #[test]
    fn expr_statement() {
        let s = parse_stmt("foo()\n");
        match s {
            AstStmt::Expr {
                expr: AstExpr::Call { call, .. },
                ..
            } => {
                assert_eq!(call.name.node.name, "foo");
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
            AstStmt::Timeout { timeout, .. } => {
                assert!(matches!(timeout, AstTimeout::Assertion { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(5));
            }
            _ => panic!("expected Timeout, got {s:?}"),
        }
    }

    #[test]
    fn let_with_string_value() {
        let s = parse_stmt("let x := \"hello\"\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node.name, "x");
                assert!(l.value.is_some());
                assert!(matches!(l.value.unwrap().node, AstExpr::String { .. }));
            }
            _ => panic!("expected Let, got {s:?}"),
        }
    }

    #[test]
    fn let_with_call_value() {
        let s = parse_stmt("let x := foo()\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node.name, "x");
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
        let s = parse_stmt("=> ${val} data\n");
        match s {
            AstStmt::SendRaw { payload, .. } => {
                assert!(payload.parts.len() >= 2);
                assert!(
                    matches!(&payload.parts[0], AstStringPart::VarRef { name, .. } if name == "val")
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
        let s = parse_stmt("x := \"hello\"\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node.name, "x");
                assert!(matches!(a.value.node, AstExpr::String { .. }));
            }
            _ => panic!("expected Assign, got {s:?}"),
        }
    }

    #[test]
    fn assign_with_call_value() {
        let s = parse_stmt("x := foo()\n");
        match s {
            AstStmt::Assign { stmt: a, .. } => {
                assert_eq!(a.name.node.name, "x");
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
                timeout, pattern, ..
            } => {
                assert!(matches!(timeout, AstTimeout::Assertion { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(3));
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
            AstStmt::TimedMatchRegex { timeout, .. } => {
                assert!(matches!(timeout, AstTimeout::Tolerance { .. }));
                assert_eq!(timeout.duration(), Duration::from_secs(5));
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
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 1);
            }
            _ => panic!("expected Expr(Call), got {s:?}"),
        }
    }

    #[test]
    fn match_literal_only_var_ref() {
        let s = parse_stmt("<= ${val}\n");
        match s {
            AstStmt::MatchLiteral { pattern, .. } => {
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::VarRef { name, .. } if name == "val")
                );
            }
            _ => panic!("expected MatchLiteral, got {s:?}"),
        }
    }

    #[test]
    fn let_underscore_variable() {
        let s = parse_stmt("let _private := \"secret\"\n");
        match s {
            AstStmt::Let { stmt: l, .. } => {
                assert_eq!(l.name.node.name, "_private");
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

    #[test]
    fn multimatch_two_regex_patterns() {
        let s = parse_stmt(
            r#"<{
  ? ^job-a: done$
  ? ^job-b: done$
}
"#,
        );
        match s {
            AstStmt::MultiMatch {
                timeout, patterns, ..
            } => {
                assert!(timeout.is_none());
                assert_eq!(patterns.len(), 2);
                assert!(patterns[0].node.is_regex);
                assert!(patterns[1].node.is_regex);
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_mixed_literal_and_regex() {
        let s = parse_stmt(
            r#"<{
  = batch complete
  ? ^\d+ items processed$
}
"#,
        );
        match s {
            AstStmt::MultiMatch { patterns, .. } => {
                assert_eq!(patterns.len(), 2);
                assert!(
                    !patterns[0].node.is_regex,
                    "first pattern should be literal"
                );
                assert!(patterns[1].node.is_regex, "second pattern should be regex");
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_with_tolerance_timeout() {
        let s = parse_stmt(
            r#"<~10s{
  = a
  = b
}
"#,
        );
        match s {
            AstStmt::MultiMatch {
                timeout, patterns, ..
            } => {
                let t = timeout.expect("expected a timeout");
                assert!(matches!(t, AstTimeout::Tolerance { .. }));
                assert_eq!(t.duration(), Duration::from_secs(10));
                assert_eq!(patterns.len(), 2);
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_with_assertion_timeout() {
        let s = parse_stmt(
            r#"<@30s{
  ? ^a$
  ? ^b$
  ? ^c$
}
"#,
        );
        match s {
            AstStmt::MultiMatch {
                timeout, patterns, ..
            } => {
                let t = timeout.expect("expected a timeout");
                assert!(matches!(t, AstTimeout::Assertion { .. }));
                assert_eq!(patterns.len(), 3);
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_comments_between_patterns() {
        let s = parse_stmt(
            r#"<{
  // first line
  ? ^a$
  // mid
  ? ^b$
}
"#,
        );
        match s {
            AstStmt::MultiMatch { patterns, .. } => {
                assert_eq!(
                    patterns.len(),
                    2,
                    "comments must not be counted as patterns"
                );
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_single_pattern_parses() {
        let s = parse_stmt(
            r#"<{
  ? ^solo$
}
"#,
        );
        match s {
            AstStmt::MultiMatch { patterns, .. } => assert_eq!(patterns.len(), 1),
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_with_interpolation_in_pattern() {
        let s = parse_stmt(
            r#"<{
  ? ^user=${name}$
}
"#,
        );
        match s {
            AstStmt::MultiMatch { patterns, .. } => {
                assert_eq!(patterns.len(), 1);
                let parts = &patterns[0].node.pattern.parts;
                assert!(
                    parts
                        .iter()
                        .any(|p| matches!(p, AstStringPart::VarRef { name, .. } if name == "name"))
                );
            }
            _ => panic!("expected MultiMatch, got {s:?}"),
        }
    }

    #[test]
    fn multimatch_empty_body_is_parse_error() {
        let source = r#"<{
}
"#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = stmt().parse(input).into_result();
        assert!(
            result.is_err(),
            "empty multimatch body must be a parse error"
        );
        let errs = format!("{:?}", result.unwrap_err());
        // Chumsky's Debug format reports the expected tokens, not the labelled
        // context. After `<{` the next token must be `?` (regex pattern) or
        // `=` (literal pattern); the error message lists those alongside the
        // timed-form expectation. Either signal counts as a useful hint.
        assert!(
            errs.contains("multimatch")
                || errs.contains("pattern")
                || errs.contains("'?'")
                || errs.contains("'='"),
            "expected error to mention multimatch context; got {errs}"
        );
    }
}
