use logos::Logos;
use std::fmt;

#[derive(Debug, PartialEq, Clone)]
pub enum PayloadFragment<'a> {
    Text(&'a str),
    Interpolation(&'a str),
    EscapedDollar,
}

#[derive(Debug, PartialEq, Clone)]
pub enum StringFragment<'a> {
    Text(&'a str),
    Interpolation(&'a str),
    Escape(&'a str),
}

#[derive(Debug, PartialEq, Clone)]
pub enum MarkerKind {
    Skip,
    Run,
    Flaky,
}

#[derive(Debug, PartialEq, Clone)]
pub enum MarkerModifier {
    If,
    Unless,
}

#[derive(Debug, PartialEq, Clone)]
pub enum MarkerExpr<'a> {
    String(Vec<StringFragment<'a>>),
    Number(&'a str),
    Var(&'a str),
    Call(&'a str, Vec<MarkerExpr<'a>>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum MarkerCondBody<'a> {
    Bare(MarkerExpr<'a>),
    Eq(MarkerExpr<'a>, MarkerExpr<'a>),
    Regex(MarkerExpr<'a>, Vec<PayloadFragment<'a>>),
}

#[derive(Debug, PartialEq, Clone)]
pub struct MarkerCondition<'a> {
    pub modifier: MarkerModifier,
    pub body: MarkerCondBody<'a>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct MarkerToken<'a> {
    pub kind: MarkerKind,
    pub condition: Option<MarkerCondition<'a>>,
}

#[derive(Logos, PartialEq, Clone)]
#[logos(skip r" +")]
pub enum Token<'a> {
    #[regex(r"#[^\n]*", |lex| &lex.slice()[1..], priority = 10, allow_greedy = true)]
    Comment(&'a str),

    #[token("import")]
    Import,
    #[token("as")]
    As,
    #[token("pure")]
    Pure,
    #[token("fn")]
    Fn,
    #[token("effect")]
    Effect,
    #[token("test")]
    Test,
    #[token("shell")]
    Shell,
    #[token("let")]
    Let,
    #[token("need")]
    Need,
    #[token("cleanup")]
    Cleanup,

    #[regex(r"\[[^\n]*\]", super::lex_marker, allow_greedy = true)]
    Marker(MarkerToken<'a>),

    #[token("{")]
    BraceOpen,
    #[token("}")]
    BraceClose,
    #[token("(")]
    ParenOpen,
    #[token(")")]
    ParenClose,
    #[token(",")]
    Comma,
    #[token("->")]
    Arrow,
    #[token("=")]
    Eq,

    #[token("=>", super::lex_payload)]
    SendRaw(Vec<PayloadFragment<'a>>),
    #[token(">", super::lex_payload)]
    Send(Vec<PayloadFragment<'a>>),
    #[token("<?", super::lex_payload)]
    MatchRegex(Vec<PayloadFragment<'a>>),
    #[token("<=", super::lex_payload)]
    MatchLiteral(Vec<PayloadFragment<'a>>),
    #[token("!?", super::lex_payload)]
    FailRegex(Vec<PayloadFragment<'a>>),
    #[token("!=", super::lex_payload)]
    FailLiteral(Vec<PayloadFragment<'a>>),

    #[token("<!?", super::lex_payload)]
    NegMatchRegex(Vec<PayloadFragment<'a>>),
    #[token("<!=", super::lex_payload)]
    NegMatchLiteral(Vec<PayloadFragment<'a>>),

    #[regex(r"<~[0-9][0-9a-zA-Z]*\?", super::lex_timed_match_regex)]
    TimedMatchRegex((&'a str, Vec<PayloadFragment<'a>>)),
    #[regex(r"<~[0-9][0-9a-zA-Z]*=", super::lex_timed_match_literal)]
    TimedMatchLiteral((&'a str, Vec<PayloadFragment<'a>>)),
    #[regex(r"<~[0-9][0-9a-zA-Z]*!\?", super::lex_timed_neg_match_regex)]
    TimedNegMatchRegex((&'a str, Vec<PayloadFragment<'a>>)),
    #[regex(r"<~[0-9][0-9a-zA-Z]*!=", super::lex_timed_neg_match_literal)]
    TimedNegMatchLiteral((&'a str, Vec<PayloadFragment<'a>>)),

    #[regex(r"~[0-9][0-9a-zA-Z]*", |lex| &lex.slice()[1..], allow_greedy = true)]
    Timeout(&'a str),

    #[token("\"\"\"", super::lex_docstring)]
    DocString(Vec<&'a str>),

    #[token("\"", super::lex_string)]
    String(Vec<StringFragment<'a>>),

    #[regex(r"\$(\{[a-zA-Z_0-9]+\}|[0-9]+)", |lex| {
        let s = lex.slice();
        if s.as_bytes()[1] == b'{' { &s[2..s.len()-1] } else { &s[1..] }
    })]
    Interpolation(&'a str),

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*(/[a-zA-Z_][a-zA-Z0-9_]*)+")]
    ModulePath(&'a str),

    #[regex(r"[a-z_][a-zA-Z0-9_]*")]
    Ident(&'a str),

    #[regex(r"[A-Z][a-zA-Z0-9_]*")]
    EffectIdent(&'a str),

    #[regex(r"[0-9]+")]
    Number(&'a str),

    #[token("\n")]
    Newline,

    Unrecognized(&'a str),
}

pub type Spanned<'a> = crate::Spanned<Token<'a>>;

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Comment(s) => write!(f, "#{s}"),
            Token::Import => write!(f, "import"),
            Token::As => write!(f, "as"),
            Token::Pure => write!(f, "pure"),
            Token::Fn => write!(f, "fn"),
            Token::Effect => write!(f, "effect"),
            Token::Test => write!(f, "test"),
            Token::Shell => write!(f, "shell"),
            Token::Let => write!(f, "let"),
            Token::Need => write!(f, "need"),
            Token::Cleanup => write!(f, "cleanup"),
            Token::Marker(m) => {
                let kind_str = match m.kind {
                    MarkerKind::Skip => "skip",
                    MarkerKind::Run => "run",
                    MarkerKind::Flaky => "flaky",
                };
                write!(f, "[{kind_str}")?;
                if let Some(ref cond) = m.condition {
                    let mod_str = match cond.modifier {
                        MarkerModifier::If => "if",
                        MarkerModifier::Unless => "unless",
                    };
                    write!(f, " {mod_str} ...")?;
                }
                write!(f, "]")
            }
            Token::BraceOpen => write!(f, "{{"),
            Token::BraceClose => write!(f, "}}"),
            Token::ParenOpen => write!(f, "("),
            Token::ParenClose => write!(f, ")"),
            Token::Comma => write!(f, ","),
            Token::Arrow => write!(f, "->"),
            Token::Eq => write!(f, "="),
            Token::Send(_) => write!(f, ">"),
            Token::SendRaw(_) => write!(f, "=>"),
            Token::MatchRegex(_) => write!(f, "<?"),
            Token::MatchLiteral(_) => write!(f, "<="),
            Token::FailRegex(_) => write!(f, "!?"),
            Token::FailLiteral(_) => write!(f, "!="),
            Token::NegMatchRegex(_) => write!(f, "<!?"),
            Token::NegMatchLiteral(_) => write!(f, "<!="),
            Token::TimedMatchRegex((d, _)) => write!(f, "<~{d}?"),
            Token::TimedMatchLiteral((d, _)) => write!(f, "<~{d}="),
            Token::TimedNegMatchRegex((d, _)) => write!(f, "<~{d}!?"),
            Token::TimedNegMatchLiteral((d, _)) => write!(f, "<~{d}!="),
            Token::Timeout(s) => write!(f, "~{s}"),
            Token::DocString(_) => write!(f, "\"\"\"...\"\"\""),
            Token::String(_) => write!(f, "\"...\""),
            Token::Interpolation(s) => write!(f, "${{{s}}}"),
            Token::ModulePath(s) => write!(f, "{s}"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::EffectIdent(s) => write!(f, "{s}"),
            Token::Number(n) => write!(f, "{n}"),
            Token::Newline => write!(f, "\\n"),
            Token::Unrecognized(s) => write!(f, "<unrecognized: {s}>"),
        }
    }
}

impl fmt::Debug for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Newline => write!(f, "[newline]\n"),
            _ => write!(f, "'{self}' "),
        }
    }
}
