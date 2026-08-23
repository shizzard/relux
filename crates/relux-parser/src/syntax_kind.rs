//! The CST vocabulary: one leaf kind per `relux_lexer::Token`, one node kind
//! per AST construct, and the `rowan::Language` binding that ties them to a
//! syntax tree.
//!
//! Leaves occupy discriminants `0..LEAF_COUNT` by construction, which is what
//! makes `is_leaf` a comparison and `from_u16` a slice index.

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
