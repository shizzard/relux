use chumsky::prelude::*;

use crate::Spanned;
use crate::dsl::lexer::Token;

use super::ParserInput;
use super::token::text;
use super::ws::ws;

// ─── Validation Types ───────────────────────────────────────

fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c == '_' || c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_camel_case(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_var_ident(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c == '_' || c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

// ─── Validated Identifier Combinators ───────────────────────

/// Variable name: starts with lowercase or `_`, alphanumeric + `_`.
pub fn ident_var<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    text()
        .try_map(|(s, span), _extra| {
            if is_var_ident(s) {
                Ok(Spanned::from((s.to_string(), span)))
            } else {
                Err(Rich::custom(
                    span,
                    format!("expected variable name, got `{s}`"),
                ))
            }
        })
        .labelled("variable name")
}

/// Function name: snake_case.
pub fn ident_fn<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    text()
        .try_map(|(s, span), _extra| {
            if is_snake_case(s) {
                Ok(Spanned::from((s.to_string(), span)))
            } else {
                Err(Rich::custom(
                    span,
                    format!("expected function name (snake_case), got `{s}`"),
                ))
            }
        })
        .labelled("function name")
}

/// Effect name: CamelCase.
pub fn ident_effect<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    text()
        .try_map(|(s, span), _extra| {
            if is_camel_case(s) {
                Ok(Spanned::from((s.to_string(), span)))
            } else {
                Err(Rich::custom(
                    span,
                    format!("expected effect name (CamelCase), got `{s}`"),
                ))
            }
        })
        .labelled("effect name")
}

/// Numeric literal (unquoted digits).
pub fn expr_numeric<'a>()
-> impl Parser<'a, ParserInput<'a>, Spanned<String>, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    text()
        .try_map(|(s, span), _extra| {
            if is_numeric(s) {
                Ok(Spanned::from((s.to_string(), span)))
            } else {
                Err(Rich::custom(
                    span,
                    format!("expected numeric literal, got `{s}`"),
                ))
            }
        })
        .labelled("numeric literal")
}

// ─── Aliased Name ───────────────────────────────────────────

pub struct AliasedName {
    pub name: Spanned<String>,
    pub alias: Option<Spanned<String>>,
}

/// `ident_fn() [as ident_fn()]`
pub fn ident_aliased_fn<'a>()
-> impl Parser<'a, ParserInput<'a>, AliasedName, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ident_fn()
        .then(
            ws().ignore_then(just(Token::As))
                .ignore_then(ws())
                .ignore_then(ident_fn())
                .or_not(),
        )
        .map(|(name, alias)| AliasedName { name, alias })
}

/// `ident_effect() [as ident_effect()]`
pub fn ident_aliased_effect<'a>()
-> impl Parser<'a, ParserInput<'a>, AliasedName, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ident_effect()
        .then(
            ws().ignore_then(just(Token::As))
                .ignore_then(ws())
                .ignore_then(ident_effect())
                .or_not(),
        )
        .map(|(name, alias)| AliasedName { name, alias })
}

/// `ident_effect() [as ident_var()]` — used in need declarations.
pub fn ident_aliased_effect_shell<'a>()
-> impl Parser<'a, ParserInput<'a>, AliasedName, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    ident_effect()
        .then(
            ws().ignore_then(just(Token::As))
                .ignore_then(ws())
                .ignore_then(ident_var())
                .or_not(),
        )
        .map(|(name, alias)| AliasedName { name, alias })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    #[test]
    fn valid_var_idents() {
        for name in ["x", "foo", "foo_bar", "_private", "x2"] {
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            let result = ident_var().parse(input).into_result();
            assert!(result.is_ok(), "expected `{name}` to be a valid var ident");
            assert_eq!(result.unwrap().node, name);
        }
    }

    #[test]
    fn invalid_var_idents() {
        {
            let name = "123";
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            assert!(
                ident_var().parse(input).into_result().is_err(),
                "expected `{name}` to be rejected as var ident"
            );
        }
    }

    #[test]
    fn valid_fn_idents() {
        for name in ["foo", "foo_bar", "match_prompt", "_helper"] {
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            let result = ident_fn().parse(input).into_result();
            assert!(result.is_ok(), "expected `{name}` to be a valid fn ident");
        }
    }

    #[test]
    fn invalid_fn_idents() {
        for name in ["Foo", "FooBar", "123abc"] {
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            assert!(
                ident_fn().parse(input).into_result().is_err(),
                "expected `{name}` to be rejected as fn ident"
            );
        }
    }

    #[test]
    fn valid_effect_idents() {
        for name in ["Db", "StartDb", "HttpServer", "A"] {
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            let result = ident_effect().parse(input).into_result();
            assert!(
                result.is_ok(),
                "expected `{name}` to be a valid effect ident"
            );
        }
    }

    #[test]
    fn invalid_effect_idents() {
        for name in ["db", "start_db", "_Foo"] {
            let pairs = lex_to_pairs(name);
            let input = make_input(&pairs, name.len());
            assert!(
                ident_effect().parse(input).into_result().is_err(),
                "expected `{name}` to be rejected as effect ident"
            );
        }
    }

    #[test]
    fn numeric_literal() {
        let source = "42";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = expr_numeric().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node, "42");
    }

    #[test]
    fn aliased_fn() {
        let source = "greet as hello";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_aliased_fn().parse(input).into_result();
        assert!(result.is_ok());
        let aliased = result.unwrap();
        assert_eq!(aliased.name.node, "greet");
        assert_eq!(aliased.alias.unwrap().node, "hello");
    }

    #[test]
    fn aliased_fn_no_alias() {
        let source = "greet";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_aliased_fn().parse(input).into_result();
        assert!(result.is_ok());
        let aliased = result.unwrap();
        assert_eq!(aliased.name.node, "greet");
        assert!(aliased.alias.is_none());
    }

    #[test]
    fn aliased_effect_shell() {
        let source = "Db as db";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_aliased_effect_shell().parse(input).into_result();
        assert!(result.is_ok());
        let aliased = result.unwrap();
        assert_eq!(aliased.name.node, "Db");
        assert_eq!(aliased.alias.unwrap().node, "db");
    }

    #[test]
    fn aliased_effect() {
        let source = "Db as Database";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_aliased_effect().parse(input).into_result();
        assert!(result.is_ok());
        let aliased = result.unwrap();
        assert_eq!(aliased.name.node, "Db");
        assert_eq!(aliased.alias.unwrap().node, "Database");
    }

    #[test]
    fn aliased_effect_no_alias() {
        let source = "Db";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_aliased_effect().parse(input).into_result();
        assert!(result.is_ok());
        let aliased = result.unwrap();
        assert_eq!(aliased.name.node, "Db");
        assert!(aliased.alias.is_none());
    }

    #[test]
    fn numeric_rejects_non_digits() {
        let source = "abc";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(expr_numeric().parse(input).into_result().is_err());
    }

    #[test]
    fn lone_underscore_var_ident() {
        let source = "_";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_var().parse(input).into_result();
        assert!(result.is_ok(), "expected `_` to be a valid var ident");
        assert_eq!(result.unwrap().node, "_");
    }

    #[test]
    fn aliased_fn_rejects_camel_case_alias() {
        let source = "greet as Hello";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ident_aliased_fn().parse(input).into_result().is_err());
    }

    #[test]
    fn aliased_effect_rejects_snake_case_alias() {
        let source = "Db as my_db";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        assert!(ident_aliased_effect().parse(input).into_result().is_err());
    }

    #[test]
    fn effect_ident_with_digits() {
        let source = "Http2Server";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_effect().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node, "Http2Server");
    }

    #[test]
    fn fn_ident_with_digits() {
        let source = "get_v2";
        let pairs = lex_to_pairs(source);
        let input = make_input(&pairs, source.len());
        let result = ident_fn().parse(input).into_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node, "get_v2");
    }
}
