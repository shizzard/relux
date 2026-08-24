use std::fmt;

use logos::Logos;

// --- Helpers ---------------------------------------------

// --- Token -----------------------------------------------

#[derive(Logos, PartialEq, Clone)]
pub enum Token<'a> {
    // --- Keywords ----------------------------------------
    #[token("fn")]
    Fn,
    #[token("pure")]
    Pure,
    #[token("effect")]
    Effect,
    #[token("test")]
    Test,
    #[token("shell")]
    Shell,
    #[token("let")]
    Let,
    #[token("start")]
    Start,
    #[token("expect")]
    Expect,
    #[token("expose")]
    Expose,
    #[token("var")]
    Var,
    #[token("import")]
    Import,
    #[token("cleanup")]
    Cleanup,
    #[token("as")]
    As,

    // Word (identifier catch-all - longer match beats keywords)
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")]
    Word(&'a str),

    // --- Symbols (single character) ----------------------
    #[token("$")]
    Dollar,
    #[token("{")]
    BraceOpen,
    #[token("}")]
    BraceClose,
    #[token("(")]
    ParenOpen,
    #[token(")")]
    ParenClose,
    #[token("\"")]
    Quote,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("=")]
    Eq,
    #[token("!")]
    Bang,
    #[token("?")]
    Question,
    #[token("~")]
    Tilde,
    #[token("@")]
    At,
    #[token("\\")]
    Backslash,
    #[regex(r"\\[^\r\n]", priority = 10, callback = |lex| &lex.source()[lex.span().start+1..lex.span().end])]
    Escape(&'a str),
    #[token("#")]
    Hash,
    #[token("[")]
    BracketOpen,
    #[token("]")]
    BracketClose,
    #[token(",")]
    Comma,
    #[token("/")]
    Slash,
    #[token("-")]
    Dash,
    #[token(".")]
    Dot,
    #[token(":")]
    Colon,

    // --- Whitespace --------------------------------------
    #[regex(" +")]
    Space(&'a str),
    #[regex("\t+")]
    Tab(&'a str),
    #[token("\n")]
    #[token("\r\n")]
    Newline,

    // --- Text (catch-all, produced by post-lex squashing) ---
    Text(&'a str),
}

pub type Spanned<'a> = relux_core::Spanned<Token<'a>>;

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Fn => write!(f, "fn"),
            Token::Pure => write!(f, "pure"),
            Token::Effect => write!(f, "effect"),
            Token::Test => write!(f, "test"),
            Token::Shell => write!(f, "shell"),
            Token::Let => write!(f, "let"),
            Token::Start => write!(f, "start"),
            Token::Expect => write!(f, "expect"),
            Token::Expose => write!(f, "expose"),
            Token::Var => write!(f, "var"),
            Token::Import => write!(f, "import"),
            Token::Cleanup => write!(f, "cleanup"),
            Token::As => write!(f, "as"),
            Token::Dollar => write!(f, "$"),
            Token::BraceOpen => write!(f, "{{"),
            Token::BraceClose => write!(f, "}}"),
            Token::ParenOpen => write!(f, "("),
            Token::ParenClose => write!(f, ")"),
            Token::Quote => write!(f, "\""),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::Eq => write!(f, "="),
            Token::Bang => write!(f, "!"),
            Token::Question => write!(f, "?"),
            Token::Tilde => write!(f, "~"),
            Token::At => write!(f, "@"),
            Token::Backslash => write!(f, "\\"),
            Token::Hash => write!(f, "#"),
            Token::BracketOpen => write!(f, "["),
            Token::BracketClose => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Slash => write!(f, "/"),
            Token::Dash => write!(f, "-"),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Space(s) => write!(f, "{s}"),
            Token::Tab(s) => write!(f, "{s}"),
            Token::Newline => write!(f, "\\n"),
            Token::Escape(s) => write!(f, "\\{s}"),
            Token::Word(s) | Token::Text(s) => write!(f, "{s}"),
        }
    }
}

impl fmt::Debug for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Newline => writeln!(f, "[newline]"),
            Token::Space(s) => write!(f, "[space:{}]", s.len()),
            Token::Tab(s) => write!(f, "[tab:{}]", s.len()),
            Token::Word(s) => write!(f, "word({s:?})"),
            Token::Text(s) => write!(f, "text({s:?})"),
            Token::Escape(s) => write!(f, "escape({s:?})"),
            _ => write!(f, "'{self}'"),
        }
    }
}

// --- Public API ------------------------------------------

pub fn lex(source: &str) -> Vec<Spanned<'_>> {
    let mut lexer = Token::lexer(source);
    let mut tokens: Vec<Spanned<'_>> = Vec::new();

    while let Some(result) = lexer.next() {
        let span = relux_core::Span::from(lexer.span());
        match result {
            Ok(Token::Word(_)) => match tokens.last_mut() {
                Some(prev)
                    if matches!(prev.node, Token::Text(_)) && prev.span.end() == span.start() =>
                {
                    prev.span = prev.span.extend_end(span.end());
                    prev.node = Token::Text(&source[prev.span.start()..prev.span.end()]);
                }
                _ => tokens.push(relux_core::Spanned {
                    node: Token::Text(&source[span.start()..span.end()]),
                    span,
                }),
            },
            Ok(tok) => tokens.push(relux_core::Spanned { node: tok, span }),
            Err(()) => {
                // Squash adjacent unmatched bytes into a single Text token
                match tokens.last_mut() {
                    Some(prev)
                        if matches!(prev.node, Token::Text(_))
                            && prev.span.end() == span.start() =>
                    {
                        prev.span = prev.span.extend_end(span.end());
                        prev.node = Token::Text(&source[prev.span.start()..prev.span.end()]);
                    }
                    _ => tokens.push(relux_core::Spanned {
                        node: Token::Text(&source[span.start()..span.end()]),
                        span,
                    }),
                }
            }
        }
    }

    tokens
}

// --- TimeoutKind (shared with parser/resolver) -----------

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TimeoutKind {
    Tolerance,
    Assertion,
}

impl TimeoutKind {
    pub fn prefix(self) -> char {
        match self {
            TimeoutKind::Tolerance => '~',
            TimeoutKind::Assertion => '@',
        }
    }
}

impl fmt::Display for TimeoutKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.prefix())
    }
}

// --- Tests -----------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token<'_>> {
        lex(input).into_iter().map(|s| s.node).collect()
    }

    fn spans(input: &str) -> Vec<std::ops::Range<usize>> {
        lex(input)
            .into_iter()
            .map(|s| std::ops::Range::from(s.span))
            .collect()
    }

    // ---------------------------------------------------------
    // Symbols
    // ---------------------------------------------------------

    mod symbols {
        use super::*;

        #[test]
        fn individual() {
            assert_eq!(tokens("$"), vec![Token::Dollar]);
            assert_eq!(tokens("{"), vec![Token::BraceOpen]);
            assert_eq!(tokens("}"), vec![Token::BraceClose]);
            assert_eq!(tokens("("), vec![Token::ParenOpen]);
            assert_eq!(tokens(")"), vec![Token::ParenClose]);
            assert_eq!(tokens(r#"""#), vec![Token::Quote]);
            assert_eq!(tokens("<"), vec![Token::Lt]);
            assert_eq!(tokens(">"), vec![Token::Gt]);
            assert_eq!(tokens("="), vec![Token::Eq]);
            assert_eq!(tokens("!"), vec![Token::Bang]);
            assert_eq!(tokens("?"), vec![Token::Question]);
            assert_eq!(tokens("~"), vec![Token::Tilde]);
            assert_eq!(tokens("@"), vec![Token::At]);
            assert_eq!(tokens(r#"\"#), vec![Token::Backslash]);
            assert_eq!(tokens("#"), vec![Token::Hash]);
            assert_eq!(tokens("["), vec![Token::BracketOpen]);
            assert_eq!(tokens("]"), vec![Token::BracketClose]);
            assert_eq!(tokens(","), vec![Token::Comma]);
            assert_eq!(tokens("/"), vec![Token::Slash]);
            assert_eq!(tokens("-"), vec![Token::Dash]);
        }

        #[test]
        fn pairs() {
            assert_eq!(tokens("{}"), vec![Token::BraceOpen, Token::BraceClose]);
            assert_eq!(tokens("()"), vec![Token::ParenOpen, Token::ParenClose]);
            assert_eq!(tokens("[]"), vec![Token::BracketOpen, Token::BracketClose]);
            assert_eq!(tokens("<>"), vec![Token::Lt, Token::Gt]);
            assert_eq!(tokens(r#""""#), vec![Token::Quote, Token::Quote]);
            assert_eq!(tokens("->"), vec![Token::Dash, Token::Gt]);
        }

        #[test]
        fn repeated() {
            assert_eq!(tokens("=="), vec![Token::Eq, Token::Eq]);
            assert_eq!(tokens("<<"), vec![Token::Lt, Token::Lt]);
            assert_eq!(tokens(">>"), vec![Token::Gt, Token::Gt]);
            assert_eq!(tokens("!!"), vec![Token::Bang, Token::Bang]);
            assert_eq!(tokens("??"), vec![Token::Question, Token::Question]);
            assert_eq!(tokens("##"), vec![Token::Hash, Token::Hash]);
            assert_eq!(tokens("//"), vec![Token::Slash, Token::Slash]);
            assert_eq!(tokens("$$"), vec![Token::Dollar, Token::Dollar]);
            assert_eq!(tokens("@@"), vec![Token::At, Token::At]);
            assert_eq!(tokens("~~"), vec![Token::Tilde, Token::Tilde]);
        }

        #[test]
        fn adjacent_chain() {
            assert_eq!(
                tokens("<>=!?~@"),
                vec![
                    Token::Lt,
                    Token::Gt,
                    Token::Eq,
                    Token::Bang,
                    Token::Question,
                    Token::Tilde,
                    Token::At,
                ]
            );
        }

        #[test]
        fn nested_braces() {
            assert_eq!(
                tokens("{{}}"),
                vec![
                    Token::BraceOpen,
                    Token::BraceOpen,
                    Token::BraceClose,
                    Token::BraceClose,
                ]
            );
        }

        #[test]
        fn spans_all_20() {
            // Backslash at EOF has no following char, so Escape doesn't match - bare Backslash.
            let input = "${}()\"<>=!?~@#[],/-\\";
            let sp = spans(input);
            assert_eq!(sp.len(), 20);
            for (i, s) in sp.iter().enumerate() {
                assert_eq!(*s, i..i + 1, "symbol at index {i}");
            }
        }

        #[test]
        fn single_byte_spans() {
            assert_eq!(spans("$"), vec![0..1]);
            assert_eq!(spans("{}"), vec![0..1, 1..2]);
        }
    }

    // ---------------------------------------------------------
    // Whitespace
    // ---------------------------------------------------------

    mod whitespace {
        use super::*;

        #[test]
        fn space_run() {
            assert_eq!(tokens("   "), vec![Token::Space("   ")]);
            assert_eq!(spans("   "), vec![0..3]);
        }

        #[test]
        fn tab_run() {
            assert_eq!(tokens("\t\t"), vec![Token::Tab("\t\t")]);
            assert_eq!(spans("\t\t"), vec![0..2]);
        }

        #[test]
        fn single_space() {
            assert_eq!(tokens(" "), vec![Token::Space(" ")]);
            assert_eq!(spans(" "), vec![0..1]);
        }

        #[test]
        fn single_tab() {
            assert_eq!(tokens("\t"), vec![Token::Tab("\t")]);
            assert_eq!(spans("\t"), vec![0..1]);
        }

        #[test]
        fn newline() {
            assert_eq!(tokens("\n"), vec![Token::Newline]);
            assert_eq!(spans("\n"), vec![0..1]);
        }

        #[test]
        fn mixed() {
            assert_eq!(
                tokens(" \t \n"),
                vec![
                    Token::Space(" "),
                    Token::Tab("\t"),
                    Token::Space(" "),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn multiple_newlines() {
            assert_eq!(
                tokens("\n\n\n"),
                vec![Token::Newline, Token::Newline, Token::Newline]
            );
        }

        #[test]
        fn space_only_line() {
            assert_eq!(tokens("   \n"), vec![Token::Space("   "), Token::Newline]);
        }

        #[test]
        fn tab_only_line() {
            assert_eq!(tokens("\t\n"), vec![Token::Tab("\t"), Token::Newline]);
        }

        #[test]
        fn newline_at_start() {
            assert_eq!(tokens("\nlet"), vec![Token::Newline, Token::Let]);
        }
    }

    // ---------------------------------------------------------
    // Line endings
    // ---------------------------------------------------------

    mod line_endings {
        use super::*;

        #[test]
        fn crlf_is_one_newline_token() {
            assert_eq!(tokens("\r\n"), vec![Token::Newline]);
            assert_eq!(spans("\r\n"), vec![0..2]);
        }

        #[test]
        fn crlf_between_text() {
            assert_eq!(
                tokens("a\r\nb"),
                vec![Token::Text("a"), Token::Newline, Token::Text("b")]
            );
            assert_eq!(spans("a\r\nb"), vec![0..1, 1..3, 3..4]);
        }

        #[test]
        fn lf_still_one_byte() {
            assert_eq!(
                tokens("a\nb"),
                vec![Token::Text("a"), Token::Newline, Token::Text("b")]
            );
            assert_eq!(spans("a\nb"), vec![0..1, 1..2, 2..3]);
        }

        #[test]
        fn mixed_endings_in_one_file() {
            assert_eq!(
                tokens("a\r\nb\nc"),
                vec![
                    Token::Text("a"),
                    Token::Newline,
                    Token::Text("b"),
                    Token::Newline,
                    Token::Text("c"),
                ]
            );
            assert_eq!(spans("a\r\nb\nc"), vec![0..1, 1..3, 3..4, 4..5, 5..6]);
        }

        #[test]
        fn crlf_after_keyword() {
            assert_eq!(tokens("let\r\n"), vec![Token::Let, Token::Newline]);
        }

        #[test]
        fn lone_cr_is_text_not_a_terminator() {
            // Deliberate divergence from the deleted normalize(), which
            // stripped lone CRs. A bare CR in the middle of a line is data.
            assert_eq!(tokens("a\rb"), vec![Token::Text("a\rb")]);
            assert_eq!(spans("a\rb"), vec![0..3]);
        }

        #[test]
        fn lone_cr_alone() {
            assert_eq!(tokens("\r"), vec![Token::Text("\r")]);
            assert_eq!(spans("\r"), vec![0..1]);
        }

        #[test]
        fn lone_cr_at_eof() {
            assert_eq!(tokens("hello\r"), vec![Token::Text("hello\r")]);
            assert_eq!(spans("hello\r"), vec![0..6]);
        }

        #[test]
        fn cr_before_cr_lf() {
            // Only the final pair is a terminator; the first CR is content.
            assert_eq!(tokens("\r\r\n"), vec![Token::Text("\r"), Token::Newline]);
            assert_eq!(spans("\r\r\n"), vec![0..1, 1..3]);
        }
    }

    // ---------------------------------------------------------
    // Keywords
    // ---------------------------------------------------------

    mod keywords {
        use super::*;

        #[test]
        fn all_keywords() {
            assert_eq!(tokens("fn"), vec![Token::Fn]);
            assert_eq!(tokens("pure"), vec![Token::Pure]);
            assert_eq!(tokens("effect"), vec![Token::Effect]);
            assert_eq!(tokens("test"), vec![Token::Test]);
            assert_eq!(tokens("shell"), vec![Token::Shell]);
            assert_eq!(tokens("let"), vec![Token::Let]);
            assert_eq!(tokens("start"), vec![Token::Start]);
            assert_eq!(tokens("expect"), vec![Token::Expect]);
            assert_eq!(tokens("expose"), vec![Token::Expose]);
            assert_eq!(tokens("var"), vec![Token::Var]);
            assert_eq!(tokens("import"), vec![Token::Import]);
            assert_eq!(tokens("cleanup"), vec![Token::Cleanup]);
            assert_eq!(tokens("as"), vec![Token::As]);
        }

        #[test]
        fn case_sensitive() {
            assert_eq!(tokens("Fn"), vec![Token::Text("Fn")]);
            assert_eq!(tokens("FN"), vec![Token::Text("FN")]);
            assert_eq!(tokens("fN"), vec![Token::Text("fN")]);
            assert_eq!(tokens("LET"), vec![Token::Text("LET")]);
            assert_eq!(tokens("Let"), vec![Token::Text("Let")]);
            assert_eq!(tokens("IMPORT"), vec![Token::Text("IMPORT")]);
            assert_eq!(tokens("Import"), vec![Token::Text("Import")]);
            assert_eq!(tokens("EFFECT"), vec![Token::Text("EFFECT")]);
            assert_eq!(tokens("Effect"), vec![Token::Text("Effect")]);
            assert_eq!(tokens("TEST"), vec![Token::Text("TEST")]);
            assert_eq!(tokens("Test"), vec![Token::Text("Test")]);
            assert_eq!(tokens("SHELL"), vec![Token::Text("SHELL")]);
            assert_eq!(tokens("Shell"), vec![Token::Text("Shell")]);
            assert_eq!(tokens("PURE"), vec![Token::Text("PURE")]);
            assert_eq!(tokens("Pure"), vec![Token::Text("Pure")]);
            assert_eq!(tokens("START"), vec![Token::Text("START")]);
            assert_eq!(tokens("Start"), vec![Token::Text("Start")]);
            assert_eq!(tokens("EXPECT"), vec![Token::Text("EXPECT")]);
            assert_eq!(tokens("Expect"), vec![Token::Text("Expect")]);
            assert_eq!(tokens("EXPOSE"), vec![Token::Text("EXPOSE")]);
            assert_eq!(tokens("Expose"), vec![Token::Text("Expose")]);
            assert_eq!(tokens("VAR"), vec![Token::Text("VAR")]);
            assert_eq!(tokens("Var"), vec![Token::Text("Var")]);
            assert_eq!(tokens("CLEANUP"), vec![Token::Text("CLEANUP")]);
            assert_eq!(tokens("Cleanup"), vec![Token::Text("Cleanup")]);
            assert_eq!(tokens("AS"), vec![Token::Text("AS")]);
            assert_eq!(tokens("As"), vec![Token::Text("As")]);
        }

        #[test]
        fn prefix_guard_blocks() {
            // Word regex matches full identifier - keywords never split words
            assert_eq!(tokens("functional"), vec![Token::Text("functional")]);
            assert_eq!(tokens("letter"), vec![Token::Text("letter")]);
            assert_eq!(tokens("testing"), vec![Token::Text("testing")]);
            assert_eq!(tokens("imported"), vec![Token::Text("imported")]);
            assert_eq!(tokens("needless"), vec![Token::Text("needless")]);
            assert_eq!(tokens("effective"), vec![Token::Text("effective")]);
            assert_eq!(tokens("purely"), vec![Token::Text("purely")]);
            assert_eq!(tokens("shellcode"), vec![Token::Text("shellcode")]);
            assert_eq!(tokens("variable"), vec![Token::Text("variable")]);
            assert_eq!(tokens("variant"), vec![Token::Text("variant")]);
        }

        #[test]
        fn followed_by_digit() {
            assert_eq!(tokens("fn0"), vec![Token::Text("fn0")]);
            assert_eq!(tokens("as9"), vec![Token::Text("as9")]);
            assert_eq!(tokens("let1"), vec![Token::Text("let1")]);
            assert_eq!(tokens("import2"), vec![Token::Text("import2")]);
            assert_eq!(tokens("var0"), vec![Token::Text("var0")]);
        }

        #[test]
        fn suffix_split() {
            // Word regex matches the full identifier - no spurious keyword splits
            assert_eq!(tokens("myfn"), vec![Token::Text("myfn")]);
            assert_eq!(tokens("islet"), vec![Token::Text("islet")]);
            assert_eq!(tokens("cleanup_fn"), vec![Token::Text("cleanup_fn")]);
        }

        #[test]
        fn preceded_by_ident_chars() {
            assert_eq!(tokens("_let"), vec![Token::Text("_let")]);
            assert_eq!(tokens("xfn"), vec![Token::Text("xfn")]);
            assert_eq!(tokens("_as"), vec![Token::Text("_as")]);
        }

        #[test]
        fn as_boundary_cases() {
            // "as" is the shortest keyword - verify longer words aren't split
            assert_eq!(tokens("assign"), vec![Token::Text("assign")]);
            assert_eq!(tokens("asset"), vec![Token::Text("asset")]);
            assert_eq!(tokens("asking"), vec![Token::Text("asking")]);
            assert_eq!(tokens("bass"), vec![Token::Text("bass")]);
            assert_eq!(tokens("cascade"), vec![Token::Text("cascade")]);
        }

        #[test]
        fn followed_by_symbol() {
            assert_eq!(tokens("fn("), vec![Token::Fn, Token::ParenOpen]);
            assert_eq!(tokens("let="), vec![Token::Let, Token::Eq]);
            assert_eq!(tokens("as,"), vec![Token::As, Token::Comma]);
            assert_eq!(tokens("start{"), vec![Token::Start, Token::BraceOpen]);
        }

        #[test]
        fn followed_by_newline() {
            assert_eq!(tokens("let\n"), vec![Token::Let, Token::Newline]);
            assert_eq!(tokens("fn\n"), vec![Token::Fn, Token::Newline]);
        }

        #[test]
        fn followed_by_tab() {
            assert_eq!(
                tokens("let\tx"),
                vec![Token::Let, Token::Tab("\t"), Token::Text("x")]
            );
        }

        #[test]
        fn between_text() {
            assert_eq!(
                tokens("x fn y"),
                vec![
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Fn,
                    Token::Space(" "),
                    Token::Text("y"),
                ]
            );
        }

        #[test]
        fn consecutive() {
            assert_eq!(
                tokens("let fn pure"),
                vec![
                    Token::Let,
                    Token::Space(" "),
                    Token::Fn,
                    Token::Space(" "),
                    Token::Pure,
                ]
            );
        }

        #[test]
        fn inside_payload() {
            // Keywords inside operator payloads are still recognized -
            // parser handles context
            assert_eq!(
                tokens(
                    r#"> echo let me know
"#
                ),
                vec![
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Space(" "),
                    Token::Let,
                    Token::Space(" "),
                    Token::Text("me"),
                    Token::Space(" "),
                    Token::Text("know"),
                    Token::Newline,
                ]
            );
        }
    }

    // ---------------------------------------------------------
    // Text (catch-all)
    // ---------------------------------------------------------

    mod text {
        use super::*;

        #[test]
        fn single_word() {
            assert_eq!(tokens("hello"), vec![Token::Text("hello")]);
        }

        #[test]
        fn squashed_adjacent() {
            assert_eq!(tokens("abc"), vec![Token::Text("abc")]);
        }

        #[test]
        fn separated_by_symbol() {
            assert_eq!(
                tokens("a$b"),
                vec![Token::Text("a"), Token::Dollar, Token::Text("b")]
            );
            assert_eq!(spans("a$b"), vec![0..1, 1..2, 2..3]);
        }

        #[test]
        fn separated_by_space() {
            assert_eq!(
                tokens("foo bar"),
                vec![Token::Text("foo"), Token::Space(" "), Token::Text("bar")]
            );
        }

        #[test]
        fn not_squashed_across_keyword() {
            assert_eq!(
                tokens("abc let def"),
                vec![
                    Token::Text("abc"),
                    Token::Space(" "),
                    Token::Let,
                    Token::Space(" "),
                    Token::Text("def"),
                ]
            );
        }

        #[test]
        fn numbers() {
            assert_eq!(tokens("42"), vec![Token::Text("42")]);
            assert_eq!(tokens("3abc"), vec![Token::Text("3abc")]);
            assert_eq!(tokens("abc123"), vec![Token::Text("abc123")]);
        }

        #[test]
        fn underscore_prefixed() {
            assert_eq!(tokens("_foo"), vec![Token::Text("_foo")]);
            assert_eq!(tokens("__init"), vec![Token::Text("__init")]);
        }

        #[test]
        fn camel_case() {
            assert_eq!(tokens("StartDb"), vec![Token::Text("StartDb")]);
            assert_eq!(tokens("CamelCase"), vec![Token::Text("CamelCase")]);
        }

        #[test]
        fn all_caps() {
            assert_eq!(tokens("CI"), vec![Token::Text("CI")]);
            assert_eq!(tokens("HOME"), vec![Token::Text("HOME")]);
        }

        #[test]
        fn mixed_alphanumeric() {
            assert_eq!(tokens("h2o"), vec![Token::Text("h2o")]);
            assert_eq!(tokens("x86_64"), vec![Token::Text("x86_64")]);
        }

        #[test]
        fn non_symbol_chars_standalone() {
            // Characters NOT in the 20-symbol set become Text
            assert_eq!(tokens("^"), vec![Token::Text("^")]);
            assert_eq!(tokens("+"), vec![Token::Text("+")]);
            assert_eq!(tokens("*"), vec![Token::Text("*")]);
            assert_eq!(tokens("."), vec![Token::Dot]);
            assert_eq!(tokens(":"), vec![Token::Colon]);
            assert_eq!(tokens("'"), vec![Token::Text("'")]);
            assert_eq!(tokens(";"), vec![Token::Text(";")]);
            assert_eq!(tokens("|"), vec![Token::Text("|")]);
            assert_eq!(tokens("&"), vec![Token::Text("&")]);
            assert_eq!(tokens("%"), vec![Token::Text("%")]);
        }

        #[test]
        fn walrus_is_colon_then_eq() {
            assert_eq!(tokens(":="), vec![Token::Colon, Token::Eq]);
        }

        #[test]
        fn non_symbol_chars_squash() {
            assert_eq!(tokens(".*+"), vec![Token::Dot, Token::Text("*+")]);
            assert_eq!(tokens("^foo$"), vec![Token::Text("^foo"), Token::Dollar]);
        }

        #[test]
        fn control_chars() {
            assert_eq!(tokens("\x00"), vec![Token::Text("\x00")]);
            assert_eq!(tokens("\x01"), vec![Token::Text("\x01")]);
            assert_eq!(tokens("\x0b"), vec![Token::Text("\x0b")]); // vertical tab
            assert_eq!(tokens("\x0c"), vec![Token::Text("\x0c")]); // form feed
            assert_eq!(tokens("\x7f"), vec![Token::Text("\x7f")]); // DEL
        }

        #[test]
        fn control_chars_squash_with_text() {
            assert_eq!(tokens("a\x00b"), vec![Token::Text("a\x00b")]);
        }

        #[test]
        fn dot_path() {
            // Dot is a symbol, so foo.bar splits
            assert_eq!(
                tokens("foo.bar"),
                vec![Token::Text("foo"), Token::Dot, Token::Text("bar")]
            );
        }

        #[test]
        fn dash_splits_text() {
            // Dash IS a symbol, so foo-bar splits
            assert_eq!(
                tokens("foo-bar"),
                vec![Token::Text("foo"), Token::Dash, Token::Text("bar")]
            );
        }
    }

    // ---------------------------------------------------------
    // Escapes (backslash sequences)
    // ---------------------------------------------------------

    mod escapes {
        use super::*;

        #[test]
        fn escape_n() {
            assert_eq!(tokens(r#"\n"#), vec![Token::Escape("n")]);
        }

        #[test]
        fn escape_r() {
            assert_eq!(tokens(r#"\r"#), vec![Token::Escape("r")]);
        }

        #[test]
        fn escape_t() {
            assert_eq!(tokens(r#"\t"#), vec![Token::Escape("t")]);
        }

        #[test]
        fn escape_zero() {
            assert_eq!(tokens(r#"\0"#), vec![Token::Escape("0")]);
        }

        #[test]
        fn escape_space() {
            assert_eq!(tokens(r#"\ "#), vec![Token::Escape(" ")]);
        }

        #[test]
        fn escape_backslash() {
            assert_eq!(tokens(r#"\\"#), vec![Token::Escape("\\")]);
        }

        #[test]
        fn escape_quote() {
            assert_eq!(tokens(r#"\""#), vec![Token::Escape("\"")]);
        }

        #[test]
        fn escape_does_not_squash_with_following_text() {
            assert_eq!(
                tokens(r#"\nworld"#),
                vec![Token::Escape("n"), Token::Text("world")]
            );
        }

        #[test]
        fn string_with_escape() {
            assert_eq!(
                tokens(r#""hello\nworld""#),
                vec![
                    Token::Quote,
                    Token::Text("hello"),
                    Token::Escape("n"),
                    Token::Text("world"),
                    Token::Quote,
                ]
            );
        }

        #[test]
        fn escape_dollar() {
            assert_eq!(tokens(r#"\$"#), vec![Token::Escape("$")]);
        }

        #[test]
        fn backslash_before_newline() {
            // Real newline is not captured by Escape (regex `.` excludes \n)
            assert_eq!(tokens("\\\n"), vec![Token::Backslash, Token::Newline]);
        }

        #[test]
        fn backslash_before_crlf() {
            // A backslash ending a CRLF line must behave exactly as it does
            // ending an LF line: a bare Backslash, then the terminator.
            assert_eq!(tokens("\\\r\n"), vec![Token::Backslash, Token::Newline]);
            assert_eq!(spans("\\\r\n"), vec![0..1, 1..3]);
        }

        #[test]
        fn backslash_before_lone_cr() {
            // A bare CR is content, so it does not form an Escape either.
            assert_eq!(tokens("\\\ra"), vec![Token::Backslash, Token::Text("\ra")]);
            assert_eq!(spans("\\\ra"), vec![0..1, 1..3]);
        }

        #[test]
        fn bare_backslash_at_eof() {
            assert_eq!(tokens("\\"), vec![Token::Backslash]);
        }

        #[test]
        fn escape_span_covers_both_chars() {
            assert_eq!(spans(r#"\n"#), vec![0..2]);
        }
    }

    // ---------------------------------------------------------
    // Unicode
    // ---------------------------------------------------------

    mod unicode {
        use super::*;

        #[test]
        fn two_byte_text() {
            // U+00E9 LATIN SMALL LETTER E WITH ACUTE (2 bytes)
            assert_eq!(tokens("h\u{00e9}llo"), vec![Token::Text("h\u{00e9}llo")]);
        }

        #[test]
        fn two_byte_spans() {
            // "h\u{00e9}llo" is 6 bytes: h(1) \u{00e9}(2) l(1) l(1) o(1)
            let input = "h\u{00e9}llo";
            assert_eq!(spans(input), vec![0..6]);
        }

        #[test]
        fn four_byte_emoji() {
            // U+1F389 PARTY POPPER (4 bytes)
            let input = "\u{1f389}";
            assert_eq!(tokens(input), vec![Token::Text("\u{1f389}")]);
            assert_eq!(spans(input), vec![0..4]);
        }

        #[test]
        fn cjk() {
            // U+65E5 U+672C U+8A9E = 3 chars, 9 bytes (3 bytes each)
            let input = "\u{65e5}\u{672c}\u{8a9e}";
            assert_eq!(tokens(input), vec![Token::Text(input)]);
            assert_eq!(spans(input), vec![0..9]);
        }

        #[test]
        fn after_symbol() {
            assert_eq!(
                tokens("$caf\u{00e9}"),
                vec![Token::Dollar, Token::Text("caf\u{00e9}")]
            );
        }

        #[test]
        fn between_symbols() {
            let input = "$\u{1f389}!";
            assert_eq!(
                tokens(input),
                vec![Token::Dollar, Token::Text("\u{1f389}"), Token::Bang]
            );
            let sp = spans(input);
            assert_eq!(sp[0], 0..1); // $
            assert_eq!(sp[1], 1..5); // emoji (4 bytes)
            assert_eq!(sp[2], 5..6); // !
        }

        #[test]
        fn squashes_with_ascii() {
            let input = "hi\u{1f389}there";
            assert_eq!(tokens(input), vec![Token::Text(input)]);
        }

        #[test]
        fn homoglyph_keyword() {
            // Cyrillic 'a' (U+0430) looks like Latin 'a' but is different
            // bytes. "\u{0430}s" visually resembles "as" but must NOT match.
            assert_eq!(tokens("\u{0430}s"), vec![Token::Text("\u{0430}s")]);
            // Cyrillic 'e' (U+0435) in "l\u{0435}t" - looks like "let"
            assert_eq!(tokens("l\u{0435}t"), vec![Token::Text("l\u{0435}t")]);
        }

        #[test]
        fn homoglyph_symbol() {
            // Fullwidth dollar sign U+FF04 is NOT the ASCII '$'
            assert_eq!(tokens("\u{ff04}"), vec![Token::Text("\u{ff04}")]);
            // Fullwidth equals U+FF1D is NOT '='
            assert_eq!(tokens("\u{ff1d}"), vec![Token::Text("\u{ff1d}")]);
            // Fullwidth left paren U+FF08 is NOT '('
            assert_eq!(tokens("\u{ff08}"), vec![Token::Text("\u{ff08}")]);
        }
    }

    // ---------------------------------------------------------
    // Strings and docstrings
    // ---------------------------------------------------------

    mod strings {
        use super::*;

        #[test]
        fn empty_quotes() {
            assert_eq!(tokens(r#""""#), vec![Token::Quote, Token::Quote]);
        }

        #[test]
        fn with_interpolation() {
            assert_eq!(
                tokens(r#""prefix${name}suffix""#),
                vec![
                    Token::Quote,
                    Token::Text("prefix"),
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("name"),
                    Token::BraceClose,
                    Token::Text("suffix"),
                    Token::Quote,
                ]
            );
        }

        #[test]
        fn consecutive_interpolations() {
            assert_eq!(
                tokens("${a}${b}"),
                vec![
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("a"),
                    Token::BraceClose,
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("b"),
                    Token::BraceClose,
                ]
            );
        }

        #[test]
        fn capture_ref_bare() {
            assert_eq!(tokens("$1"), vec![Token::Dollar, Token::Text("1")]);
        }

        #[test]
        fn capture_ref_braced() {
            assert_eq!(
                tokens("${1}"),
                vec![
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("1"),
                    Token::BraceClose
                ]
            );
        }

        #[test]
        fn dollar_followed_by_letter() {
            assert_eq!(tokens("$x"), vec![Token::Dollar, Token::Text("x")]);
        }

        #[test]
        fn docstring_delimiters() {
            assert_eq!(
                tokens(
                    r#""""docstring"""
"#
                ),
                vec![
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Text("docstring"),
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn docstring_with_newline() {
            assert_eq!(
                tokens(
                    r#""""line1
line2"""
"#
                ),
                vec![
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Text("line1"),
                    Token::Newline,
                    Token::Text("line2"),
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn docstring_empty() {
            assert_eq!(
                tokens(r#""""""""#),
                vec![
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                    Token::Quote,
                ]
            );
        }
    }

    // ---------------------------------------------------------
    // Regex patterns as token streams
    // ---------------------------------------------------------

    mod regex_patterns {
        use super::*;

        #[test]
        fn character_class() {
            assert_eq!(
                tokens("[0-9]"),
                vec![
                    Token::BracketOpen,
                    Token::Text("0"),
                    Token::Dash,
                    Token::Text("9"),
                    Token::BracketClose,
                ]
            );
        }

        #[test]
        fn character_class_alpha() {
            assert_eq!(
                tokens("[a-zA-Z_]"),
                vec![
                    Token::BracketOpen,
                    Token::Text("a"),
                    Token::Dash,
                    Token::Text("zA"),
                    Token::Dash,
                    Token::Text("Z_"),
                    Token::BracketClose,
                ]
            );
        }

        #[test]
        fn quantifiers() {
            assert_eq!(
                tokens(
                    r#"<? .*
"#
                ),
                vec![
                    Token::Lt,
                    Token::Question,
                    Token::Space(" "),
                    Token::Dot,
                    Token::Text("*"),
                    Token::Newline,
                ]
            );
            assert_eq!(
                tokens(
                    r#"<? [a-z]+
"#
                ),
                vec![
                    Token::Lt,
                    Token::Question,
                    Token::Space(" "),
                    Token::BracketOpen,
                    Token::Text("a"),
                    Token::Dash,
                    Token::Text("z"),
                    Token::BracketClose,
                    Token::Text("+"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn alternation_pipe() {
            assert_eq!(tokens("foo|bar"), vec![Token::Text("foo|bar")]);
        }
    }

    // ---------------------------------------------------------
    // Span accuracy
    // ---------------------------------------------------------

    mod span_accuracy {
        use super::*;

        #[test]
        fn let_string() {
            let input = r#"let x = "hi"
"#;
            let sp = spans(input);
            assert_eq!(sp[0], 0..3); // let
            assert_eq!(sp[1], 3..4); // space
            assert_eq!(sp[2], 4..5); // x
            assert_eq!(sp[3], 5..6); // space
            assert_eq!(sp[4], 6..7); // =
            assert_eq!(sp[5], 7..8); // space
            assert_eq!(sp[6], 8..9); // "
            assert_eq!(sp[7], 9..11); // hi
            assert_eq!(sp[8], 11..12); // "
            assert_eq!(sp[9], 12..13); // \n
        }

        #[test]
        fn empty_input() {
            assert_eq!(tokens(""), Vec::<Token>::new());
        }
    }

    // ---------------------------------------------------------
    // Real-world syntax lines
    // ---------------------------------------------------------

    mod syntax_lines {
        use super::*;

        #[test]
        fn send() {
            assert_eq!(
                tokens(
                    r#"> echo hello
"#
                ),
                vec![
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Space(" "),
                    Token::Text("hello"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn send_raw() {
            assert_eq!(
                tokens(
                    r#"=> partial
"#
                ),
                vec![
                    Token::Eq,
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("partial"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn send_with_interpolation() {
            assert_eq!(
                tokens(
                    r#"> echo ${name}
"#
                ),
                vec![
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Space(" "),
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("name"),
                    Token::BraceClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn match_regex() {
            assert_eq!(
                tokens(
                    r#"<? ^pattern$
"#
                ),
                vec![
                    Token::Lt,
                    Token::Question,
                    Token::Space(" "),
                    Token::Text("^pattern"),
                    Token::Dollar,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn match_literal() {
            assert_eq!(
                tokens(
                    r#"<= substring
"#
                ),
                vec![
                    Token::Lt,
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("substring"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn empty_match_regex() {
            assert_eq!(
                tokens("<?\n"),
                vec![Token::Lt, Token::Question, Token::Newline]
            );
        }

        #[test]
        fn fail_regex() {
            assert_eq!(
                tokens(
                    r#"!? error
"#
                ),
                vec![
                    Token::Bang,
                    Token::Question,
                    Token::Space(" "),
                    Token::Text("error"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn fail_literal() {
            assert_eq!(
                tokens(
                    r#"!= error
"#
                ),
                vec![
                    Token::Bang,
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("error"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn empty_fail_pattern() {
            assert_eq!(
                tokens("!?\n"),
                vec![Token::Bang, Token::Question, Token::Newline]
            );
        }

        #[test]
        fn timed_match() {
            assert_eq!(
                tokens(
                    r#"<~5s?
"#
                ),
                vec![
                    Token::Lt,
                    Token::Tilde,
                    Token::Text("5s"),
                    Token::Question,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_match_with_payload() {
            assert_eq!(
                tokens(
                    r#"<~5s? ^pattern$
"#
                ),
                vec![
                    Token::Lt,
                    Token::Tilde,
                    Token::Text("5s"),
                    Token::Question,
                    Token::Space(" "),
                    Token::Text("^pattern"),
                    Token::Dollar,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_match_long_duration() {
            assert_eq!(
                tokens(
                    r#"<~2h30m12s?
"#
                ),
                vec![
                    Token::Lt,
                    Token::Tilde,
                    Token::Text("2h30m12s"),
                    Token::Question,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_assert() {
            assert_eq!(
                tokens(
                    r#"<@2s=
"#
                ),
                vec![
                    Token::Lt,
                    Token::At,
                    Token::Text("2s"),
                    Token::Eq,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_assert_with_payload() {
            assert_eq!(
                tokens(
                    r#"<@3s= expected output
"#
                ),
                vec![
                    Token::Lt,
                    Token::At,
                    Token::Text("3s"),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("expected"),
                    Token::Space(" "),
                    Token::Text("output"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn let_string() {
            assert_eq!(
                tokens(
                    r#"let x = "hello"
"#
                ),
                vec![
                    Token::Let,
                    Token::Space(" "),
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("hello"),
                    Token::Quote,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn let_uninitialized() {
            assert_eq!(
                tokens(
                    r#"let x
"#
                ),
                vec![
                    Token::Let,
                    Token::Space(" "),
                    Token::Text("x"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn assign() {
            assert_eq!(
                tokens(
                    r#"x = "value"
"#
                ),
                vec![
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("value"),
                    Token::Quote,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn import_path() {
            assert_eq!(
                tokens(
                    r#"import lib/utils
"#
                ),
                vec![
                    Token::Import,
                    Token::Space(" "),
                    Token::Text("lib"),
                    Token::Slash,
                    Token::Text("utils"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn import_selective() {
            assert_eq!(
                tokens(
                    r#"import lib/m { foo, Bar }
"#
                ),
                vec![
                    Token::Import,
                    Token::Space(" "),
                    Token::Text("lib"),
                    Token::Slash,
                    Token::Text("m"),
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("Bar"),
                    Token::Space(" "),
                    Token::BraceClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn import_with_alias() {
            assert_eq!(
                tokens(
                    r#"import lib/m { foo as bar, Db as D }
"#
                ),
                vec![
                    Token::Import,
                    Token::Space(" "),
                    Token::Text("lib"),
                    Token::Slash,
                    Token::Text("m"),
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::Space(" "),
                    Token::As,
                    Token::Space(" "),
                    Token::Text("bar"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("Db"),
                    Token::Space(" "),
                    Token::As,
                    Token::Space(" "),
                    Token::Text("D"),
                    Token::Space(" "),
                    Token::BraceClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn import_trailing_comma() {
            assert_eq!(
                tokens(
                    r#"import m { foo, }
"#
                ),
                vec![
                    Token::Import,
                    Token::Space(" "),
                    Token::Text("m"),
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::BraceClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn fn_def() {
            assert_eq!(
                tokens(
                    r#"fn foo(a, b) {
"#
                ),
                vec![
                    Token::Fn,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::ParenOpen,
                    Token::Text("a"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("b"),
                    Token::ParenClose,
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn fn_zero_params() {
            assert_eq!(
                tokens(
                    r#"fn foo() {
"#
                ),
                vec![
                    Token::Fn,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::ParenOpen,
                    Token::ParenClose,
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn pure_fn() {
            assert_eq!(
                tokens(
                    r#"pure fn add(a, b) {
"#
                ),
                vec![
                    Token::Pure,
                    Token::Space(" "),
                    Token::Fn,
                    Token::Space(" "),
                    Token::Text("add"),
                    Token::ParenOpen,
                    Token::Text("a"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("b"),
                    Token::ParenClose,
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn effect_head() {
            assert_eq!(
                tokens(
                    r#"effect StartDb -> db {
"#
                ),
                vec![
                    Token::Effect,
                    Token::Space(" "),
                    Token::Text("StartDb"),
                    Token::Space(" "),
                    Token::Dash,
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("db"),
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn test_def() {
            // Note: "test" inside the string is recognized as a keyword
            // because it's followed by `"` (not an ident char). The parser
            // handles this - inside string context, keywords are just content.
            assert_eq!(
                tokens(
                    r#"test "my test" {
"#
                ),
                vec![
                    Token::Test,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("my"),
                    Token::Space(" "),
                    Token::Test,
                    Token::Quote,
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn shell_block() {
            assert_eq!(
                tokens(
                    r#"shell s {
"#
                ),
                vec![
                    Token::Shell,
                    Token::Space(" "),
                    Token::Text("s"),
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn cleanup_block() {
            assert_eq!(
                tokens(
                    r#"cleanup {
"#
                ),
                vec![
                    Token::Cleanup,
                    Token::Space(" "),
                    Token::BraceOpen,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn start_with_alias() {
            assert_eq!(
                tokens(
                    r#"start Db as db
"#
                ),
                vec![
                    Token::Start,
                    Token::Space(" "),
                    Token::Text("Db"),
                    Token::Space(" "),
                    Token::As,
                    Token::Space(" "),
                    Token::Text("db"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn start_overlay() {
            assert_eq!(
                tokens(
                    r#"start E(x = "v")
"#
                ),
                vec![
                    Token::Start,
                    Token::Space(" "),
                    Token::Text("E"),
                    Token::ParenOpen,
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("v"),
                    Token::Quote,
                    Token::ParenClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn start_multi_overlay() {
            assert_eq!(
                tokens(
                    r#"start E(x = "a", y = "b")
"#
                ),
                vec![
                    Token::Start,
                    Token::Space(" "),
                    Token::Text("E"),
                    Token::ParenOpen,
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("a"),
                    Token::Quote,
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("y"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("b"),
                    Token::Quote,
                    Token::ParenClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timeout_tolerance() {
            assert_eq!(
                tokens(
                    r#"~10s
"#
                ),
                vec![Token::Tilde, Token::Text("10s"), Token::Newline]
            );
        }

        #[test]
        fn timeout_assertion() {
            assert_eq!(
                tokens(
                    r#"@5s
"#
                ),
                vec![Token::At, Token::Text("5s"), Token::Newline]
            );
        }

        #[test]
        fn comment() {
            assert_eq!(
                tokens(
                    r#"// comment text
"#
                ),
                vec![
                    Token::Slash,
                    Token::Slash,
                    Token::Space(" "),
                    Token::Text("comment"),
                    Token::Space(" "),
                    Token::Text("text"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn marker() {
            assert_eq!(
                tokens(
                    r#"# flaky
"#
                ),
                vec![
                    Token::Hash,
                    Token::Space(" "),
                    Token::Text("flaky"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn marker_skip_if() {
            assert_eq!(
                tokens(
                    r#"# skip if "${CI}" = "true"
"#
                ),
                vec![
                    Token::Hash,
                    Token::Space(" "),
                    Token::Text("skip"),
                    Token::Space(" "),
                    Token::Text("if"),
                    Token::Space(" "),
                    Token::Quote,
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("CI"),
                    Token::BraceClose,
                    Token::Quote,
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("true"),
                    Token::Quote,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn marker_regex_condition() {
            assert_eq!(
                tokens(
                    r#"# skip unless "${ARCH}" ? ^x86_64$
"#
                ),
                vec![
                    Token::Hash,
                    Token::Space(" "),
                    Token::Text("skip"),
                    Token::Space(" "),
                    Token::Text("unless"),
                    Token::Space(" "),
                    Token::Quote,
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("ARCH"),
                    Token::BraceClose,
                    Token::Quote,
                    Token::Space(" "),
                    Token::Question,
                    Token::Space(" "),
                    Token::Text("^x86_64"),
                    Token::Dollar,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn var_interpolation() {
            assert_eq!(
                tokens(
                    r#"${name}
"#
                ),
                vec![
                    Token::Dollar,
                    Token::BraceOpen,
                    Token::Text("name"),
                    Token::BraceClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn function_call() {
            assert_eq!(
                tokens(
                    r#"sleep("2s")
"#
                ),
                vec![
                    Token::Text("sleep"),
                    Token::ParenOpen,
                    Token::Quote,
                    Token::Text("2s"),
                    Token::Quote,
                    Token::ParenClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn function_call_multi_arg() {
            assert_eq!(
                tokens(
                    r#"foo(a, "b", c)
"#
                ),
                vec![
                    Token::Text("foo"),
                    Token::ParenOpen,
                    Token::Text("a"),
                    Token::Comma,
                    Token::Space(" "),
                    Token::Quote,
                    Token::Text("b"),
                    Token::Quote,
                    Token::Comma,
                    Token::Space(" "),
                    Token::Text("c"),
                    Token::ParenClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn indented() {
            assert_eq!(
                tokens(
                    r#"    > echo
"#
                ),
                vec![
                    Token::Space("    "),
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn tab_indented() {
            assert_eq!(
                tokens("\t\t> echo\n"),
                vec![
                    Token::Tab("\t\t"),
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn closing_brace() {
            assert_eq!(tokens("}\n"), vec![Token::BraceClose, Token::Newline]);
        }

        #[test]
        fn blank_lines_between_content() {
            assert_eq!(
                tokens(
                    r#"fn


let
"#
                ),
                vec![
                    Token::Fn,
                    Token::Newline,
                    Token::Newline,
                    Token::Newline,
                    Token::Let,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_literal_match() {
            assert_eq!(
                tokens(
                    r#"<~500ms= expected
"#
                ),
                vec![
                    Token::Lt,
                    Token::Tilde,
                    Token::Text("500ms"),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("expected"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn timed_literal_match_long_duration() {
            assert_eq!(
                tokens(
                    r#"<~2h30m12s= text
"#
                ),
                vec![
                    Token::Lt,
                    Token::Tilde,
                    Token::Text("2h30m12s"),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("text"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn bare_start() {
            assert_eq!(
                tokens(
                    r#"start StartDb
"#
                ),
                vec![
                    Token::Start,
                    Token::Space(" "),
                    Token::Text("StartDb"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn escaped_dollar_in_send() {
            assert_eq!(
                tokens(
                    r#"> echo $$HOME
"#
                ),
                vec![
                    Token::Gt,
                    Token::Space(" "),
                    Token::Text("echo"),
                    Token::Space(" "),
                    Token::Dollar,
                    Token::Dollar,
                    Token::Text("HOME"),
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn let_with_function_call() {
            assert_eq!(
                tokens(
                    r#"let x = foo("a")
"#
                ),
                vec![
                    Token::Let,
                    Token::Space(" "),
                    Token::Text("x"),
                    Token::Space(" "),
                    Token::Eq,
                    Token::Space(" "),
                    Token::Text("foo"),
                    Token::ParenOpen,
                    Token::Quote,
                    Token::Text("a"),
                    Token::Quote,
                    Token::ParenClose,
                    Token::Newline,
                ]
            );
        }

        #[test]
        fn symbol_before_keyword() {
            assert_eq!(tokens("(fn"), vec![Token::ParenOpen, Token::Fn]);
            assert_eq!(tokens("{let"), vec![Token::BraceOpen, Token::Let]);
            assert_eq!(tokens(",as"), vec![Token::Comma, Token::As]);
        }

        #[test]
        fn tab_between_tokens() {
            assert_eq!(
                tokens("fn\tfoo"),
                vec![Token::Fn, Token::Tab("\t"), Token::Text("foo")]
            );
        }

        #[test]
        fn dash_double() {
            assert_eq!(
                tokens("--verbose"),
                vec![Token::Dash, Token::Dash, Token::Text("verbose")]
            );
        }

        #[test]
        fn dash_number() {
            assert_eq!(tokens("-5"), vec![Token::Dash, Token::Text("5")]);
        }
    }

    // ---------------------------------------------------------
    // Integration: multi-line module
    // ---------------------------------------------------------

    mod integration {
        use super::*;

        #[test]
        fn multiline_module() {
            let input = r#"import lib/db

fn setup() {
    > echo ready
    <? ^ready$
}

test "basic" {
    shell s {
        > echo hello
        <= hello
    }
}
"#;
            let toks = tokens(input);

            assert_eq!(toks[0], Token::Import);
            assert_eq!(toks[2], Token::Text("lib"));
            assert_eq!(toks[3], Token::Slash);
            assert_eq!(toks[4], Token::Text("db"));
            assert_eq!(toks[5], Token::Newline);
            assert_eq!(toks[6], Token::Newline); // blank line
            assert_eq!(toks[7], Token::Fn);

            // Find "test" keyword
            let test_pos = toks.iter().position(|t| *t == Token::Test).unwrap();
            assert_eq!(toks[test_pos + 2], Token::Quote);

            // Verify no empty spans
            let sp = spans(input);
            for (i, s) in sp.iter().enumerate() {
                assert!(s.start < s.end, "empty span at token {i}: {s:?}");
            }

            // Verify spans are contiguous
            for i in 1..sp.len() {
                assert_eq!(
                    sp[i - 1].end,
                    sp[i].start,
                    "gap between token {} and {}: {:?} vs {:?}",
                    i - 1,
                    i,
                    sp[i - 1],
                    sp[i]
                );
            }

            // Verify spans cover the entire input
            assert_eq!(sp.first().unwrap().start, 0);
            assert_eq!(sp.last().unwrap().end, input.len());
        }

        #[test]
        fn multiline_module_crlf() {
            let lf = r#"import lib/db

fn setup() {
    > echo ready
    <? ^ready$
}

test "basic" {
    shell s {
        > echo hello
        <= hello
    }
}
"#;
            // Synthesized, never committed as a file: with no .gitattributes,
            // a checked-in CRLF fixture is at the mercy of core.autocrlf.
            let crlf = lf.replace('\n', "\r\n");

            // Same token kinds, in the same order.
            assert_eq!(tokens(&crlf), tokens(lf));

            let sp = spans(&crlf);

            // No empty spans.
            for (i, s) in sp.iter().enumerate() {
                assert!(s.start < s.end, "empty span at token {i}: {s:?}");
            }

            // Contiguous.
            for i in 1..sp.len() {
                assert_eq!(
                    sp[i - 1].end,
                    sp[i].start,
                    "gap between token {} and {}: {:?} vs {:?}",
                    i - 1,
                    i,
                    sp[i - 1],
                    sp[i]
                );
            }

            // Covering the whole input.
            assert_eq!(sp.first().unwrap().start, 0);
            assert_eq!(sp.last().unwrap().end, crlf.len());

            for s in lex(&crlf) {
                if matches!(s.node, Token::Newline) {
                    assert_eq!(
                        s.span.end() - s.span.start(),
                        2,
                        "CRLF newline must be 2 bytes"
                    );
                }
            }
        }
    }
}
