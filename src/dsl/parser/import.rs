use chumsky::prelude::*;

use crate::Span;
use crate::Spanned;
use crate::dsl::lexer::Token;

use super::ParserInput;
use super::ast::AstImport;
use super::ast::AstImportName;
use super::ident::ident_aliased_effect;
use super::ident::ident_aliased_fn;
use super::punctuation::punctuation_brace_close;
use super::punctuation::punctuation_brace_open;
use super::token::text;
use super::ws::flex_ws;
use super::ws::newline;
use super::ws::ws;

// ─── L5: AstImport Combinators ────────────────────────────────

/// `path/to/module` — slash-separated path segments.
fn import_path<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    text()
        .then(
            just(Token::Slash)
                .ignore_then(text())
                .repeated()
                .collect::<Vec<_>>(),
        )
        .map_with(|((first, _), rest), e| {
            let mut path = first.to_string();
            for (seg, _) in rest {
                path.push('/');
                path.push_str(seg);
            }
            Spanned::new(path, Span::from(e.span()))
        })
        .labelled("import path")
}

/// `import path [{ names }]` — import declaration.
pub fn import<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<AstImport>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    let import_name = ident_aliased_effect()
        .or(ident_aliased_fn())
        .map_with(|a, e| {
            let span = Span::from(e.span());
            Spanned::new(
                AstImportName {
                    name: a.name,
                    alias: a.alias,
                    span,
                },
                span,
            )
        });

    let sep = select_ref! {
        Token::Space(_) => (),
        Token::Tab(_) => (),
        Token::Newline => (),
        Token::Comma => (),
    }
    .repeated()
    .at_least(1)
    .ignored();

    let selective = ws()
        .ignore_then(punctuation_brace_open())
        .ignore_then(flex_ws())
        .ignore_then(
            import_name
                .separated_by(sep)
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(flex_ws())
        .then_ignore(punctuation_brace_close());

    just(Token::Import)
        .ignore_then(ws())
        .ignore_then(import_path())
        .then(selective.or_not())
        .map_with(|(path, names), e| {
            let span = Span::from(e.span());
            Spanned::new(AstImport { path, names, span }, span)
        })
        .then_ignore(newline())
        .labelled("import declaration")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::lex_to_pairs;
    use crate::dsl::parser::make_input;

    fn parse_import(source: &str) -> AstImport {
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        import().parse(input).into_result().unwrap().node
    }

    #[test]
    fn wildcard_import() {
        let imp = parse_import("import lib/greeter\n");
        assert_eq!(imp.path.node, "lib/greeter");
        assert!(imp.names.is_none());
    }

    #[test]
    fn selective_functions() {
        let imp = parse_import("import lib/greeter { greet, hello }\n");
        assert_eq!(imp.path.node, "lib/greeter");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].node.name.node.name, "greet");
        assert_eq!(names[1].node.name.node.name, "hello");
    }

    #[test]
    fn mixed_effects_and_functions() {
        let imp = parse_import("import lib/greeter { Db, greet }\n");
        assert_eq!(imp.path.node, "lib/greeter");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].node.name.node.name, "Db");
        assert_eq!(names[1].node.name.node.name, "greet");
    }

    #[test]
    fn aliased_import() {
        let imp = parse_import("import lib/greeter { Db as Database }\n");
        let names = imp.names.unwrap();
        assert_eq!(names[0].node.name.node.name, "Db");
        assert_eq!(names[0].node.alias.as_ref().unwrap().node.name, "Database");
    }

    #[test]
    fn trailing_comma() {
        let imp = parse_import("import lib/greeter { greet, }\n");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].node.name.node.name, "greet");
    }

    #[test]
    fn deep_path() {
        let imp = parse_import("import lib/utils/greeter\n");
        assert_eq!(imp.path.node, "lib/utils/greeter");
    }

    #[test]
    fn multiline_selective_import() {
        let imp = parse_import(
            r#"import lib/greeter {
  greet
  hello
}
"#,
        );
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].node.name.node.name, "greet");
        assert_eq!(names[1].node.name.node.name, "hello");
    }

    #[test]
    fn single_segment_path() {
        let imp = parse_import("import utils\n");
        assert_eq!(imp.path.node, "utils");
        assert!(imp.names.is_none());
    }

    #[test]
    fn aliased_function_import() {
        let imp = parse_import("import lib/greeter { greet as hello }\n");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].node.name.node.name, "greet");
        assert_eq!(names[0].node.alias.as_ref().unwrap().node.name, "hello");
    }

    #[test]
    fn single_name_selective() {
        let imp = parse_import("import lib/greeter { greet }\n");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].node.name.node.name, "greet");
        assert!(names[0].node.alias.is_none());
    }

    #[test]
    fn deep_path_five_segments() {
        let imp = parse_import("import a/b/c/d/e\n");
        assert_eq!(imp.path.node, "a/b/c/d/e");
        assert!(imp.names.is_none());
    }

    #[test]
    fn selective_with_mixed_effects_and_fns() {
        let imp = parse_import("import lib/all { Db, greet, Cache, hello }\n");
        let names = imp.names.unwrap();
        assert_eq!(names.len(), 4);
        assert_eq!(names[0].node.name.node.name, "Db");
        assert_eq!(names[1].node.name.node.name, "greet");
        assert_eq!(names[2].node.name.node.name, "Cache");
        assert_eq!(names[3].node.name.node.name, "hello");
    }

    #[test]
    fn empty_selective_import() {
        let imp = parse_import("import lib/greeter {}\n");
        let names = imp.names.unwrap();
        assert!(names.is_empty());
    }
}
