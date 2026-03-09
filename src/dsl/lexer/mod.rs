pub mod tokens;

use logos::Logos;
pub use tokens::{
    MarkerCondBody, MarkerCondition, MarkerExpr, MarkerKind, MarkerModifier, MarkerToken,
    PayloadFragment, Spanned, StringFragment, Token,
};

// ─── Internal Lexer Modes ───────────────────────────────────

#[derive(Logos, Debug, PartialEq, Clone)]
enum DocStringMode<'a> {
    #[token("\"\"\"")]
    Close,

    #[regex(r#"[^"]+"#)]
    Text(&'a str),

    #[regex(r#""{1,2}"#)]
    QuotedText(&'a str),
}

#[derive(Logos, Debug, PartialEq, Clone)]
enum RegexInterpolationMode<'a> {
    #[regex(r"\$\{[a-zA-Z_0-9]+\}", |lex| {
        let s = lex.slice();
        &s[2..s.len()-1]
    })]
    Interpolation(&'a str),

    #[token("$$", priority = 5)]
    EscapedDollar,

    #[regex(r"[^\n$]+")]
    Text(&'a str),

    #[token("$\n", priority = 3)]
    TrailingDollar,

    #[token("\n")]
    Newline,

    #[token("$")]
    BareDollar,
}

#[derive(Logos, Debug, PartialEq, Clone)]
enum StringMode<'a> {
    #[regex(r"\$\{[a-zA-Z_0-9]+\}", |lex| {
        let s = lex.slice();
        &s[2..s.len()-1]
    })]
    Interpolation(&'a str),

    #[token("$$")]
    EscapedDollar,

    #[regex(r"\\.", priority = 5)]
    Escape(&'a str),

    #[regex(r#"[^"\\$\n]+"#)]
    Text(&'a str),

    #[regex(r"\$[^{\n$]")]
    LiteralDollar(&'a str),

    #[token("\"")]
    Close,
}

// ─── Morph Callbacks ────────────────────────────────────────

fn lex_payload<'s>(lex: &mut logos::Lexer<'s, Token<'s>>) -> Option<Vec<PayloadFragment<'s>>> {
    let mut sub = lex.clone().morph::<RegexInterpolationMode<'s>>();
    let mut fragments = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(RegexInterpolationMode::Interpolation(s)) => {
                fragments.push(PayloadFragment::Interpolation(s));
            }
            Ok(RegexInterpolationMode::EscapedDollar) => {
                fragments.push(PayloadFragment::EscapedDollar);
            }
            Ok(RegexInterpolationMode::Text(s)) => {
                fragments.push(PayloadFragment::Text(s));
            }
            Ok(RegexInterpolationMode::BareDollar) => {
                fragments.push(PayloadFragment::Text("$"));
            }
            Ok(RegexInterpolationMode::TrailingDollar) => {
                fragments.push(PayloadFragment::Text("$"));
                break;
            }
            Ok(RegexInterpolationMode::Newline) => break,
            Err(_) => continue,
        }
    }
    *lex = sub.morph();
    Some(fragments)
}

fn lex_string<'s>(lex: &mut logos::Lexer<'s, Token<'s>>) -> Option<Vec<StringFragment<'s>>> {
    let mut sub = lex.clone().morph::<StringMode<'s>>();
    let mut fragments = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(StringMode::Interpolation(s)) => {
                fragments.push(StringFragment::Interpolation(s));
            }
            Ok(StringMode::EscapedDollar) => {
                fragments.push(StringFragment::Text("$"));
            }
            Ok(StringMode::Escape(s)) => {
                fragments.push(StringFragment::Escape(s));
            }
            Ok(StringMode::Text(s)) => {
                fragments.push(StringFragment::Text(s));
            }
            Ok(StringMode::LiteralDollar(s)) => {
                fragments.push(StringFragment::Text(s));
            }
            Ok(StringMode::Close) => break,
            Err(_) => continue,
        }
    }
    *lex = sub.morph();
    Some(fragments)
}

fn lex_timed_match_regex<'s>(
    lex: &mut logos::Lexer<'s, Token<'s>>,
) -> Option<(&'s str, Vec<PayloadFragment<'s>>)> {
    let matched = lex.slice();
    let dur = &matched[2..matched.len() - 1];
    let payload = lex_payload(lex)?;
    Some((dur, payload))
}

fn lex_timed_match_literal<'s>(
    lex: &mut logos::Lexer<'s, Token<'s>>,
) -> Option<(&'s str, Vec<PayloadFragment<'s>>)> {
    let matched = lex.slice();
    let dur = &matched[2..matched.len() - 1];
    let payload = lex_payload(lex)?;
    Some((dur, payload))
}

fn lex_timed_neg_match_regex<'s>(
    lex: &mut logos::Lexer<'s, Token<'s>>,
) -> Option<(&'s str, Vec<PayloadFragment<'s>>)> {
    let matched = lex.slice();
    let dur = &matched[2..matched.len() - 2];
    let payload = lex_payload(lex)?;
    Some((dur, payload))
}

fn lex_timed_neg_match_literal<'s>(
    lex: &mut logos::Lexer<'s, Token<'s>>,
) -> Option<(&'s str, Vec<PayloadFragment<'s>>)> {
    let matched = lex.slice();
    let dur = &matched[2..matched.len() - 2];
    let payload = lex_payload(lex)?;
    Some((dur, payload))
}

fn lex_docstring<'s>(lex: &mut logos::Lexer<'s, Token<'s>>) -> Option<Vec<&'s str>> {
    let mut sub = lex.clone().morph::<DocStringMode<'s>>();
    let mut parts = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(DocStringMode::Text(s)) | Ok(DocStringMode::QuotedText(s)) => {
                parts.push(s);
            }
            Ok(DocStringMode::Close) => break,
            Err(_) => continue,
        }
    }
    *lex = sub.morph();
    Some(parts)
}

fn lex_marker_string<'a>(content: &'a str) -> Option<(Vec<StringFragment<'a>>, &'a str)> {
    if !content.starts_with('"') {
        return None;
    }
    let src = &content[1..];
    let mut sub = StringMode::lexer(src);
    let mut fragments = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(StringMode::Interpolation(s)) => {
                fragments.push(StringFragment::Interpolation(s));
            }
            Ok(StringMode::EscapedDollar) => {
                fragments.push(StringFragment::Text("$"));
            }
            Ok(StringMode::Escape(s)) => {
                fragments.push(StringFragment::Escape(s));
            }
            Ok(StringMode::Text(s)) => {
                fragments.push(StringFragment::Text(s));
            }
            Ok(StringMode::LiteralDollar(s)) => {
                fragments.push(StringFragment::Text(s));
            }
            Ok(StringMode::Close) => {
                let consumed = src.len() - sub.remainder().len();
                return Some((fragments, &content[1 + consumed..]));
            }
            Err(_) => continue,
        }
    }
    None // unterminated string
}

fn lex_marker_expr<'a>(content: &'a str) -> Option<(MarkerExpr<'a>, &'a str)> {
    let content = content.trim_start();
    if content.is_empty() {
        return None;
    }
    if content.starts_with('"') {
        let (fragments, rest) = lex_marker_string(content)?;
        Some((MarkerExpr::String(fragments), rest))
    } else if content.as_bytes()[0].is_ascii_digit() {
        let end = content
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(content.len());
        if end == 0 {
            return None;
        }
        Some((MarkerExpr::Number(&content[..end]), &content[end..]))
    } else {
        None
    }
}

/// Lex regex content with `${var}` and `$$` interpolation.
///
/// Unlike `PayloadMode` (which treats every `$` as an interpolation prefix),
/// this only splits on `${…}` and `$$`. A bare `$` (regex anchor) stays in
/// the text fragment — the correct behavior for regex patterns.
fn lex_marker_regex<'a>(content: &'a str) -> Vec<PayloadFragment<'a>> {
    let mut sub = RegexInterpolationMode::lexer(content);
    let mut fragments = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(RegexInterpolationMode::Interpolation(s)) => {
                fragments.push(PayloadFragment::Interpolation(s));
            }
            Ok(RegexInterpolationMode::EscapedDollar) => {
                fragments.push(PayloadFragment::EscapedDollar);
            }
            Ok(RegexInterpolationMode::Text(s)) => {
                fragments.push(PayloadFragment::Text(s));
            }
            Ok(RegexInterpolationMode::BareDollar) => {
                fragments.push(PayloadFragment::Text("$"));
            }
            Ok(RegexInterpolationMode::TrailingDollar | RegexInterpolationMode::Newline) => break,
            Err(_) => continue,
        }
    }
    fragments
}

fn lex_marker<'s>(lex: &mut logos::Lexer<'s, Token<'s>>) -> Option<MarkerToken<'s>> {
    let matched = lex.slice();
    let inner = matched[1..matched.len() - 1].trim();

    let space1 = inner.find(|c: char| c.is_whitespace());
    let kind_str = match space1 {
        Some(pos) => &inner[..pos],
        None => inner,
    };
    let kind = match kind_str {
        "skip" => MarkerKind::Skip,
        "run" => MarkerKind::Run,
        "flaky" => MarkerKind::Flaky,
        _ => return None,
    };

    // Bare marker: [skip], [flaky], [run]
    let Some(space1) = space1 else {
        return Some(MarkerToken {
            kind,
            condition: None,
        });
    };
    let rest = inner[space1..].trim_start();
    if rest.is_empty() {
        return Some(MarkerToken {
            kind,
            condition: None,
        });
    }

    // Conditional marker: modifier is required
    let space2 = rest.find(|c: char| c.is_whitespace())?;
    let modifier = match &rest[..space2] {
        "if" => MarkerModifier::If,
        "unless" => MarkerModifier::Unless,
        _ => return None,
    };
    let rest = rest[space2..].trim_start();

    if rest.is_empty() {
        return None;
    }

    // Parse LHS expression
    let (lhs, rest) = lex_marker_expr(rest)?;
    let rest = rest.trim_start();

    // Check what follows
    let body = if rest.is_empty() {
        // Truthiness check
        MarkerCondBody::Bare(lhs)
    } else {
        match rest.as_bytes()[0] {
            b'=' => {
                let rest = rest[1..].trim_start();
                let (rhs, trailing) = lex_marker_expr(rest)?;
                if !trailing.trim().is_empty() {
                    return None;
                }
                MarkerCondBody::Eq(lhs, rhs)
            }
            b'?' => {
                let rest = rest[1..].trim_start();
                let fragments = lex_marker_regex(rest);
                MarkerCondBody::Regex(lhs, fragments)
            }
            _ => return None,
        }
    };

    Some(MarkerToken {
        kind,
        condition: Some(MarkerCondition { modifier, body }),
    })
}

// ─── Public API ─────────────────────────────────────────────

pub fn lex(source: &str) -> Vec<Spanned<'_>> {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        // logos morph callbacks corrupt span(); derive position from remainder
        let end = source.len() - lexer.remainder().len();
        let start = tokens.last().map_or(0, |t: &Spanned| t.span.end);
        // skip spaces (matching logos #[logos(skip r" +")])
        let start = source[start..end]
            .find(|c: char| c != ' ')
            .map_or(start, |i| start + i);
        let span = start..end;
        match result {
            Ok(tok) => tokens.push(Spanned { node: tok, span }),
            Err(()) => match tokens.last_mut() {
                Some(prev)
                    if matches!(prev.node, Token::Unrecognized(_))
                        && prev.span.end == span.start =>
                {
                    prev.span.end = span.end;
                    prev.node = Token::Unrecognized(&source[prev.span.clone()]);
                }
                _ => tokens.push(Spanned {
                    node: Token::Unrecognized(&source[span.clone()]),
                    span,
                }),
            },
        }
    }
    tokens
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token<'_>> {
        lex(input).into_iter().map(|s| s.node).collect()
    }

    fn spans(input: &str) -> Vec<std::ops::Range<usize>> {
        lex(input).into_iter().map(|s| s.span).collect()
    }

    #[test]
    fn test_import_selective() {
        let input = "import lib/module1 { foo, bar as b, StartDb as Db }\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Import,
                Token::ModulePath("lib/module1"),
                Token::BraceOpen,
                Token::Ident("foo"),
                Token::Comma,
                Token::Ident("bar"),
                Token::As,
                Token::Ident("b"),
                Token::Comma,
                Token::EffectIdent("StartDb"),
                Token::As,
                Token::EffectIdent("Db"),
                Token::BraceClose,
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..6);   // import
        assert_eq!(sp[1], 7..18);  // lib/module1
        assert_eq!(sp[2], 19..20); // {
        assert_eq!(sp[3], 21..24); // foo
        assert_eq!(sp[12], 50..51); // }
        assert_eq!(sp[13], 51..52); // \n
    }

    #[test]
    fn test_import_wildcard() {
        let input = "import lib/module2\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Import, Token::ModulePath("lib/module2"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..6);   // import
        assert_eq!(sp[1], 7..18);  // lib/module2
        assert_eq!(sp[2], 18..19); // \n
    }

    #[test]
    fn test_send_with_interp() {
        let input = "> echo ${name}\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::Interpolation("name"),
            ])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_send_strict_space() {
        let input = "> hello\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Send(vec![PayloadFragment::Text(" hello")])]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_match_regex() {
        let input = "<? listening on port (\\d+)\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::MatchRegex(vec![PayloadFragment::Text(
                " listening on port (\\d+)"
            )])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_match_literal() {
        let input = "<= hello world\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::MatchLiteral(vec![PayloadFragment::Text(" hello world")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_send_raw() {
        let input = "=> partial\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::SendRaw(vec![PayloadFragment::Text(" partial")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_fail_regex() {
        let input = "!? [Ee]rror|FATAL\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::FailRegex(vec![PayloadFragment::Text(" [Ee]rror|FATAL")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_fail_literal() {
        let input = "!= error\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::FailLiteral(vec![PayloadFragment::Text(" error")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timeout() {
        let input = "~10s\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Timeout("10s"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..4);  // ~10s
        assert_eq!(sp[1], 4..5); // \n
    }

    #[test]
    fn test_timeout_compact() {
        let input = "~2h30m12s\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Timeout("2h30m12s"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..9);   // ~2h30m12s
        assert_eq!(sp[1], 9..10);  // \n
    }

    #[test]
    fn test_docstring() {
        let input = "\"\"\"\nhello world\n\"\"\"\n";
        assert_eq!(input.len(), 20);
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::DocString(vec!["\nhello world\n"]), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..19);  // """..."""
        assert_eq!(sp[1], 19..20); // \n
    }

    #[test]
    fn test_fn_decl() {
        let input = "fn foo(a, b) {\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Fn,
                Token::Ident("foo"),
                Token::ParenOpen,
                Token::Ident("a"),
                Token::Comma,
                Token::Ident("b"),
                Token::ParenClose,
                Token::BraceOpen,
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..2);   // fn
        assert_eq!(sp[1], 3..6);   // foo
        assert_eq!(sp[2], 6..7);   // (
        assert_eq!(sp[3], 7..8);   // a
        assert_eq!(sp[4], 8..9);   // ,
        assert_eq!(sp[5], 10..11); // b
        assert_eq!(sp[6], 11..12); // )
        assert_eq!(sp[7], 13..14); // {
        assert_eq!(sp[8], 14..15); // \n
    }

    #[test]
    fn test_effect_head() {
        let input = "effect StartDb -> db {\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Effect,
                Token::EffectIdent("StartDb"),
                Token::Arrow,
                Token::Ident("db"),
                Token::BraceOpen,
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..6);   // effect
        assert_eq!(sp[1], 7..14);  // StartDb
        assert_eq!(sp[2], 15..17); // ->
        assert_eq!(sp[3], 18..20); // db
        assert_eq!(sp[4], 21..22); // {
        assert_eq!(sp[5], 22..23); // \n
    }

    #[test]
    fn test_need_with_overlay() {
        let input = "need E3 {\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::Need, Token::EffectIdent("E3"), Token::BraceOpen, Token::Newline]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..4);  // need
        assert_eq!(sp[1], 5..7);  // E3
        assert_eq!(sp[2], 8..9);  // {
        assert_eq!(sp[3], 9..10); // \n
    }

    #[test]
    fn test_comment_preserved() {
        let input = "# this is a comment\nlet x\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Comment(" this is a comment"),
                Token::Newline,
                Token::Let,
                Token::Ident("x"),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..19);  // # this is a comment
        assert_eq!(sp[1], 19..20); // \n
        assert_eq!(sp[2], 20..23); // let
        assert_eq!(sp[3], 24..25); // x
        assert_eq!(sp[4], 25..26); // \n
    }

    #[test]
    fn test_comment_no_space() {
        let input = "#compact comment\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Comment("compact comment"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..16);  // #compact comment
        assert_eq!(sp[1], 16..17); // \n
    }

    #[test]
    fn test_comment_empty() {
        let input = "#\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Comment(""), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..1); // #
        assert_eq!(sp[1], 1..2); // \n
    }

    #[test]
    fn test_comment_inline_after_code() {
        let input = "let x # trailing comment\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Comment(" trailing comment"),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..3);   // let
        assert_eq!(sp[1], 4..5);   // x
        assert_eq!(sp[2], 6..24);  // # trailing comment
        assert_eq!(sp[3], 24..25); // \n
    }

    #[test]
    fn test_comment_with_symbols() {
        let input = "# > this is not a send\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Comment(" > this is not a send"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..22);  // # > this is not a send
        assert_eq!(sp[1], 22..23); // \n
    }

    #[test]
    fn test_escaped_dollar_in_payload() {
        let input = "> echo $$HOME\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::EscapedDollar,
                PayloadFragment::Text("HOME"),
            ])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_let_with_string() {
        let input = "let x = \"foo\"\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Eq,
                Token::String(vec![StringFragment::Text("foo")]),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..3);   // let
        assert_eq!(sp[1], 4..5);   // x
        assert_eq!(sp[2], 6..7);   // =
        assert_eq!(sp[3], 8..13);  // "foo"
        assert_eq!(sp[4], 13..14); // \n
    }

    #[test]
    fn test_let_with_interp_string() {
        let input = "let x = \"hello ${name}\"\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Eq,
                Token::String(vec![
                    StringFragment::Text("hello "),
                    StringFragment::Interpolation("name"),
                ]),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[3], 8..23);  // "hello ${name}"
        assert_eq!(sp[4], 23..24); // \n
    }

    #[test]
    fn test_braced_numeric_interp() {
        let input = "let x = ${1}\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Eq,
                Token::Interpolation("1"),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[3], 8..12);  // ${1}
        assert_eq!(sp[4], 12..13); // \n
    }

    #[test]
    fn test_bare_dollar_digit_in_payload() {
        let input = "> echo $1\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::Text("$"),
                PayloadFragment::Text("1"),
            ])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_bare_dollar_digit_in_string() {
        let input = "let x = \"val=$1\"\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Eq,
                Token::String(vec![
                    StringFragment::Text("val="),
                    StringFragment::Text("$1"),
                ]),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[3], 8..16);  // "val=$1"
        assert_eq!(sp[4], 16..17); // \n
    }

    #[test]
    fn test_let_uninitialized() {
        let input = "let x\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Let, Token::Ident("x"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..3); // let
        assert_eq!(sp[1], 4..5); // x
        assert_eq!(sp[2], 5..6); // \n
    }

    #[test]
    fn test_unrecognized_input_squashed() {
        let toks = lex("import !!!\n");
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[0].node, Token::Import);
        assert_eq!(toks[0].span, 0..6);
        assert_eq!(toks[1].node, Token::Unrecognized("!!!"));
        assert_eq!(toks[1].span, 7..10);
        assert_eq!(toks[2].node, Token::Newline);
        assert_eq!(toks[2].span, 10..11);
    }

    #[test]
    fn test_unrecognized_not_squashed_across_valid_token() {
        let toks = lex("! import !\n");
        assert_eq!(toks[0].node, Token::Unrecognized("!"));
        assert_eq!(toks[0].span, 0..1);
        assert_eq!(toks[1].node, Token::Import);
        assert_eq!(toks[1].span, 2..8);
        assert_eq!(toks[2].node, Token::Unrecognized("!"));
        assert_eq!(toks[2].span, 9..10);
        assert_eq!(toks[3].node, Token::Newline);
        assert_eq!(toks[3].span, 10..11);
    }

    #[test]
    fn test_ident_vs_effect_ident() {
        let input = "foo Bar _baz Qux\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Ident("foo"),
                Token::EffectIdent("Bar"),
                Token::Ident("_baz"),
                Token::EffectIdent("Qux"),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..3);   // foo
        assert_eq!(sp[1], 4..7);   // Bar
        assert_eq!(sp[2], 8..12);  // _baz
        assert_eq!(sp[3], 13..16); // Qux
        assert_eq!(sp[4], 16..17); // \n
    }

    #[test]
    fn test_payload_spans_in_sequence() {
        let input = "> cmd1\n<? regex\n<= literal\n";
        let sp = spans(input);
        assert_eq!(sp[0], 0..7);   // > cmd1\n   (7 bytes)
        assert_eq!(sp[1], 7..16);  // <? regex\n (9 bytes)
        assert_eq!(sp[2], 16..27); // <= literal\n (11 bytes)
    }

    #[test]
    fn test_number_token() {
        let input = "42\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Number("42"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..2); // 42
        assert_eq!(sp[1], 2..3); // \n
    }

    #[test]
    fn test_number_with_idents() {
        let input = "let x = 8\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Eq,
                Token::Number("8"),
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..3); // let
        assert_eq!(sp[1], 4..5); // x
        assert_eq!(sp[2], 6..7); // =
        assert_eq!(sp[3], 8..9); // 8
        assert_eq!(sp[4], 9..10); // \n
    }

    #[test]
    fn test_number_in_call() {
        let input = "rand(8)\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Ident("rand"),
                Token::ParenOpen,
                Token::Number("8"),
                Token::ParenClose,
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..4); // rand
        assert_eq!(sp[1], 4..5); // (
        assert_eq!(sp[2], 5..6); // 8
        assert_eq!(sp[3], 6..7); // )
        assert_eq!(sp[4], 7..8); // \n
    }

    #[test]
    fn test_neg_match_regex() {
        let input = "<!? error pattern\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::NegMatchRegex(vec![PayloadFragment::Text(" error pattern")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_neg_match_literal() {
        let input = "<!= some literal\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::NegMatchLiteral(vec![PayloadFragment::Text(" some literal")])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timed_match_regex() {
        let input = "<~2s? regex pattern\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::TimedMatchRegex(("2s", vec![PayloadFragment::Text(" regex pattern")]))]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timed_match_literal() {
        let input = "<~500ms= literal text\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::TimedMatchLiteral(("500ms", vec![PayloadFragment::Text(" literal text")]))]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timed_neg_match_regex() {
        let input = "<~1m30s!? error regex\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::TimedNegMatchRegex(("1m30s", vec![PayloadFragment::Text(" error regex")]))]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timed_neg_match_literal() {
        let input = "<~30s!= error literal\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::TimedNegMatchLiteral(("30s", vec![PayloadFragment::Text(" error literal")]))]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_timed_match_with_interp() {
        let input = "<~5s? listening on ${port}\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::TimedMatchRegex(("5s", vec![
                PayloadFragment::Text(" listening on "),
                PayloadFragment::Interpolation("port"),
            ]))]
        );
    }

    #[test]
    fn test_marker_bare_var() {
        let input = "[skip unless \"${FOO}\"]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Skip,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::Unless,
                        body: MarkerCondBody::Bare(MarkerExpr::String(vec![
                            StringFragment::Interpolation("FOO"),
                        ])),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_literal_eq() {
        let input = "[run if \"${BAR}\" = \"linux\"]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Run,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::If,
                        body: MarkerCondBody::Eq(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("BAR"),
                            ]),
                            MarkerExpr::String(vec![
                                StringFragment::Text("linux"),
                            ]),
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_eq_number() {
        let input = "[run if \"${COUNT}\" = 0]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Run,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::If,
                        body: MarkerCondBody::Eq(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("COUNT"),
                            ]),
                            MarkerExpr::Number("0"),
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_regex() {
        let input = "[skip unless \"${ARCH}\" ? ^(x86_64|aarch64)$]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Skip,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::Unless,
                        body: MarkerCondBody::Regex(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("ARCH"),
                            ]),
                            vec![PayloadFragment::Text("^(x86_64|aarch64)"), PayloadFragment::Text("$")],
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_regex_with_bracket() {
        let input = "[skip unless \"${FOO}\" ? ^[a-z]+$]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Skip,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::Unless,
                        body: MarkerCondBody::Regex(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("FOO"),
                            ]),
                            vec![PayloadFragment::Text("^[a-z]+"), PayloadFragment::Text("$")],
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_regex_with_interpolation() {
        let input = "[skip unless \"${VER}\" ? ^${MAJOR}\\..*$]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Skip,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::Unless,
                        body: MarkerCondBody::Regex(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("VER"),
                            ]),
                            vec![
                                PayloadFragment::Text("^"),
                                PayloadFragment::Interpolation("MAJOR"),
                                PayloadFragment::Text("\\..*"),
                                PayloadFragment::Text("$"),
                            ],
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    /// Helper: lex regex content directly via `lex_marker_regex` and return fragments.
    fn marker_regex_fragments(regex: &str) -> Vec<PayloadFragment<'_>> {
        super::lex_marker_regex(regex)
    }

    #[test]
    fn test_marker_regex_backslash_escapes() {
        let frags = marker_regex_fragments(r"\d{3}-\d{4}");
        assert_eq!(frags, vec![PayloadFragment::Text(r"\d{3}-\d{4}")]);
    }

    #[test]
    fn test_marker_regex_quantifiers() {
        let frags = marker_regex_fragments("a{2,5}b+c*d?");
        assert_eq!(frags, vec![PayloadFragment::Text("a{2,5}b+c*d?")]);
    }

    #[test]
    fn test_marker_regex_char_class_nested_bracket() {
        let frags = marker_regex_fragments(r"^[a-z\]]+$");
        assert_eq!(
            frags,
            vec![PayloadFragment::Text(r"^[a-z\]]+"), PayloadFragment::Text("$")]
        );
    }

    #[test]
    fn test_marker_regex_groups_and_alternation() {
        let frags = marker_regex_fragments("(?:foo|bar)");
        assert_eq!(frags, vec![PayloadFragment::Text("(?:foo|bar)")]);
    }

    #[test]
    fn test_marker_regex_dot_and_anchors() {
        let frags = marker_regex_fragments("^.*$");
        assert_eq!(
            frags,
            vec![PayloadFragment::Text("^.*"), PayloadFragment::Text("$")]
        );
    }

    #[test]
    fn test_marker_regex_bare_dollar_mid() {
        let frags = marker_regex_fragments("foo$bar");
        assert_eq!(
            frags,
            vec![
                PayloadFragment::Text("foo"),
                PayloadFragment::Text("$"),
                PayloadFragment::Text("bar"),
            ]
        );
    }

    #[test]
    fn test_marker_regex_escaped_dollar() {
        let frags = marker_regex_fragments("cost $$5");
        assert_eq!(
            frags,
            vec![
                PayloadFragment::Text("cost "),
                PayloadFragment::EscapedDollar,
                PayloadFragment::Text("5"),
            ]
        );
    }

    #[test]
    fn test_marker_regex_interp_adjacent_to_text() {
        let frags = marker_regex_fragments("pre${VAR}post");
        assert_eq!(
            frags,
            vec![
                PayloadFragment::Text("pre"),
                PayloadFragment::Interpolation("VAR"),
                PayloadFragment::Text("post"),
            ]
        );
    }

    #[test]
    fn test_marker_regex_only_dollar_anchor() {
        let frags = marker_regex_fragments("$");
        assert_eq!(frags, vec![PayloadFragment::Text("$")]);
    }

    #[test]
    fn test_marker_regex_consecutive_dollars() {
        let frags = marker_regex_fragments("$$$$");
        assert_eq!(
            frags,
            vec![PayloadFragment::EscapedDollar, PayloadFragment::EscapedDollar]
        );
    }

    #[test]
    fn test_marker_regex_lookahead() {
        let frags = marker_regex_fragments("foo(?=bar)");
        assert_eq!(frags, vec![PayloadFragment::Text("foo(?=bar)")]);
    }

    #[test]
    fn test_marker_regex_curly_braces() {
        let frags = marker_regex_fragments(r"\d{3,}");
        assert_eq!(frags, vec![PayloadFragment::Text(r"\d{3,}")]);
    }

    #[test]
    fn test_marker_flaky() {
        let input = "[flaky if \"${CI}\"]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Flaky,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::If,
                        body: MarkerCondBody::Bare(MarkerExpr::String(vec![
                            StringFragment::Interpolation("CI"),
                        ])),
                    }),
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_invalid_kind() {
        let input = "[nope if \"${FOO}\"]\n";
        let toks = tokens(input);
        assert!(matches!(toks[0], Token::Unrecognized(_)));
    }

    #[test]
    fn test_marker_invalid_modifier_word() {
        let input = "[skip FOO]\n";
        let toks = tokens(input);
        assert!(matches!(toks[0], Token::Unrecognized(_)));
    }

    #[test]
    fn test_bare_skip_marker() {
        let input = "[skip]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Skip,
                    condition: None,
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_bare_flaky_marker() {
        let input = "[flaky]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Flaky,
                    condition: None,
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_bare_run_marker() {
        let input = "[run]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Run,
                    condition: None,
                }),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_marker_string_with_compound_interp() {
        let input = "[run if \"${HOST}:${PORT}\" = \"localhost:8080\"]\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Marker(MarkerToken {
                    kind: MarkerKind::Run,
                    condition: Some(MarkerCondition {
                        modifier: MarkerModifier::If,
                        body: MarkerCondBody::Eq(
                            MarkerExpr::String(vec![
                                StringFragment::Interpolation("HOST"),
                                StringFragment::Text(":"),
                                StringFragment::Interpolation("PORT"),
                            ]),
                            MarkerExpr::String(vec![
                                StringFragment::Text("localhost:8080"),
                            ]),
                        ),
                    }),
                }),
                Token::Newline,
            ]
        );
    }
}
