pub mod tokens;

use logos::Logos;
pub use tokens::{PayloadFragment, Spanned, StringFragment, Token};

// ─── Internal Lexer Modes ───────────────────────────────────

#[derive(Logos, Debug, PartialEq, Clone)]
enum PayloadMode<'a> {
    #[regex(r"\$(\{[a-zA-Z_0-9]+\}|[0-9]+)", |lex| {
        let s = lex.slice();
        if s.as_bytes()[1] == b'{' { &s[2..s.len()-1] } else { &s[1..] }
    })]
    Interpolation(&'a str),

    #[token("$$", priority = 5)]
    EscapedDollar,

    #[regex(r"[^\n$]+")]
    Text(&'a str),

    #[regex(r"\$[^{\n0-9$]")]
    LiteralDollar(&'a str),

    #[regex(r"\$\n")]
    TrailingDollar,

    #[token("\n")]
    Newline,
}

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
enum StringMode<'a> {
    #[regex(r"\$(\{[a-zA-Z_0-9]+\}|[0-9]+)", |lex| {
        let s = lex.slice();
        if s.as_bytes()[1] == b'{' { &s[2..s.len()-1] } else { &s[1..] }
    })]
    Interpolation(&'a str),

    #[token("$$")]
    EscapedDollar,

    #[regex(r"\\.", priority = 5)]
    Escape(&'a str),

    #[regex(r#"[^"\\$\n]+"#)]
    Text(&'a str),

    #[regex(r"\$[^{\n0-9$]")]
    LiteralDollar(&'a str),

    #[token("\"")]
    Close,
}

// ─── Morph Callbacks ────────────────────────────────────────

fn lex_payload<'s>(lex: &mut logos::Lexer<'s, Token<'s>>) -> Option<Vec<PayloadFragment<'s>>> {
    let mut sub = lex.clone().morph::<PayloadMode<'s>>();
    let mut fragments = Vec::new();
    while let Some(result) = sub.next() {
        match result {
            Ok(PayloadMode::Interpolation(s)) => {
                fragments.push(PayloadFragment::Interpolation(s));
            }
            Ok(PayloadMode::EscapedDollar) => {
                fragments.push(PayloadFragment::EscapedDollar);
            }
            Ok(PayloadMode::Text(s)) => {
                fragments.push(PayloadFragment::Text(s));
            }
            Ok(PayloadMode::LiteralDollar(s)) => {
                fragments.push(PayloadFragment::Text(s));
            }
            Ok(PayloadMode::TrailingDollar) => {
                fragments.push(PayloadFragment::Text("$"));
                break;
            }
            Ok(PayloadMode::Newline) => break,
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
    fn test_timeout_compound() {
        let input = "~2h 30m 12s\n";
        let toks = tokens(input);
        assert_eq!(toks, vec![Token::Timeout("2h 30m 12s"), Token::Newline]);
        let sp = spans(input);
        assert_eq!(sp[0], 0..11);  // ~2h 30m 12s
        assert_eq!(sp[1], 11..12); // \n
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
        let input = "effect StartDb -> shell db {\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Effect,
                Token::EffectIdent("StartDb"),
                Token::Arrow,
                Token::Shell,
                Token::Ident("db"),
                Token::BraceOpen,
                Token::Newline,
            ]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..6);   // effect
        assert_eq!(sp[1], 7..14);  // StartDb
        assert_eq!(sp[2], 15..17); // ->
        assert_eq!(sp[3], 18..23); // shell
        assert_eq!(sp[4], 24..26); // db
        assert_eq!(sp[5], 27..28); // {
        assert_eq!(sp[6], 28..29); // \n
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
    fn test_bare_interp_in_payload() {
        let input = "> echo $1\n";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::Interpolation("1"),
            ])]
        );
        let sp = spans(input);
        assert_eq!(sp[0], 0..input.len());
    }

    #[test]
    fn test_bare_interp_in_string() {
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
                    StringFragment::Interpolation("1"),
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
}
