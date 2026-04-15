use chumsky::prelude::*;

use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::expr::expr;
use super::interpolation::interp_regex;
use super::prefix::prefix_comment;
use super::prefix::prefix_marker;
use super::token::text;
use super::ws::newline;
use super::ws::ws;
use relux_ast::AstCondModifier;
use relux_ast::AstMarkerCond;
use relux_ast::AstMarkerCondBody;
use relux_ast::AstMarkerDecl;
use relux_ast::AstMarkerKind;

// ─── L4: Annotation Combinators ─────────────────────────────

/// `// comment text` — consumes through newline, returns comment text.
pub fn comment<'a>()
-> impl Parser<'a, ParserInput<'a>, String, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    prefix_comment()
        .ignore_then(ws())
        .ignore_then(
            none_of([Token::Newline])
                .repeated()
                .collect::<Vec<Token<'a>>>(),
        )
        .then_ignore(newline())
        .map(|tokens| {
            let s: String = tokens.iter().map(|t| t.to_string()).collect();
            s.trim_end().to_string()
        })
        .labelled("comment")
        .boxed()
}

/// `"""..."""` — docstring. Single/double quotes inside are valid content;
/// only three consecutive quotes close the docstring.
pub fn docstring<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    use super::ws::docstring_delim;

    docstring_delim()
        .ignore_then(
            // Match anything that isn't three consecutive quotes.
            // We use a custom approach: collect tokens until we see `"""`.
            // Token::Newline displays as literal "\n" (for debug), so we
            // map it to a real newline character here.
            just(Token::Newline)
                .to("\n".to_string())
                .or(none_of([Token::Quote, Token::Newline]).map(|tok: Token<'a>| tok.to_string()))
                .or(
                    // A quote that is NOT followed by two more quotes
                    just(Token::Quote)
                        .then(just(Token::Quote).not().rewind())
                        .to("\"".to_string()),
                )
                .or(
                    // Two quotes NOT followed by a third
                    just(Token::Quote)
                        .then(just(Token::Quote))
                        .then(just(Token::Quote).not().rewind())
                        .to("\"\"".to_string()),
                )
                .repeated()
                .collect::<Vec<String>>(),
        )
        .then_ignore(docstring_delim())
        .map_with(|parts, e| {
            let s: String = parts.join("");
            Spanned::new(s, crate::span_from_chumsky(e.span()))
        })
        .labelled("docstring")
        .boxed()
}

/// Marker condition body: `expr`, `expr = expr`, or `expr ? regex`.
fn marker_cond_body<'a>()
-> impl Parser<'a, ParserInput<'a>, AstMarkerCondBody, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    let eq_cond = expr()
        .then_ignore(ws().then(just(Token::Eq)).then(ws()))
        .then(expr())
        .map_with(|(lhs, rhs), e| AstMarkerCondBody::Eq {
            lhs: lhs.node,
            rhs: rhs.node,
            span: crate::span_from_chumsky(e.span()),
        });

    let regex_cond = expr()
        .then_ignore(ws().then(just(Token::Question)).then(ws()))
        .then(interp_regex(Token::Newline))
        .map_with(|(lhs, pat), e| AstMarkerCondBody::Regex {
            expr: lhs.node,
            pattern: pat.node,
            span: crate::span_from_chumsky(e.span()),
        });

    let bare_cond = expr().map_with(|e, extra| AstMarkerCondBody::Bare {
        expr: e.node,
        span: crate::span_from_chumsky(extra.span()),
    });

    choice((eq_cond, regex_cond, bare_cond))
}

/// `# skip/run/flaky [if/unless cond]` — marker declaration.
pub fn marker<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstMarkerDecl>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    let kind = text().try_map(|(s, span): (&str, SimpleSpan), _extra| match s {
        "skip" => Ok(AstMarkerKind::Skip {
            span: crate::span_from_chumsky(span),
        }),
        "run" => Ok(AstMarkerKind::Run {
            span: crate::span_from_chumsky(span),
        }),
        "flaky" => Ok(AstMarkerKind::Flaky {
            span: crate::span_from_chumsky(span),
        }),
        _ => Err(Rich::custom(
            span,
            format!("expected `skip`, `run`, or `flaky`, found `{s}`"),
        )),
    });

    let modifier = text().try_map(|(s, span): (&str, SimpleSpan), _extra| match s {
        "if" => Ok(AstCondModifier::If {
            span: crate::span_from_chumsky(span),
        }),
        "unless" => Ok(AstCondModifier::Unless {
            span: crate::span_from_chumsky(span),
        }),
        _ => Err(Rich::custom(
            span,
            format!("expected `if` or `unless`, found `{s}`"),
        )),
    });

    let condition = ws()
        .ignore_then(modifier)
        .then_ignore(ws())
        .then(marker_cond_body())
        .map_with(|(modifier, body), e| AstMarkerCond {
            modifier,
            body,
            span: crate::span_from_chumsky(e.span()),
        });

    prefix_marker()
        .ignore_then(ws())
        .ignore_then(kind)
        .then(condition.or_not())
        .map_with(|(kind, condition), e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstMarkerDecl {
                    kind,
                    condition,
                    span,
                },
                span,
            )
        })
        .then_ignore(newline())
        .labelled("marker (# skip/run/flaky)")
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;
    use relux_ast::AstExpr;
    use relux_ast::AstStringPart;

    fn parse_comment(source: &str) -> String {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        comment().parse(input).into_result().unwrap()
    }

    fn parse_marker(source: &str) -> AstMarkerDecl {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        marker().parse(input).into_result().unwrap().node
    }

    #[test]
    fn simple_comment() {
        assert_eq!(parse_comment("// hello world\n"), "hello world");
    }

    #[test]
    fn comment_trims_trailing_whitespace() {
        assert_eq!(parse_comment("// trailing   \n"), "trailing");
    }

    #[test]
    fn empty_comment() {
        assert_eq!(parse_comment("//\n"), "");
    }

    #[test]
    fn docstring_simple() {
        let source = r#""""hello world""""#;
        let full = source.to_string();
        let pairs = lex_to_pairs(&full);
        let input = make_input(&pairs, full.len());
        let result = docstring().parse(input).into_result().unwrap();
        assert_eq!(result.node, "hello world");
    }

    #[test]
    fn docstring_with_internal_quotes() {
        let source = r#""""say "hi" please""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = docstring().parse(input).into_result().unwrap();
        assert_eq!(result.node, "say \"hi\" please");
    }

    #[test]
    fn marker_skip() {
        let m = parse_marker("# skip\n");
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        assert!(m.condition.is_none());
    }

    #[test]
    fn marker_run() {
        let m = parse_marker("# run\n");
        assert!(matches!(m.kind, AstMarkerKind::Run { .. }));
        assert!(m.condition.is_none());
    }

    #[test]
    fn marker_flaky() {
        let m = parse_marker("# flaky\n");
        assert!(matches!(m.kind, AstMarkerKind::Flaky { .. }));
        assert!(m.condition.is_none());
    }

    #[test]
    fn marker_skip_if_bare() {
        let m = parse_marker("# skip if MY_VAR\n");
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::If { .. }));
        assert!(
            matches!(cond.body, AstMarkerCondBody::Bare { expr: AstExpr::Var { ref name, .. }, .. } if name == "MY_VAR")
        );
    }

    #[test]
    fn marker_run_unless_eq() {
        let m = parse_marker("# run unless MY_VAR = expected\n");
        assert!(matches!(m.kind, AstMarkerKind::Run { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::Unless { .. }));
        match cond.body {
            AstMarkerCondBody::Eq {
                lhs: AstExpr::Var { name: ref l, .. },
                rhs: AstExpr::Var { name: ref r, .. },
                ..
            } => {
                assert_eq!(l, "MY_VAR");
                assert_eq!(r, "expected");
            }
            _ => panic!("expected Eq condition, got {:?}", cond.body),
        }
    }

    #[test]
    fn marker_skip_if_regex() {
        let source = "# skip if MY_VAR ? \\d+\n";
        let m = parse_marker(source);
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::If { .. }));
        match cond.body {
            AstMarkerCondBody::Regex {
                expr: AstExpr::Var { name: ref v, .. },
                ref pattern,
                ..
            } => {
                assert_eq!(v, "MY_VAR");
                assert_eq!(pattern.parts.len(), 1);
                assert!(
                    matches!(&pattern.parts[0], AstStringPart::Literal { value, .. } if value == r"\d+")
                );
            }
            _ => panic!("expected Regex condition, got {:?}", cond.body),
        }
    }

    #[test]
    fn marker_skip_if_function_call() {
        let m = parse_marker("# skip if which(\"jq\")\n");
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::If { .. }));
        assert!(matches!(
            cond.body,
            AstMarkerCondBody::Bare {
                expr: AstExpr::Call { .. },
                ..
            }
        ));
    }

    #[test]
    fn docstring_with_newlines() {
        let source = r#""""
line one
line two
""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = docstring().parse(input).into_result().unwrap();
        assert!(result.node.contains("line one"));
        assert!(result.node.contains("line two"));
    }

    #[test]
    fn docstring_empty() {
        let source = r#""""""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = docstring().parse(input).into_result().unwrap();
        assert_eq!(result.node, "");
    }

    #[test]
    fn marker_invalid_kind_rejected() {
        let source = "# invalid\n";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(marker().parse(input).into_result().is_err());
    }

    #[test]
    fn marker_skip_if_eq_with_strings() {
        let m = parse_marker("# skip if env(\"FOO\") = \"bar\"\n");
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::If { .. }));
        match cond.body {
            AstMarkerCondBody::Eq {
                lhs: AstExpr::Call { .. },
                rhs: AstExpr::String { .. },
                ..
            } => {}
            _ => panic!("expected Eq(Call, String) condition, got {:?}", cond.body),
        }
    }

    #[test]
    fn marker_run_if_bare() {
        let m = parse_marker("# run if MY_VAR\n");
        assert!(matches!(m.kind, AstMarkerKind::Run { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::If { .. }));
        assert!(matches!(
            cond.body,
            AstMarkerCondBody::Bare {
                expr: AstExpr::Var { .. },
                ..
            }
        ));
    }

    #[test]
    fn docstring_with_adjacent_double_quotes() {
        let source = r#""""has "" inside""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = docstring().parse(input).into_result().unwrap();
        assert_eq!(result.node, "has \"\" inside");
    }

    #[test]
    fn marker_skip_unless_bare() {
        let m = parse_marker("# skip unless MY_VAR\n");
        assert!(matches!(m.kind, AstMarkerKind::Skip { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::Unless { .. }));
        assert!(matches!(
            cond.body,
            AstMarkerCondBody::Bare {
                expr: AstExpr::Var { .. },
                ..
            }
        ));
    }

    #[test]
    fn comment_with_special_chars() {
        let c = parse_comment("// hello { world } (test) = !\n");
        assert!(c.contains('{'));
        assert!(c.contains('}'));
        assert!(c.contains('('));
        assert!(c.contains(')'));
    }

    #[test]
    fn marker_skip_without_modifier_rejects() {
        // `# skip MY_VAR` without `if`/`unless` modifier should fail
        let source = "# skip MY_VAR\n";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(marker().parse(input).into_result().is_err());
    }

    #[test]
    fn marker_run_unless_regex() {
        let source = "# run unless MY_VAR ? ^prod$\n";
        let m = parse_marker(source);
        assert!(matches!(m.kind, AstMarkerKind::Run { .. }));
        let cond = m.condition.unwrap();
        assert!(matches!(cond.modifier, AstCondModifier::Unless { .. }));
        assert!(matches!(cond.body, AstMarkerCondBody::Regex { .. }));
    }

    #[test]
    fn marker_flaky_has_no_condition() {
        let m = parse_marker("# flaky\n");
        assert!(matches!(m.kind, AstMarkerKind::Flaky { .. }));
        assert!(m.condition.is_none());
    }
}
