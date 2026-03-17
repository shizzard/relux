use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::ParserInput;
use super::annotation::{comment, marker};
use super::ast::{AstFnDef, AstMarkerDecl, AstPureFnDef, AstStmt};
use super::ident::{ident_fn, ident_var};
use super::punctuation::{
    punctuation_brace_close, punctuation_brace_open, punctuation_comma, punctuation_paren_close,
    punctuation_paren_open,
};
use super::stmt::stmt;
use super::token::keyword;
use super::ws::{leading_ws, newline, ws};

/// Sentinel span for dummy blank-line comments.
const SENTINEL: Span = Span::new(0, 0);

// ─── Helpers ────────────────────────────────────────────────

/// Collects leading markers and comments (with interspersed blank lines).
/// Returns `(markers, comments)`.
fn preamble<'a>() -> impl Parser<
    'a,
    ParserInput<'a>,
    (Vec<Spanned<AstMarkerDecl>>, Vec<String>),
    extra::Err<Rich<'a, Token<'a>>>,
> + Clone {
    let marker_item = leading_ws()
        .ignore_then(marker())
        .map(|m| PreambleItem::Marker(Box::new(m)));
    let comment_item = leading_ws()
        .ignore_then(comment())
        .map(PreambleItem::Comment);
    let blank = newline().to(PreambleItem::Blank);

    choice((marker_item, comment_item, blank))
        .repeated()
        .collect::<Vec<_>>()
        .map(|items| {
            let mut markers = Vec::new();
            let mut comments = Vec::new();
            for item in items {
                match item {
                    PreambleItem::Marker(m) => markers.push(*m),
                    PreambleItem::Comment(c) => comments.push(c),
                    PreambleItem::Blank => {}
                }
            }
            (markers, comments)
        })
}

#[derive(Clone)]
enum PreambleItem {
    Marker(Box<Spanned<AstMarkerDecl>>),
    Comment(String),
    Blank,
}

/// Comma-separated parameter list between parens.
fn params<'a>()
-> impl Parser<'a, ParserInput<'a>, Vec<Spanned<String>>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    punctuation_paren_open()
        .ignore_then(ws())
        .ignore_then(
            ident_var()
                .separated_by(ws().then(punctuation_comma()).then(ws()))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(ws())
        .then_ignore(punctuation_paren_close())
}

/// Body: `{ [stmt | newline]* }`.
fn body<'a>()
-> impl Parser<'a, ParserInput<'a>, Vec<Spanned<AstStmt>>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    punctuation_brace_open()
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
        .map(|stmts| {
            stmts
                .into_iter()
                .filter(
                    |s| !matches!(&s.node, AstStmt::Comment { text, .. } if text.is_empty() && s.span == SENTINEL),
                )
                .collect()
        })
}

// ─── L6: Function Definition Combinators ────────────────────

/// `[preamble] fn name(params) { body }` — function definition.
pub fn def_fn<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstFnDef>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    preamble()
        .then_ignore(leading_ws())
        .then_ignore(keyword(Token::Fn))
        .then_ignore(ws())
        .then(ident_fn())
        .then(params())
        .then_ignore(ws())
        .then(body())
        .map_with(|((((_markers, _comments), name), params), body), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstFnDef {
                    name,
                    params,
                    markers: _markers,
                    body,
                    span,
                },
                span,
            )
        })
        .labelled("function definition")
}

/// `[preamble] pure fn name(params) { body }` — pure function definition.
pub fn def_pure_fn<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstPureFnDef>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    preamble()
        .then_ignore(leading_ws())
        .then_ignore(keyword(Token::Pure))
        .then_ignore(ws())
        .then_ignore(keyword(Token::Fn))
        .then_ignore(ws())
        .then(ident_fn())
        .then(params())
        .then_ignore(ws())
        .then(body())
        .map_with(|((((_markers, _comments), name), params), body), e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstPureFnDef {
                    name,
                    params,
                    markers: _markers,
                    body,
                    span,
                },
                span,
            )
        })
        .labelled("pure function definition")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::ast::AstMarkerKind;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    fn parse_fn(source: &str) -> AstFnDef {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        def_fn()
            .then_ignore(any().repeated())
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    fn parse_pure_fn(source: &str) -> AstPureFnDef {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        def_pure_fn()
            .then_ignore(any().repeated())
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    #[test]
    fn simple_fn() {
        let f = parse_fn(
            r#"fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert!(f.params.is_empty());
        assert_eq!(f.body.len(), 1);
    }

    #[test]
    fn fn_with_params() {
        let f = parse_fn(
            r#"fn greet(name, greeting) {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].node, "name");
        assert_eq!(f.params[1].node, "greeting");
    }

    #[test]
    fn fn_with_marker() {
        let f = parse_fn(
            r#"# skip
fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(f.markers.len(), 1);
        assert!(matches!(f.markers[0].node.kind, AstMarkerKind::Skip { .. }));
    }

    #[test]
    fn simple_pure_fn() {
        let f = parse_pure_fn(
            r#"pure fn concat(a, b) {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "concat");
        assert_eq!(f.params.len(), 2);
    }

    #[test]
    fn fn_zero_params() {
        let f = parse_fn(
            r#"fn noop() {
}
"#,
        );
        assert_eq!(f.name.node, "noop");
        assert!(f.params.is_empty());
        assert!(f.body.is_empty());
    }

    #[test]
    fn fn_with_multiple_stmts() {
        let f = parse_fn(
            r#"fn greet() {
  > echo hello
  <= hello
  > echo bye
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert_eq!(f.body.len(), 3);
    }

    #[test]
    fn fn_body_with_blank_lines() {
        let f = parse_fn(
            r#"fn greet() {
  > echo hello

  <= hello
}
"#,
        );
        assert_eq!(f.body.len(), 2);
    }

    #[test]
    fn pure_fn_with_marker() {
        let f = parse_pure_fn(
            r#"# skip
pure fn concat(a, b) {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "concat");
        assert_eq!(f.markers.len(), 1);
        assert!(matches!(f.markers[0].node.kind, AstMarkerKind::Skip { .. }));
    }

    #[test]
    fn pure_fn_empty_body() {
        let f = parse_pure_fn(
            r#"pure fn noop() {
}
"#,
        );
        assert_eq!(f.name.node, "noop");
        assert!(f.body.is_empty());
    }

    #[test]
    fn fn_with_comments_in_preamble() {
        let f = parse_fn(
            r#"// this is a helper
fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert!(f.markers.is_empty());
        assert_eq!(f.body.len(), 1);
    }

    #[test]
    fn fn_with_multiple_markers() {
        let f = parse_fn(
            r#"# skip
# flaky
fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(f.markers.len(), 2);
        assert!(matches!(f.markers[0].node.kind, AstMarkerKind::Skip { .. }));
        assert!(matches!(
            f.markers[1].node.kind,
            AstMarkerKind::Flaky { .. }
        ));
    }

    #[test]
    fn fn_with_single_param() {
        let f = parse_fn(
            r#"fn greet(name) {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].node, "name");
    }

    #[test]
    fn fn_with_trailing_comma_params() {
        let f = parse_fn(
            r#"fn greet(name, greeting,) {
  > echo hello
}
"#,
        );
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].node, "name");
        assert_eq!(f.params[1].node, "greeting");
    }

    #[test]
    fn fn_with_marker_and_comment_preamble() {
        let f = parse_fn(
            r#"// helper function
# skip
fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(f.name.node, "greet");
        assert_eq!(f.markers.len(), 1);
        assert!(matches!(f.markers[0].node.kind, AstMarkerKind::Skip { .. }));
    }

    #[test]
    fn fn_params_spaces_around_comma() {
        let f = parse_fn(
            r#"fn greet( name , greeting ) {
  > echo hello
}
"#,
        );
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].node, "name");
        assert_eq!(f.params[1].node, "greeting");
    }
}
