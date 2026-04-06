use chumsky::prelude::*;

use crate::Span;
use crate::Spanned;
use crate::dsl::lexer::Token;

use super::AstItem;
use super::AstModule;
use super::ParserInput;
use super::annotation::comment;
use super::effect::def_effect;
use super::fn_def::def_fn;
use super::fn_def::def_pure_fn;
use super::import::import;
use super::test_def::def_test;
use super::ws::leading_ws;
use super::ws::newline;

/// Sentinel span for dummy blank-line items.
const SENTINEL: Span = Span::new(0, 0);

// ─── L7: Module Combinators ────────────────────────────────

/// A single module-level item.
fn module_item<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstItem>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    leading_ws().ignore_then(
        choice((
            import().map(|i| {
                let span = i.span;
                Spanned::new(
                    AstItem::Import {
                        import: i.node,
                        span,
                    },
                    span,
                )
            }),
            def_pure_fn().map(|f| {
                let span = f.span;
                Spanned::new(AstItem::PureFn { def: f.node, span }, span)
            }),
            def_fn().map(|f| {
                let span = f.span;
                Spanned::new(AstItem::Fn { def: f.node, span }, span)
            }),
            def_effect().map(|e| {
                let span = e.span;
                Spanned::new(AstItem::Effect { def: e.node, span }, span)
            }),
            def_test().map(|t| {
                let span = t.span;
                Spanned::new(AstItem::Test { def: t.node, span }, span)
            }),
            comment().map_with(|c, e| {
                let span = Span::from(e.span());
                Spanned::new(AstItem::Comment { text: c, span }, span)
            }),
        ))
        .labelled("top-level item (import, fn, effect, test, or comment)"),
    )
}

/// Full module: zero or more items with interspersed blank lines.
pub fn module<'a>()
-> impl Parser<'a, ParserInput<'a>, AstModule, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    module_item()
        // Fragile: SENTINEL comment must be filtered below — edit with caution.
        .or(newline().map_with(|_, _| {
            Spanned::new(
                AstItem::Comment { text: String::new(), span: SENTINEL },
                SENTINEL,
            )
        }))
        .repeated()
        .collect::<Vec<_>>()
        .map_with(|items, e| {
            let items = items
                .into_iter()
                .filter(
                    |i| !matches!(&i.node, AstItem::Comment { text, .. } if text.is_empty() && i.span == SENTINEL),
                )
                .collect();
            AstModule { items, span: Span::from(e.span()) }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    fn parse_module(source: &str) -> AstModule {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        module().parse(input).into_result().unwrap()
    }

    #[test]
    fn empty_module() {
        let m = parse_module("");
        assert!(m.items.is_empty());
    }

    #[test]
    fn module_with_import() {
        let m = parse_module("import lib/greeter\n");
        assert_eq!(m.items.len(), 1);
        assert!(matches!(&m.items[0].node, AstItem::Import { .. }));
    }

    #[test]
    fn module_with_fn() {
        let m = parse_module(
            r#"fn greet() {
  > echo hello
}
"#,
        );
        assert_eq!(m.items.len(), 1);
        assert!(matches!(&m.items[0].node, AstItem::Fn { .. }));
    }

    #[test]
    fn module_with_comment() {
        let m = parse_module("// this is a comment\n");
        assert_eq!(m.items.len(), 1);
        assert!(
            matches!(&m.items[0].node, AstItem::Comment { text, .. } if text == "this is a comment")
        );
    }

    #[test]
    fn module_with_blank_lines() {
        let m = parse_module(
            r#"// comment

// another
"#,
        );
        assert_eq!(m.items.len(), 2);
    }

    #[test]
    fn multi_item_module() {
        let source = r#"import lib/greeter

fn greet() {
  > echo hello
}

test "basic" {
  shell main {
    > echo hi
  }
}
"#;
        let m = parse_module(source);
        assert_eq!(m.items.len(), 3);
        assert!(matches!(&m.items[0].node, AstItem::Import { .. }));
        assert!(matches!(&m.items[1].node, AstItem::Fn { .. }));
        assert!(matches!(&m.items[2].node, AstItem::Test { .. }));
    }

    #[test]
    fn module_with_effect() {
        let source = r#"effect Db {
  shell db {
    > echo start
  }
}
"#;
        let m = parse_module(source);
        assert_eq!(m.items.len(), 1);
        assert!(matches!(&m.items[0].node, AstItem::Effect { .. }));
    }

    #[test]
    fn module_with_pure_fn() {
        let source = r#"pure fn concat(a, b) {
  > echo hello
}
"#;
        let m = parse_module(source);
        assert_eq!(m.items.len(), 1);
        assert!(matches!(&m.items[0].node, AstItem::PureFn { .. }));
    }

    #[test]
    fn module_only_blank_lines() {
        let m = parse_module("\n\n\n");
        assert!(m.items.is_empty());
    }

    #[test]
    fn module_all_item_types() {
        let source = r#"import lib/greeter

// a comment

fn greet() {
  > echo hello
}

pure fn concat(a, b) {
  > echo hello
}

effect Db {
  shell db {
    > echo start
  }
}

test "basic" {
  shell main {
    > echo hi
  }
}
"#;
        let m = parse_module(source);
        // Comment may be absorbed by fn preamble, so check by type presence
        assert!(
            m.items
                .iter()
                .any(|i| matches!(&i.node, AstItem::Import { .. }))
        );
        assert!(
            m.items
                .iter()
                .any(|i| matches!(&i.node, AstItem::Fn { .. }))
        );
        assert!(
            m.items
                .iter()
                .any(|i| matches!(&i.node, AstItem::PureFn { .. }))
        );
        assert!(
            m.items
                .iter()
                .any(|i| matches!(&i.node, AstItem::Effect { .. }))
        );
        assert!(
            m.items
                .iter()
                .any(|i| matches!(&i.node, AstItem::Test { .. }))
        );
    }

    #[test]
    fn public_parse_api_success() {
        let source = r#"test "basic" {
  shell main {
    > echo hi
  }
}
"#;
        let result = crate::dsl::parser::parse(source);
        assert!(result.is_ok());
        let m = result.unwrap();
        assert_eq!(m.items.len(), 1);
        assert!(matches!(&m.items[0].node, AstItem::Test { .. }));
    }

    #[test]
    fn public_parse_api_error() {
        let source = "this is not valid relux\n";
        let result = crate::dsl::parser::parse(source);
        assert!(result.is_err());
    }

    #[test]
    fn module_with_only_comments() {
        let m = parse_module(
            r#"// first comment
// second comment
"#,
        );
        assert_eq!(m.items.len(), 2);
        assert!(
            m.items
                .iter()
                .all(|i| matches!(&i.node, AstItem::Comment { .. }))
        );
    }

    #[test]
    fn module_multiple_tests() {
        let source = r#"test "first" {
  shell main {
    > echo a
  }
}

test "second" {
  shell main {
    > echo b
  }
}
"#;
        let m = parse_module(source);
        let test_count = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, AstItem::Test { .. }))
            .count();
        assert_eq!(test_count, 2);
    }
}
