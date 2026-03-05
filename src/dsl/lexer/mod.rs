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

    #[test]
    fn test_import_selective() {
        let toks = tokens("import lib/module1 { foo, bar as b, StartDb as Db }\n");
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
    }

    #[test]
    fn test_import_selective_multiline() {
        let toks = tokens("import lib/module1 {\n foo,\n bar as b\n}\n");
        assert_eq!(
            toks,
            vec![
                Token::Import,
                Token::ModulePath("lib/module1"),
                Token::BraceOpen,
                Token::Newline,
                Token::Ident("foo"),
                Token::Comma,
                Token::Newline,
                Token::Ident("bar"),
                Token::As,
                Token::Ident("b"),
                Token::Newline,
                Token::BraceClose,
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_import_wildcard() {
        let toks = tokens("import lib/module2\n");
        assert_eq!(
            toks,
            vec![
                Token::Import,
                Token::ModulePath("lib/module2"),
                Token::Newline
            ]
        );
    }

    #[test]
    fn test_send_with_interp() {
        let toks = tokens("> echo ${name}\n");
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::Interpolation("name"),
            ])]
        );
    }

    #[test]
    fn test_send_strict_space() {
        let toks = tokens("> hello\n");
        assert_eq!(
            toks,
            vec![Token::Send(vec![PayloadFragment::Text(" hello")])]
        );
    }

    #[test]
    fn test_match_regex() {
        let toks = tokens("<? listening on port (\\d+)\n");
        assert_eq!(
            toks,
            vec![Token::MatchRegex(vec![PayloadFragment::Text(
                " listening on port (\\d+)"
            ),])]
        );
    }

    #[test]
    fn test_match_regex_span() {
        let input = "<? listening on port (\\d+)\n";
        let spanned = lex(input);
        assert_eq!(spanned.len(), 1);
        assert_eq!(spanned[0].span, 0..input.len()); // callback consumes trailing \n
    }

    #[test]
    fn test_timeout() {
        let toks = tokens("~10s\n");
        assert_eq!(toks, vec![Token::Timeout("10s"), Token::Newline]);
    }

    #[test]
    fn test_timeout_compound() {
        let toks = tokens("~2h 30m 12s\n");
        assert_eq!(toks, vec![Token::Timeout("2h 30m 12s"), Token::Newline]);
    }

    #[test]
    fn test_docstring() {
        let toks = tokens("\"\"\"\nhello world\n\"\"\"\n");
        assert_eq!(
            toks,
            vec![Token::DocString(vec!["\nhello world\n"]), Token::Newline]
        );
    }

    #[test]
    fn test_fn_decl() {
        let toks = tokens("fn foo(a, b) {\n");
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
    }

    #[test]
    fn test_effect_head() {
        let toks = tokens("effect StartDb -> shell db {\n");
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
    }

    #[test]
    fn test_need_with_overlay() {
        let toks = tokens("need E3 {\n");
        assert_eq!(
            toks,
            vec![
                Token::Need,
                Token::EffectIdent("E3"),
                Token::BraceOpen,
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_fail_pattern() {
        let toks = tokens("!? [Ee]rror|FATAL\n");
        assert_eq!(
            toks,
            vec![Token::FailRegex(vec![PayloadFragment::Text(
                " [Ee]rror|FATAL"
            ),])]
        );
    }

    #[test]
    fn test_comment_preserved() {
        let toks = tokens("# this is a comment\nlet x\n");
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
    }

    #[test]
    fn test_comment_no_space() {
        let toks = tokens("#compact comment\n");
        assert_eq!(
            toks,
            vec![Token::Comment("compact comment"), Token::Newline]
        );
    }

    #[test]
    fn test_comment_empty() {
        let toks = tokens("#\n");
        assert_eq!(toks, vec![Token::Comment(""), Token::Newline]);
    }

    #[test]
    fn test_comment_inline_after_code() {
        let toks = tokens("let x # trailing comment\n");
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x"),
                Token::Comment(" trailing comment"),
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_comment_with_symbols() {
        let toks = tokens("# > this is not a send\n");
        assert_eq!(
            toks,
            vec![Token::Comment(" > this is not a send"), Token::Newline]
        );
    }

    #[test]
    fn test_escaped_dollar_in_payload() {
        let toks = tokens("> echo $$HOME\n");
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::EscapedDollar,
                PayloadFragment::Text("HOME"),
            ])]
        );
    }

    #[test]
    fn test_let_with_string() {
        let toks = tokens("let x = \"foo\"\n");
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
    }

    #[test]
    fn test_let_with_interp_string() {
        let toks = tokens("let x = \"hello ${name}\"\n");
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
    }

    #[test]
    fn test_braced_numeric_interp() {
        let toks = tokens("let x = ${1}\n");
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
    }

    #[test]
    fn test_bare_interp_in_payload() {
        let toks = tokens("> echo $1\n");
        assert_eq!(
            toks,
            vec![Token::Send(vec![
                PayloadFragment::Text(" echo "),
                PayloadFragment::Interpolation("1"),
            ])]
        );
    }

    #[test]
    fn test_bare_interp_in_string() {
        let toks = tokens("let x = \"val=$1\"\n");
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
    }

    #[test]
    fn test_let_uninitialized() {
        let toks = tokens("let x\n");
        assert_eq!(toks, vec![Token::Let, Token::Ident("x"), Token::Newline]);
    }

    #[test]
    fn test_unrecognized_input_squashed() {
        let toks = lex("import !!!\n");
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[0].node, Token::Import);
        assert_eq!(toks[1].node, Token::Unrecognized("!!!"));
        assert_eq!(toks[1].span, 7..10);
        assert_eq!(toks[2].node, Token::Newline);
    }

    #[test]
    fn test_unrecognized_not_squashed_across_valid_token() {
        let toks = lex("! import !\n");
        assert_eq!(toks[0].node, Token::Unrecognized("!"));
        assert_eq!(toks[0].span, 0..1);
        assert_eq!(toks[1].node, Token::Import);
        assert_eq!(toks[2].node, Token::Unrecognized("!"));
        assert_eq!(toks[2].span, 9..10);
    }

    #[test]
    fn test_ident_vs_effect_ident() {
        let toks = tokens("foo Bar _baz Qux\n");
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
    }
}
