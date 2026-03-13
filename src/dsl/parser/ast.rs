pub type Span = std::ops::Range<usize>;

use crate::Spanned;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TimeoutKind {
    Tolerance,
    Assertion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub items: Vec<Spanned<Item>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Comment(String),
    Import(Import),
    Fn(FnDef),
    PureFn(PureFnDef),
    Effect(EffectDef),
    Test(TestDef),
    Marker(MarkerDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: Spanned<String>,
    pub names: Option<Vec<Spanned<ImportName>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportName {
    pub name: Spanned<String>,
    pub alias: Option<Spanned<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: Spanned<String>,
    pub params: Vec<Spanned<String>>,
    pub body: Vec<Spanned<Stmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectDef {
    pub name: Spanned<String>,
    pub exported_shell: Spanned<String>,
    pub markers: Vec<Spanned<MarkerDecl>>,
    pub body: Vec<Spanned<EffectItem>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectItem {
    Comment(String),
    Need(NeedDecl),
    Let(PureLetStmt),
    Shell(ShellBlock),
    Cleanup(CleanupBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestDef {
    pub name: Spanned<String>,
    pub timeout: Option<Spanned<(TimeoutKind, String)>>,
    pub markers: Vec<Spanned<MarkerDecl>>,
    pub body: Vec<Spanned<TestItem>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestItem {
    Comment(String),
    DocString(String),
    Need(NeedDecl),
    Let(PureLetStmt),
    Shell(ShellBlock),
    Cleanup(CleanupBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkerDecl {
    pub kind: MarkerKind,
    pub condition: Option<AstMarkerCond>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarkerKind {
    Skip,
    Run,
    Flaky,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CondModifier {
    If,
    Unless,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstMarkerCond {
    pub modifier: CondModifier,
    pub body: AstMarkerCondBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstMarkerCondBody {
    Bare(PureAstExpr),
    Eq(PureAstExpr, PureAstExpr),
    Regex(PureAstExpr, AstStringExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NeedDecl {
    pub effect: Spanned<String>,
    pub alias: Option<Spanned<String>>,
    pub overlay: Vec<Spanned<OverlayEntry>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayEntry {
    pub key: Spanned<String>,
    pub value: Spanned<PureAstExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellBlock {
    pub name: Spanned<String>,
    pub stmts: Vec<Spanned<Stmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanupBlock {
    pub stmts: Vec<Spanned<CleanupStmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Comment(String),
    Let(LetStmt),
    Assign(AssignStmt),
    Timeout(TimeoutKind, String),
    FailRegex(AstStringExpr),
    FailLiteral(AstStringExpr),
    ClearFailPattern,
    Expr(AstExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanupStmt {
    Comment(String),
    Send(AstStringExpr),
    SendRaw(AstStringExpr),
    Let(LetStmt),
    Assign(AssignStmt),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub name: Spanned<String>,
    pub value: Option<Spanned<AstExpr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignStmt {
    pub name: Spanned<String>,
    pub value: Spanned<AstExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstExpr {
    String(AstStringExpr),
    Var(String),
    Call(CallExpr),
    Send(AstStringExpr),
    SendRaw(AstStringExpr),
    MatchRegex(AstStringExpr),
    MatchLiteral(AstStringExpr),
    NegMatchRegex(AstStringExpr),
    NegMatchLiteral(AstStringExpr),
    TimedMatchRegex(TimeoutKind, String, AstStringExpr),
    TimedMatchLiteral(TimeoutKind, String, AstStringExpr),
    TimedNegMatchRegex(TimeoutKind, String, AstStringExpr),
    TimedNegMatchLiteral(TimeoutKind, String, AstStringExpr),
    BufferReset,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstStringExpr {
    pub parts: Vec<AstStringPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstStringPart {
    Literal(String),
    Interp(String),
    Escape(String),
    EscapedDollar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    pub name: Spanned<String>,
    pub args: Vec<Spanned<AstExpr>>,
}

// ─── Pure Function AST Types ────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct PureFnDef {
    pub name: Spanned<String>,
    pub params: Vec<Spanned<String>>,
    pub body: Vec<Spanned<PureAstStmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PureAstStmt {
    Comment(String),
    Let(PureLetStmt),
    Assign(PureAssignStmt),
    Expr(PureAstExpr),
    /// Impure statement found inside a pure fn body. The resolver emits a
    /// diagnostic for this — the parser accepts it so that the error message
    /// can reference purity rather than producing a generic "unexpected token".
    ImpureViolation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PureLetStmt {
    pub name: Spanned<String>,
    pub value: Option<Spanned<PureAstExpr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PureAssignStmt {
    pub name: Spanned<String>,
    pub value: Spanned<PureAstExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PureAstExpr {
    String(AstStringExpr),
    Var(String),
    Call(PureCallExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PureCallExpr {
    pub name: Spanned<String>,
    pub args: Vec<Spanned<PureAstExpr>>,
}
