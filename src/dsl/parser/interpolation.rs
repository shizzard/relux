use chumsky::prelude::*;

use crate::dsl::lexer::Token;
use crate::{Span, Spanned};

use super::ParserInput;
use super::ast::{AstInterpolation, AstStringPart};
use super::ident::{expr_numeric, ident_var};

// ─── Escape Table ───────────────────────────────────────────

fn interpret_escape(ch: &str) -> Option<char> {
    match ch {
        "n" => Some('\n'),
        "t" => Some('\t'),
        "r" => Some('\r'),
        "\\" => Some('\\'),
        "\"" => Some('"'),
        "0" => Some('\0'),
        "a" => Some('\x07'),
        "b" => Some('\x08'),
        "f" => Some('\x0C'),
        "v" => Some('\x0B'),
        "e" => Some('\x1B'),
        _ => None,
    }
}

// ─── L2: Primitive Interpolation Combinators ────────────────

/// `$$` → `AstStringPart::EscapedDollar`
fn interp_escaped_dollar<'a>()
-> impl Parser<'a, ParserInput<'a>, AstStringPart, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Dollar)
        .then(just(Token::Dollar))
        .map_with(|_, e| AstStringPart::EscapedDollar {
            span: Span::from(e.span()),
        })
}

/// `${var_name}` → `AstStringPart::VarRef(name)`
fn interp_var_ref<'a>()
-> impl Parser<'a, ParserInput<'a>, AstStringPart, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Dollar)
        .ignore_then(just(Token::BraceOpen))
        .ignore_then(ident_var())
        .then_ignore(just(Token::BraceClose))
        .map_with(|name, e| AstStringPart::VarRef {
            name: name.node,
            span: Span::from(e.span()),
        })
}

/// `${1}` → `AstStringPart::CaptureRef(index)`
fn interp_capture_ref<'a>()
-> impl Parser<'a, ParserInput<'a>, AstStringPart, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    just(Token::Dollar)
        .ignore_then(just(Token::BraceOpen))
        .ignore_then(expr_numeric())
        .then_ignore(just(Token::BraceClose))
        .map_with(|num, e| AstStringPart::CaptureRef {
            index: num.node.parse::<usize>().unwrap(),
            span: Span::from(e.span()),
        })
}

/// `\n`, `\t`, etc. → interpreted escape character. Invalid escapes emit an error
/// but still consume the token (preventing backtrack to catch-all).
fn interp_escape_seq<'a>()
-> impl Parser<'a, ParserInput<'a>, AstStringPart, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    select_ref! { Token::Escape(s) => *s }
        .map_with(|s, e| (s, e.span()))
        .validate(
            |(s, span): (&str, SimpleSpan), _extra, emitter| match interpret_escape(s) {
                Some(ch) => AstStringPart::Literal {
                    value: ch.to_string(),
                    span: Span::from(span),
                },
                None => {
                    emitter.emit(Rich::custom(
                        span,
                        format!("unknown escape sequence `\\{s}`"),
                    ));
                    AstStringPart::Literal {
                        value: format!("\\{s}"),
                        span: Span::from(span),
                    }
                }
            },
        )
}

/// In regex contexts, escape sequences pass through verbatim as `\x`.
fn interp_raw_escape_seq<'a>()
-> impl Parser<'a, ParserInput<'a>, AstStringPart, extra::Err<Rich<'a, Token<'a>>>> + Clone {
    select_ref! { Token::Escape(s) => *s }.map_with(|s, e| {
        let mut lit = String::from('\\');
        lit.push_str(s);
        AstStringPart::Literal {
            value: lit,
            span: Span::from(e.span()),
        }
    })
}

// ─── L3: Interpolation Context Combinators ──────────────────

/// Helper: coalesce adjacent Literal parts.
fn coalesce_parts(parts: Vec<AstStringPart>) -> Vec<AstStringPart> {
    let mut out: Vec<AstStringPart> = Vec::new();
    for part in parts {
        if let AstStringPart::Literal { value: ref s, span } = part
            && let Some(AstStringPart::Literal {
                value: prev,
                span: prev_span,
            }) = out.last_mut()
        {
            prev.push_str(s);
            *prev_span = prev_span.merge(span);
            continue;
        }
        out.push(part);
    }
    out
}

/// Collect interpolation parts until `terminator`, interpreting escape sequences.
/// Used for send payloads, match-literal payloads, and string expressions.
pub fn interp_literal<'a>(
    terminator: Token<'a>,
) -> impl Parser<'a, ParserInput<'a>, Spanned<AstInterpolation>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    let catch_all = none_of(terminator).map_with(|tok: Token<'a>, e| {
        let s = tok.to_string();
        AstStringPart::Literal {
            value: s,
            span: Span::from(e.span()),
        }
    });

    choice((
        interp_escaped_dollar(),
        interp_var_ref(),
        interp_capture_ref(),
        interp_escape_seq(),
        catch_all,
    ))
    .repeated()
    .collect::<Vec<_>>()
    .map_with(|parts, e| {
        let span = Span::from(e.span());
        Spanned::new(
            AstInterpolation {
                parts: coalesce_parts(parts),
                span,
            },
            span,
        )
    })
    .labelled("interpolated text")
}

/// Collect interpolation parts until `terminator`, passing escape sequences through raw.
/// Used for match-regex payloads and marker regex patterns.
pub fn interp_regex<'a>(
    terminator: Token<'a>,
) -> impl Parser<'a, ParserInput<'a>, Spanned<AstInterpolation>, extra::Err<Rich<'a, Token<'a>>>> + Clone
{
    let catch_all = none_of(terminator).map_with(|tok: Token<'a>, e| {
        let s = tok.to_string();
        AstStringPart::Literal {
            value: s,
            span: Span::from(e.span()),
        }
    });

    choice((
        interp_escaped_dollar(),
        interp_var_ref(),
        interp_capture_ref(),
        interp_raw_escape_seq(),
        catch_all,
    ))
    .repeated()
    .collect::<Vec<_>>()
    .map_with(|parts, e| {
        let span = Span::from(e.span());
        Spanned::new(
            AstInterpolation {
                parts: coalesce_parts(parts),
                span,
            },
            span,
        )
    })
    .labelled("regex pattern")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::{lex_to_pairs, make_input};

    fn parse_literal(source: &str) -> AstInterpolation {
        // Simulate a payload terminated by newline
        let full = format!("{source}\n");
        let pairs = lex_to_pairs(&full);
        let input = make_input(&pairs, full.len());
        interp_literal(Token::Newline)
            .then_ignore(just(Token::Newline))
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    fn parse_regex(source: &str) -> AstInterpolation {
        let full = format!("{source}\n");
        let pairs = lex_to_pairs(&full);
        let input = make_input(&pairs, full.len());
        interp_regex(Token::Newline)
            .then_ignore(just(Token::Newline))
            .parse(input)
            .into_result()
            .unwrap()
            .node
    }

    /// Compare only the structural content of parts, ignoring spans.
    fn part_values(parts: &[AstStringPart]) -> Vec<&str> {
        parts
            .iter()
            .map(|p| match p {
                AstStringPart::Literal { value, .. } => value.as_str(),
                AstStringPart::VarRef { name, .. } => name.as_str(),
                AstStringPart::EscapedDollar { .. } => "$$",
                AstStringPart::CaptureRef { .. } => "$N",
            })
            .collect()
    }

    fn is_literal(p: &AstStringPart) -> bool {
        matches!(p, AstStringPart::Literal { .. })
    }

    fn is_var_ref(p: &AstStringPart, expected_name: &str) -> bool {
        matches!(p, AstStringPart::VarRef { name, .. } if name == expected_name)
    }

    fn is_capture_ref(p: &AstStringPart, expected_idx: usize) -> bool {
        matches!(p, AstStringPart::CaptureRef { index, .. } if *index == expected_idx)
    }

    #[test]
    fn plain_text() {
        let interp = parse_literal("hello world");
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec!["hello world"]);
    }

    #[test]
    fn var_ref() {
        let interp = parse_literal("hello ${name}");
        assert_eq!(interp.parts.len(), 2);
        assert!(is_literal(&interp.parts[0]));
        assert!(is_var_ref(&interp.parts[1], "name"));
    }

    #[test]
    fn capture_ref() {
        let interp = parse_literal("got ${1}");
        assert_eq!(interp.parts.len(), 2);
        assert!(is_literal(&interp.parts[0]));
        assert!(is_capture_ref(&interp.parts[1], 1));
    }

    #[test]
    fn escaped_dollar() {
        let interp = parse_literal("price $$5");
        assert_eq!(interp.parts.len(), 3);
        assert!(is_literal(&interp.parts[0]));
        assert!(matches!(
            interp.parts[1],
            AstStringPart::EscapedDollar { .. }
        ));
        assert!(is_literal(&interp.parts[2]));
    }

    #[test]
    fn escape_sequences() {
        let interp = parse_literal(r#"a\nb"#);
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec!["a\nb"]);
    }

    #[test]
    fn invalid_escape() {
        let full = "\\z\n";
        let pairs = lex_to_pairs(full);
        let input = make_input(&pairs, full.len());
        let result = interp_literal(Token::Newline)
            .then_ignore(just(Token::Newline))
            .parse(input)
            .into_result();
        assert!(result.is_err());
    }

    #[test]
    fn regex_passes_escapes_through() {
        let interp = parse_regex(r#"\d+\.\d+"#);
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec![r#"\d+\.\d+"#]);
    }

    #[test]
    fn literal_coalesces_adjacent() {
        // "a" + Space + "b" should coalesce to "a b"
        let interp = parse_literal("a b");
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec!["a b"]);
    }

    #[test]
    fn regex_with_var_ref() {
        let interp = parse_regex("${name}.*");
        assert_eq!(interp.parts.len(), 2);
        assert!(is_var_ref(&interp.parts[0], "name"));
        assert!(is_literal(&interp.parts[1]));
    }

    #[test]
    fn regex_with_escaped_dollar() {
        let interp = parse_regex("$$100");
        assert_eq!(interp.parts.len(), 2);
        assert!(matches!(
            interp.parts[0],
            AstStringPart::EscapedDollar { .. }
        ));
        assert!(is_literal(&interp.parts[1]));
    }

    #[test]
    fn empty_literal() {
        let interp = parse_literal("");
        assert!(interp.parts.is_empty());
    }

    #[test]
    fn empty_regex() {
        let interp = parse_regex("");
        assert!(interp.parts.is_empty());
    }

    #[test]
    fn multiple_var_refs_not_coalesced() {
        let interp = parse_literal("${a} and ${b}");
        // Should be: VarRef("a"), Literal(" and "), VarRef("b")
        assert_eq!(interp.parts.len(), 3);
        assert!(is_var_ref(&interp.parts[0], "a"));
        assert!(is_literal(&interp.parts[1]));
        assert!(is_var_ref(&interp.parts[2], "b"));
    }

    #[test]
    fn all_valid_escape_sequences() {
        // \n \t \r \\ \" \0 \a \b \f \v \e
        let interp = parse_literal(r#"\n\t\r\\\"\0\a\b\f\v\e"#);
        assert_eq!(interp.parts.len(), 1);
        let val = part_values(&interp.parts)[0];
        assert!(val.contains('\n'));
        assert!(val.contains('\t'));
        assert!(val.contains('\r'));
        assert!(val.contains('\\'));
        assert!(val.contains('"'));
        assert!(val.contains('\0'));
        assert!(val.contains('\x07')); // \a
        assert!(val.contains('\x08')); // \b
        assert!(val.contains('\x0C')); // \f
        assert!(val.contains('\x0B')); // \v
        assert!(val.contains('\x1B')); // \e
    }

    #[test]
    fn regex_with_capture_ref() {
        let interp = parse_regex("(${1}).*");
        assert!(interp.parts.len() >= 3);
        assert!(is_capture_ref(&interp.parts[1], 1));
    }

    #[test]
    fn literal_with_special_tokens() {
        // Braces, parens, operators should all pass through as literal text
        let interp = parse_literal("hello { world } (test) < > = !");
        assert_eq!(interp.parts.len(), 1);
        let val = part_values(&interp.parts)[0];
        assert!(val.contains('{'));
        assert!(val.contains('}'));
        assert!(val.contains('('));
        assert!(val.contains(')'));
    }

    #[test]
    fn mixed_refs_and_text() {
        let interp = parse_literal("start ${a} mid ${1} end");
        assert_eq!(interp.parts.len(), 5);
        assert!(is_literal(&interp.parts[0]));
        assert!(is_var_ref(&interp.parts[1], "a"));
        assert!(is_literal(&interp.parts[2]));
        assert!(is_capture_ref(&interp.parts[3], 1));
        assert!(is_literal(&interp.parts[4]));
    }

    #[test]
    fn regex_invalid_escape_passes_through() {
        // In regex context, \z should pass through verbatim (unlike literal which errors)
        let interp = parse_regex(r"\z");
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec![r"\z"]);
    }

    #[test]
    fn lone_dollar_in_literal() {
        // A bare `$` not followed by `$` or `{` falls into the catch-all
        let interp = parse_literal("cost $5");
        assert_eq!(interp.parts.len(), 1);
        let val = part_values(&interp.parts)[0];
        assert!(val.contains('$'));
        assert!(val.contains('5'));
    }

    #[test]
    fn only_escaped_dollar() {
        let interp = parse_literal("$$");
        assert_eq!(interp.parts.len(), 1);
        assert!(matches!(
            interp.parts[0],
            AstStringPart::EscapedDollar { .. }
        ));
    }

    #[test]
    fn capture_ref_zero_in_literal() {
        let interp = parse_literal("${0}");
        assert_eq!(interp.parts.len(), 1);
        assert!(is_capture_ref(&interp.parts[0], 0));
    }

    #[test]
    fn consecutive_var_refs() {
        let interp = parse_literal("${a}${b}");
        assert_eq!(interp.parts.len(), 2);
        assert!(is_var_ref(&interp.parts[0], "a"));
        assert!(is_var_ref(&interp.parts[1], "b"));
    }

    #[test]
    fn only_var_ref() {
        let interp = parse_literal("${name}");
        assert_eq!(interp.parts.len(), 1);
        assert!(is_var_ref(&interp.parts[0], "name"));
    }

    #[test]
    fn regex_consecutive_escapes() {
        let interp = parse_regex(r"\d+\s+\w+");
        assert_eq!(interp.parts.len(), 1);
        assert_eq!(part_values(&interp.parts), vec![r"\d+\s+\w+"]);
    }
}
