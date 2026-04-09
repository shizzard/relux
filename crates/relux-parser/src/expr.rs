use chumsky::prelude::*;

use relux_core::Spanned;
use relux_lexer::Token;

use super::ParserInput;
use super::ident::expr_numeric;
use super::ident::ident_fn;
use super::ident::ident_var;
use super::interpolation::interp_literal;
use super::punctuation::punctuation_comma;
use super::punctuation::punctuation_paren_close;
use super::punctuation::punctuation_paren_open;
use super::ws::ws;
use relux_ast::AstCallExpr;
use relux_ast::AstExpr;
use relux_ast::AstInterpolation;
use relux_ast::AstStringPart;

/// Plain string: `"..."` — all tokens between quotes are literal text (no interpolation).
/// Used for test names.
pub fn plain_string<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Quote)
        .ignore_then(
            none_of([Token::Quote, Token::Newline])
                .repeated()
                .collect::<Vec<Token<'a>>>(),
        )
        .then_ignore(just(Token::Quote))
        .map_with(|tokens, e| {
            let s: String = tokens.iter().map(|t| t.to_string()).collect();
            Spanned::new(s, crate::span_from_chumsky(e.span()))
        })
        .labelled("plain string")
}

/// Any expression: function call, string, capture ref, numeric literal, or variable.
/// Uses `recursive()` because function call arguments are themselves expressions.
pub fn expr<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstExpr>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    recursive(|expr_rec| {
        // String expression: `"..."` with interpolation
        let expr_string = just(Token::Quote)
            .ignore_then(interp_literal(Token::Quote))
            .then_ignore(just(Token::Quote))
            .map_with(|interp, e| {
                let span = crate::span_from_chumsky(e.span());
                Spanned::new(
                    AstExpr::String {
                        interp: interp.node,
                        span,
                    },
                    span,
                )
            });

        // Function call: `fn_name(arg1, arg2)`
        let expr_call = ident_fn()
            .then(
                punctuation_paren_open()
                    .ignore_then(
                        expr_rec
                            .separated_by(ws().then(punctuation_comma()).then(ws()))
                            .allow_trailing()
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(punctuation_paren_close()),
            )
            .map_with(|(name, args), e| {
                let span = crate::span_from_chumsky(e.span());
                Spanned::new(
                    AstExpr::Call {
                        call: AstCallExpr { name, args, span },
                        span,
                    },
                    span,
                )
            });

        // Capture reference: `$1`
        let expr_capture_ref =
            just(Token::Dollar)
                .ignore_then(expr_numeric())
                .map_with(|num, e| {
                    let span = crate::span_from_chumsky(e.span());
                    Spanned::new(
                        AstExpr::CaptureRef {
                            index: num.node.parse::<usize>().unwrap(),
                            span,
                        },
                        span,
                    )
                });

        // Numeric literal: `42` → AstExpr::String with single literal part
        let expr_numeric_lit = expr_numeric().map_with(|num, e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstExpr::String {
                    interp: AstInterpolation {
                        parts: vec![AstStringPart::Literal {
                            value: num.node,
                            span,
                        }],
                        span,
                    },
                    span,
                },
                span,
            )
        });

        // Variable reference: `my_var`
        let expr_ident = ident_var().map_with(|name, e| {
            let span = crate::span_from_chumsky(e.span());
            Spanned::new(
                AstExpr::Var {
                    name: name.node.name,
                    span,
                },
                span,
            )
        });

        choice((
            expr_call,
            expr_string,
            expr_capture_ref,
            expr_numeric_lit,
            expr_ident,
        ))
        .labelled("expression")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex_to_pairs;
    use crate::make_input;

    fn parse_expr(source: &str) -> AstExpr {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        expr().parse(input).into_result().unwrap().node
    }

    #[test]
    fn string_expression() {
        let e = parse_expr(r#""hello""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 1);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::Literal { value, .. } if value == "hello")
                );
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn string_with_interpolation() {
        let e = parse_expr(r#""hello ${name}""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 2);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::Literal { value, .. } if value == "hello ")
                );
                assert!(
                    matches!(&interp.parts[1], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn function_call_no_args() {
        let e = parse_expr("foo()");
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert!(call.args.is_empty());
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn function_call_with_args() {
        let e = parse_expr(r#"greet("hello", name)"#);
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "greet");
                assert_eq!(call.args.len(), 2);
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn nested_function_call() {
        let e = parse_expr("outer(inner())");
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "outer");
                assert_eq!(call.args.len(), 1);
                match &call.args[0].node {
                    AstExpr::Call { call: inner, .. } => assert_eq!(inner.name.node.name, "inner"),
                    other => panic!("expected inner Call, got {other:?}"),
                }
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn capture_ref_expr() {
        let e = parse_expr("$1");
        assert!(matches!(e, AstExpr::CaptureRef { index: 1, .. }));
    }

    #[test]
    fn numeric_literal_expr() {
        let e = parse_expr("42");
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 1);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::Literal { value, .. } if value == "42")
                );
            }
            _ => panic!("expected String (numeric), got {e:?}"),
        }
    }

    #[test]
    fn var_ref_expr() {
        let e = parse_expr("my_var");
        match e {
            AstExpr::Var { name, .. } => assert_eq!(name, "my_var"),
            _ => panic!("expected Var, got {e:?}"),
        }
    }

    #[test]
    fn plain_string_test_name() {
        let source = r#""my test name""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = plain_string().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node, "my test name");
    }

    #[test]
    fn empty_string_expression() {
        let e = parse_expr(r#""""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert!(interp.parts.is_empty());
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn function_call_trailing_comma() {
        let e = parse_expr(r#"foo("a",)"#);
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 1);
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn capture_ref_zero() {
        let e = parse_expr("$0");
        assert!(matches!(e, AstExpr::CaptureRef { index: 0, .. }));
    }

    #[test]
    fn empty_plain_string() {
        let source = r#""""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = plain_string().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node, "");
    }

    #[test]
    fn string_with_escape_sequence() {
        let e = parse_expr(r#""hello\nworld""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 1);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::Literal { value, .. } if value == "hello\nworld")
                );
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn function_call_mixed_arg_types() {
        let e = parse_expr(r#"foo("str", var, 42, $1)"#);
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 4);
                assert!(matches!(call.args[0].node, AstExpr::String { .. }));
                assert!(matches!(call.args[1].node, AstExpr::Var { .. }));
                assert!(matches!(call.args[2].node, AstExpr::String { .. })); // numeric → String
                assert!(matches!(call.args[3].node, AstExpr::CaptureRef { .. }));
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn deeply_nested_calls() {
        let e = parse_expr("a(b(c()))");
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "a");
                match &call.args[0].node {
                    AstExpr::Call { call: b, .. } => {
                        assert_eq!(b.name.node.name, "b");
                        match &b.args[0].node {
                            AstExpr::Call { call: c, .. } => assert_eq!(c.name.node.name, "c"),
                            other => panic!("expected Call c, got {other:?}"),
                        }
                    }
                    other => panic!("expected Call b, got {other:?}"),
                }
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn uppercase_var_ref() {
        let e = parse_expr("MY_VAR");
        match e {
            AstExpr::Var { name, .. } => assert_eq!(name, "MY_VAR"),
            _ => panic!("expected Var, got {e:?}"),
        }
    }

    #[test]
    fn function_call_single_arg() {
        let e = parse_expr("foo(x)");
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 1);
                assert!(matches!(call.args[0].node, AstExpr::Var { .. }));
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn underscore_var_ref() {
        let e = parse_expr("_private");
        match e {
            AstExpr::Var { name, .. } => assert_eq!(name, "_private"),
            _ => panic!("expected Var, got {e:?}"),
        }
    }

    #[test]
    fn plain_string_with_special_chars() {
        let source = r#""test: {hello} (world)""#;
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = plain_string().parse(input).into_result();
        assert!(result.is_ok());
        let s = result.unwrap().node;
        assert!(s.contains('{'));
        assert!(s.contains('}'));
        assert!(s.contains('('));
        assert!(s.contains(')'));
    }

    #[test]
    fn string_with_only_whitespace() {
        let e = parse_expr(r#""   ""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 1);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::Literal { value, .. } if value == "   ")
                );
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn string_with_only_var_ref() {
        let e = parse_expr(r#""${name}""#);
        match e {
            AstExpr::String { interp, .. } => {
                assert_eq!(interp.parts.len(), 1);
                assert!(
                    matches!(&interp.parts[0], AstStringPart::VarRef { name, .. } if name == "name")
                );
            }
            _ => panic!("expected String, got {e:?}"),
        }
    }

    #[test]
    fn function_call_with_nested_string_arg() {
        let e = parse_expr(r#"foo("hello ${name}")"#);
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 1);
                assert!(matches!(call.args[0].node, AstExpr::String { .. }));
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }

    #[test]
    fn multi_digit_capture_ref() {
        let e = parse_expr("$10");
        assert!(matches!(e, AstExpr::CaptureRef { index: 10, .. }));
    }

    #[test]
    fn function_call_spaces_around_comma() {
        let e = parse_expr(r#"foo("a" , "b")"#);
        match e {
            AstExpr::Call { call, .. } => {
                assert_eq!(call.name.node.name, "foo");
                assert_eq!(call.args.len(), 2);
            }
            _ => panic!("expected Call, got {e:?}"),
        }
    }
}
