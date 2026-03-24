use std::time::Duration;

use crate::{Span, Spanned};

// ─── AstIdent ───────────────────────────────────────────────

/// Dedicated identifier type, replacing raw `String` for names
/// throughout the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct AstIdent {
    pub name: String,
    pub span: Span,
}

impl AstIdent {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

// ─── Trait + Macros ─────────────────────────────────────────

pub trait AstNode {
    fn span(&self) -> &Span;
}

macro_rules! impl_ast_node_struct {
    ($($ty:ty),* $(,)?) => {
        $(
            impl AstNode for $ty {
                fn span(&self) -> &Span {
                    &self.span
                }
            }
        )*
    };
}

macro_rules! impl_ast_node_enum {
    ($ty:ty { $($variant:ident),* $(,)? }) => {
        impl AstNode for $ty {
            fn span(&self) -> &Span {
                match self {
                    $(Self::$variant { span, .. } => span,)*
                }
            }
        }
    };
}

// ─── Expressions ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AstExpr {
    String {
        interp: AstInterpolation,
        span: Span,
    },
    Var {
        name: String,
        span: Span,
    },
    Call {
        call: AstCallExpr,
        span: Span,
    },
    CaptureRef {
        index: usize,
        span: Span,
    },
}

impl_ast_node_enum!(AstExpr {
    String,
    Var,
    Call,
    CaptureRef
});

#[derive(Debug, Clone, PartialEq)]
pub struct AstInterpolation {
    pub parts: Vec<AstStringPart>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstStringPart {
    Literal { value: String, span: Span },
    VarRef { name: String, span: Span },
    EscapedDollar { span: Span },
    CaptureRef { index: usize, span: Span },
}

impl_ast_node_enum!(AstStringPart {
    Literal,
    VarRef,
    EscapedDollar,
    CaptureRef
});

impl AstExpr {
    /// Deterministic, span-independent string representation.
    /// Two structurally identical expressions at different source positions
    /// produce the same string. Used for overlay deduplication.
    pub fn canonical(&self) -> String {
        match self {
            AstExpr::String { interp, .. } => format!("S:{}", interp.canonical()),
            AstExpr::Var { name, .. } => format!("V:{name}"),
            AstExpr::Call { call, .. } => {
                let args: Vec<String> = call.args.iter().map(|a| a.node.canonical()).collect();
                format!("C:{}({})", call.name.node.name, args.join(","))
            }
            AstExpr::CaptureRef { index, .. } => format!("cap:{index}"),
        }
    }
}

impl AstInterpolation {
    pub(crate) fn canonical(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                AstStringPart::Literal { value, .. } => {
                    out.push_str("L:");
                    out.push_str(value);
                }
                AstStringPart::VarRef { name, .. } => {
                    out.push_str("V:");
                    out.push_str(name);
                }
                AstStringPart::EscapedDollar { .. } => out.push('D'),
                AstStringPart::CaptureRef { index, .. } => {
                    out.push_str("cap:");
                    out.push_str(&index.to_string());
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstCallExpr {
    pub name: Spanned<AstIdent>,
    pub args: Vec<Spanned<AstExpr>>,
    pub span: Span,
}

// ─── Statements ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AstStmt {
    Comment {
        text: String,
        span: Span,
    },
    Let {
        stmt: AstLetStmt,
        span: Span,
    },
    Assign {
        stmt: AstAssignStmt,
        span: Span,
    },
    Timeout {
        timeout: AstTimeout,
        span: Span,
    },
    FailRegex {
        pattern: AstInterpolation,
        span: Span,
    },
    FailLiteral {
        pattern: AstInterpolation,
        span: Span,
    },
    ClearFailPattern {
        span: Span,
    },
    Send {
        payload: AstInterpolation,
        span: Span,
    },
    SendRaw {
        payload: AstInterpolation,
        span: Span,
    },
    MatchRegex {
        pattern: AstInterpolation,
        span: Span,
    },
    MatchLiteral {
        pattern: AstInterpolation,
        span: Span,
    },
    TimedMatchRegex {
        timeout: AstTimeout,
        pattern: Spanned<AstInterpolation>,
        span: Span,
    },
    TimedMatchLiteral {
        timeout: AstTimeout,
        pattern: Spanned<AstInterpolation>,
        span: Span,
    },
    BufferReset {
        span: Span,
    },
    Expr {
        expr: AstExpr,
        span: Span,
    },
}

impl_ast_node_enum!(AstStmt {
    Comment,
    Let,
    Assign,
    Timeout,
    FailRegex,
    FailLiteral,
    ClearFailPattern,
    Send,
    SendRaw,
    MatchRegex,
    MatchLiteral,
    TimedMatchRegex,
    TimedMatchLiteral,
    BufferReset,
    Expr,
});

#[derive(Debug, Clone, PartialEq)]
pub struct AstLetStmt {
    pub name: Spanned<AstIdent>,
    pub value: Option<Spanned<AstExpr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstAssignStmt {
    pub name: Spanned<AstIdent>,
    pub value: Spanned<AstExpr>,
    pub span: Span,
}

// ─── Blocks ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstShellBlock {
    pub name: Spanned<AstIdent>,
    pub stmts: Vec<Spanned<AstStmt>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstCleanupBlock {
    pub stmts: Vec<Spanned<AstStmt>>,
    pub span: Span,
}

// ─── Markers ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstMarkerDecl {
    pub kind: AstMarkerKind,
    pub condition: Option<AstMarkerCond>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstMarkerKind {
    Skip { span: Span },
    Run { span: Span },
    Flaky { span: Span },
}

impl_ast_node_enum!(AstMarkerKind { Skip, Run, Flaky });

#[derive(Debug, Clone, PartialEq)]
pub enum AstCondModifier {
    If { span: Span },
    Unless { span: Span },
}

impl_ast_node_enum!(AstCondModifier { If, Unless });

#[derive(Debug, Clone, PartialEq)]
pub struct AstMarkerCond {
    pub modifier: AstCondModifier,
    pub body: AstMarkerCondBody,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstMarkerCondBody {
    Bare {
        expr: AstExpr,
        span: Span,
    },
    Eq {
        lhs: AstExpr,
        rhs: AstExpr,
        span: Span,
    },
    Regex {
        expr: AstExpr,
        pattern: AstInterpolation,
        span: Span,
    },
}

impl_ast_node_enum!(AstMarkerCondBody { Bare, Eq, Regex });

// ─── Imports ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstImport {
    pub path: Spanned<String>,
    pub names: Option<Vec<Spanned<AstImportName>>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstImportName {
    pub name: Spanned<AstIdent>,
    pub alias: Option<Spanned<AstIdent>>,
    pub span: Span,
}

// ─── Need ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstNeedDecl {
    pub effect: Spanned<AstIdent>,
    pub alias: Option<Spanned<AstIdent>>,
    pub overlay: Vec<Spanned<AstOverlayEntry>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstOverlayEntry {
    pub key: Spanned<AstIdent>,
    pub value: Spanned<AstExpr>,
    pub span: Span,
}

// ─── Function Definitions ───────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstFnDef {
    pub name: Spanned<AstIdent>,
    pub params: Vec<Spanned<AstIdent>>,
    pub markers: Vec<Spanned<AstMarkerDecl>>,
    pub body: Vec<Spanned<AstStmt>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstPureFnDef {
    pub name: Spanned<AstIdent>,
    pub params: Vec<Spanned<AstIdent>>,
    pub markers: Vec<Spanned<AstMarkerDecl>>,
    pub body: Vec<Spanned<AstStmt>>,
    pub span: Span,
}

// ─── Effect Definition ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstEffectDef {
    pub name: Spanned<AstIdent>,
    pub exported_shell: Spanned<AstIdent>,
    pub markers: Vec<Spanned<AstMarkerDecl>>,
    pub body: Vec<Spanned<AstEffectItem>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstEffectItem {
    Comment { text: String, span: Span },
    Need { decl: AstNeedDecl, span: Span },
    Let { stmt: AstLetStmt, span: Span },
    Shell { block: AstShellBlock, span: Span },
    Cleanup { block: AstCleanupBlock, span: Span },
}

impl_ast_node_enum!(AstEffectItem {
    Comment,
    Need,
    Let,
    Shell,
    Cleanup
});

// ─── Test Definition ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstTestDef {
    pub name: Spanned<String>,
    pub timeout: Option<Spanned<AstTimeout>>,
    pub markers: Vec<Spanned<AstMarkerDecl>>,
    pub body: Vec<Spanned<AstTestItem>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstTestItem {
    Comment { text: String, span: Span },
    DocString { text: String, span: Span },
    Need { decl: AstNeedDecl, span: Span },
    Let { stmt: AstLetStmt, span: Span },
    Shell { block: AstShellBlock, span: Span },
    Cleanup { block: AstCleanupBlock, span: Span },
}

impl_ast_node_enum!(AstTestItem {
    Comment,
    DocString,
    Need,
    Let,
    Shell,
    Cleanup
});

// ─── Module ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AstModule {
    pub items: Vec<crate::Spanned<AstItem>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstItem {
    Comment { text: String, span: Span },
    Import { import: AstImport, span: Span },
    Fn { def: AstFnDef, span: Span },
    PureFn { def: AstPureFnDef, span: Span },
    Effect { def: AstEffectDef, span: Span },
    Test { def: AstTestDef, span: Span },
}

impl_ast_node_enum!(AstItem {
    Comment,
    Import,
    Fn,
    PureFn,
    Effect,
    Test
});

// ─── Timeout ────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone)]
pub enum AstTimeout {
    Tolerance { duration: Duration, span: Span },
    Assertion { duration: Duration, span: Span },
}

impl AstTimeout {
    pub fn duration(&self) -> Duration {
        match self {
            AstTimeout::Tolerance { duration, .. } => *duration,
            AstTimeout::Assertion { duration, .. } => *duration,
        }
    }
}

impl_ast_node_enum!(AstTimeout {
    Tolerance,
    Assertion
});

// ─── Macro Impls ────────────────────────────────────────────

impl_ast_node_struct!(
    AstIdent,
    AstInterpolation,
    AstCallExpr,
    AstLetStmt,
    AstAssignStmt,
    AstShellBlock,
    AstCleanupBlock,
    AstMarkerDecl,
    AstMarkerCond,
    AstImport,
    AstImportName,
    AstNeedDecl,
    AstOverlayEntry,
    AstFnDef,
    AstPureFnDef,
    AstEffectDef,
    AstTestDef,
    AstModule,
);
