pub type Span = std::ops::Range<usize>;

use crate::Spanned;

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub items: Vec<Spanned<Item>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Comment(String),
    Import(Import),
    Fn(FnDef),
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
    Let(LetStmt),
    Shell(ShellBlock),
    Cleanup(CleanupBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestDef {
    pub name: Spanned<String>,
    pub markers: Vec<Spanned<MarkerDecl>>,
    pub body: Vec<Spanned<TestItem>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestItem {
    Comment(String),
    DocString(String),
    Need(NeedDecl),
    Let(LetStmt),
    Shell(ShellBlock),
    Cleanup(CleanupBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkerDecl {
    pub kind: MarkerKind,
    pub modifier: CondModifier,
    pub var: String,
    pub condition: Option<MarkerCondition>,
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
pub enum MarkerCondition {
    Eq(String),
    Regex(String),
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
    pub value: Spanned<AstExpr>,
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
    Timeout(String),
    FailRegex(AstStringExpr),
    FailLiteral(AstStringExpr),
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
    TimedMatchRegex(String, AstStringExpr),
    TimedMatchLiteral(String, AstStringExpr),
    TimedNegMatchRegex(String, AstStringExpr),
    TimedNegMatchLiteral(String, AstStringExpr),
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
