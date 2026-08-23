//! The CST vocabulary: one leaf kind per `relux_lexer::Token`, one node kind
//! per AST construct, and the `rowan::Language` binding that ties them to a
//! syntax tree.
//!
//! Leaves occupy discriminants `0..LEAF_COUNT` by construction, which is what
//! makes `is_leaf` a comparison and `from_u16` a slice index.

use relux_lexer::Token;

macro_rules! syntax_kinds {
    (leaves: [$($leaf:ident),* $(,)?], nodes: [$($node:ident),* $(,)?] $(,)?) => {
        /// Every kind a CST element can have.
        ///
        /// Leaf kinds map 1:1 onto `relux_lexer::Token`. Node kinds mirror the
        /// AST vocabulary in `relux-ast`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        #[allow(non_camel_case_types)]
        pub enum SyntaxKind {
            $($leaf,)*
            $($node,)*
            /// A node covering text the parser could not fit into the grammar.
            ERROR,
            /// Placeholder written by the parser's marker machinery. Never
            /// present in a finished tree.
            TOMBSTONE,
        }

        impl SyntaxKind {
            /// Leaves occupy `0..LEAF_COUNT`.
            const LEAF_COUNT: usize = [$(stringify!($leaf)),*].len();

            /// Every kind, in discriminant order.
            pub const ALL: &'static [SyntaxKind] = &[
                $(SyntaxKind::$leaf,)*
                $(SyntaxKind::$node,)*
                SyntaxKind::ERROR,
                SyntaxKind::TOMBSTONE,
            ];
        }
    };
}

syntax_kinds! {
    leaves: [
        // Keywords -- 1:1 with the keyword tokens.
        FN_KW, PURE_KW, EFFECT_KW, TEST_KW, SHELL_KW, LET_KW, START_KW,
        EXPECT_KW, EXPOSE_KW, VAR_KW, IMPORT_KW, CLEANUP_KW, AS_KW,

        // Defined for the 1:1 mapping, but never present in a tree: `lex()`
        // rewrites every `Word` into `Text` before returning
        // (relux-lexer/src/lib.rs:186). T03 asserts on this.
        WORD,

        // Symbols.
        DOLLAR, L_BRACE, R_BRACE, L_PAREN, R_PAREN, QUOTE, LT, GT, EQ, BANG,
        QUESTION, TILDE, AT, BACKSLASH, ESCAPE, HASH, L_BRACKET, R_BRACKET,
        COMMA, SLASH, DASH, DOT, COLON,

        // Whitespace. These are real leaves, not rowan trivia -- see the
        // story doc. NEWLINE covers `\n` or `\r\n`, so it is one byte or two.
        SPACE, TAB, NEWLINE,

        // Everything else, including unmatched bytes.
        TEXT,
    ],
    nodes: [
        MODULE,

        IMPORT, IMPORT_NAME_LIST, IMPORT_NAME,

        FN_DEF, PURE_FN_DEF, PARAM_LIST,
        EFFECT_DEF, TEST_DEF,

        EXPECT_DECL, EXPOSE_DECL, START_DECL, OVERLAY, OVERLAY_ENTRY,
        SHELL_BLOCK, CLEANUP_BLOCK,

        MARKER_DECL, MARKER_COND,
        COMMENT, DOCSTRING,

        SEND_STMT, SEND_RAW_STMT, MATCH_STMT, FAIL_STMT, TIMED_MATCH_STMT,
        MULTIMATCH_STMT, MULTIMATCH_ARM,
        LET_STMT, ASSIGN_STMT, TIMEOUT_STMT, EXPR_STMT, PURE_MATCH_STMT,

        CALL_EXPR, ARG_LIST, STRING_EXPR, CAPTURE_REF, QUALIFIED_VAR,
        VAR_EXPR, NUMERIC_EXPR, PLAIN_STRING,

        INTERPOLATION, INTERP_VAR_REF, INTERP_QUALIFIED_VAR_REF,
        INTERP_CAPTURE_REF, INTERP_ESCAPED_DOLLAR, INTERP_ESCAPE,
        INTERP_LITERAL,

        TIMEOUT,

        IDENT_VAR, IDENT_FN, IDENT_EFFECT, IDENT_MODULE,
    ],
}

impl SyntaxKind {
    /// Recover a kind from its raw discriminant. `None` if out of range.
    pub fn from_u16(raw: u16) -> Option<SyntaxKind> {
        Self::ALL.get(raw as usize).copied()
    }

    /// True for kinds that appear as tokens rather than as interior nodes.
    /// `ERROR` and `TOMBSTONE` are not leaves.
    pub fn is_leaf(self) -> bool {
        (self as usize) < Self::LEAF_COUNT
    }

    /// Whitespace is a token here, not trivia, so consumers must skip it
    /// explicitly when walking children.
    pub fn is_whitespace(self) -> bool {
        matches!(
            self,
            SyntaxKind::SPACE | SyntaxKind::TAB | SyntaxKind::NEWLINE
        )
    }

    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            SyntaxKind::FN_KW
                | SyntaxKind::PURE_KW
                | SyntaxKind::EFFECT_KW
                | SyntaxKind::TEST_KW
                | SyntaxKind::SHELL_KW
                | SyntaxKind::LET_KW
                | SyntaxKind::START_KW
                | SyntaxKind::EXPECT_KW
                | SyntaxKind::EXPOSE_KW
                | SyntaxKind::VAR_KW
                | SyntaxKind::IMPORT_KW
                | SyntaxKind::CLEANUP_KW
                | SyntaxKind::AS_KW
        )
    }
}

/// The rowan language marker for Relux. Uninhabited: it is a type-level tag,
/// never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReluxLanguage {}

impl rowan::Language for ReluxLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from_u16(raw.0)
            .unwrap_or_else(|| panic!("unknown SyntaxKind discriminant: {}", raw.0))
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<ReluxLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<ReluxLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<ReluxLanguage>;

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

/// Map a lexer token to its leaf kind. Exhaustive by construction.
pub fn kind_of(token: &Token<'_>) -> SyntaxKind {
    match token {
        Token::Fn => SyntaxKind::FN_KW,
        Token::Pure => SyntaxKind::PURE_KW,
        Token::Effect => SyntaxKind::EFFECT_KW,
        Token::Test => SyntaxKind::TEST_KW,
        Token::Shell => SyntaxKind::SHELL_KW,
        Token::Let => SyntaxKind::LET_KW,
        Token::Start => SyntaxKind::START_KW,
        Token::Expect => SyntaxKind::EXPECT_KW,
        Token::Expose => SyntaxKind::EXPOSE_KW,
        Token::Var => SyntaxKind::VAR_KW,
        Token::Import => SyntaxKind::IMPORT_KW,
        Token::Cleanup => SyntaxKind::CLEANUP_KW,
        Token::As => SyntaxKind::AS_KW,
        Token::Word(_) => SyntaxKind::WORD,
        Token::Dollar => SyntaxKind::DOLLAR,
        Token::BraceOpen => SyntaxKind::L_BRACE,
        Token::BraceClose => SyntaxKind::R_BRACE,
        Token::ParenOpen => SyntaxKind::L_PAREN,
        Token::ParenClose => SyntaxKind::R_PAREN,
        Token::Quote => SyntaxKind::QUOTE,
        Token::Lt => SyntaxKind::LT,
        Token::Gt => SyntaxKind::GT,
        Token::Eq => SyntaxKind::EQ,
        Token::Bang => SyntaxKind::BANG,
        Token::Question => SyntaxKind::QUESTION,
        Token::Tilde => SyntaxKind::TILDE,
        Token::At => SyntaxKind::AT,
        Token::Backslash => SyntaxKind::BACKSLASH,
        Token::Escape(_) => SyntaxKind::ESCAPE,
        Token::Hash => SyntaxKind::HASH,
        Token::BracketOpen => SyntaxKind::L_BRACKET,
        Token::BracketClose => SyntaxKind::R_BRACKET,
        Token::Comma => SyntaxKind::COMMA,
        Token::Slash => SyntaxKind::SLASH,
        Token::Dash => SyntaxKind::DASH,
        Token::Dot => SyntaxKind::DOT,
        Token::Colon => SyntaxKind::COLON,
        Token::Space(_) => SyntaxKind::SPACE,
        Token::Tab(_) => SyntaxKind::TAB,
        Token::Newline => SyntaxKind::NEWLINE,
        Token::Text(_) => SyntaxKind::TEXT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Token` variant, once. Kept in the same order as the enum in
    /// `relux-lexer` so a diff against it is readable.
    fn every_token() -> Vec<Token<'static>> {
        vec![
            Token::Fn,
            Token::Pure,
            Token::Effect,
            Token::Test,
            Token::Shell,
            Token::Let,
            Token::Start,
            Token::Expect,
            Token::Expose,
            Token::Var,
            Token::Import,
            Token::Cleanup,
            Token::As,
            Token::Word("w"),
            Token::Dollar,
            Token::BraceOpen,
            Token::BraceClose,
            Token::ParenOpen,
            Token::ParenClose,
            Token::Quote,
            Token::Lt,
            Token::Gt,
            Token::Eq,
            Token::Bang,
            Token::Question,
            Token::Tilde,
            Token::At,
            Token::Backslash,
            Token::Escape("n"),
            Token::Hash,
            Token::BracketOpen,
            Token::BracketClose,
            Token::Comma,
            Token::Slash,
            Token::Dash,
            Token::Dot,
            Token::Colon,
            Token::Space(" "),
            Token::Tab("\t"),
            Token::Newline,
            Token::Text("t"),
        ]
    }

    #[test]
    fn every_token_maps_to_a_distinct_leaf() {
        let kinds: Vec<SyntaxKind> = every_token().iter().map(kind_of).collect();

        let mut unique = kinds.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            kinds.len(),
            "kind_of is not injective: two tokens share a leaf kind"
        );

        for kind in &kinds {
            assert!(kind.is_leaf(), "{kind:?} is not classified as a leaf");
        }
    }

    #[test]
    fn leaf_count_matches_the_token_enum() {
        // If this fails, `leaves:` and `Token` have drifted apart: either a
        // token was added without a kind, or a kind was added that no token
        // produces.
        assert_eq!(SyntaxKind::LEAF_COUNT, every_token().len());
    }

    #[test]
    fn nodes_and_specials_are_not_leaves() {
        assert!(!SyntaxKind::MODULE.is_leaf());
        assert!(!SyntaxKind::TEST_DEF.is_leaf());
        assert!(!SyntaxKind::ERROR.is_leaf());
        assert!(!SyntaxKind::TOMBSTONE.is_leaf());
    }

    #[test]
    fn raw_conversion_round_trips_for_every_kind() {
        use rowan::Language;

        for &kind in SyntaxKind::ALL {
            let raw = ReluxLanguage::kind_to_raw(kind);
            assert_eq!(ReluxLanguage::kind_from_raw(raw), kind, "{kind:?}");
        }
    }

    #[test]
    fn from_u16_rejects_out_of_range() {
        assert_eq!(SyntaxKind::from_u16(0), Some(SyntaxKind::ALL[0]));
        assert_eq!(SyntaxKind::from_u16(SyntaxKind::ALL.len() as u16), None);
        assert_eq!(SyntaxKind::from_u16(u16::MAX), None);
    }

    #[test]
    fn all_is_in_discriminant_order() {
        for (i, &kind) in SyntaxKind::ALL.iter().enumerate() {
            assert_eq!(kind as usize, i, "{kind:?} is out of order in ALL");
        }
    }

    #[test]
    fn whitespace_and_keyword_classification() {
        assert!(SyntaxKind::SPACE.is_whitespace());
        assert!(SyntaxKind::TAB.is_whitespace());
        assert!(SyntaxKind::NEWLINE.is_whitespace());
        assert!(!SyntaxKind::TEXT.is_whitespace());

        assert!(SyntaxKind::FN_KW.is_keyword());
        assert!(SyntaxKind::AS_KW.is_keyword());
        assert!(!SyntaxKind::WORD.is_keyword());
        assert!(!SyntaxKind::MODULE.is_keyword());
    }
}
