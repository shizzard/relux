use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

// ─── Source Map ─────────────────────────────────────────────
// Maps FileId to the file path and source text, needed for
// rendering annotated error diagnostics.

pub type FileId = usize;

#[derive(Debug, Clone)]
pub struct SourceMap {
    pub files: Vec<SourceFile>,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
}

impl SourceMap {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            project_root: None,
        }
    }

    pub fn add(&mut self, path: PathBuf, source: String) -> FileId {
        let id = self.files.len();
        self.files.push(SourceFile { path, source });
        id
    }

    pub fn display_path(&self, file: FileId) -> String {
        self.files
            .get(file)
            .map(|f| match &self.project_root {
                Some(root) => f
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&f.path)
                    .display()
                    .to_string(),
                None => f.path.display().to_string(),
            })
            .unwrap_or_else(|| "<unknown>".into())
    }
}

// ─── Span ───────────────────────────────────────────────────
// Byte range into a specific source file.

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub file: FileId,
    pub range: Range<usize>,
}

impl Span {
    pub fn new(file: FileId, range: Range<usize>) -> Self {
        Self { file, range }
    }
}

pub type Spanned<T> = crate::Spanned<T, Span>;

// ─── Resolved Indices ───────────────────────────────────────

/// Index into `Plan::effects` (effect definitions).
pub type EffectId = usize;

/// Index into `Plan::functions`. Identifies a specific (name, arity) pair.
pub type FnId = usize;

/// Node in the effect instance DAG.
pub type InstanceId = daggy::NodeIndex;

// ─── Plan ───────────────────────────────────────────────────
// Resolved execution plan for a single test. Contains only
// the functions and effects reachable from this test.
// One Plan per test — the runner executes plans independently.

#[derive(Debug, Clone)]
pub struct Plan {
    /// Functions reachable from this test (directly or via effects).
    pub functions: Vec<Function>,
    /// Effect definitions (templates). Referenced by EffectId.
    pub effects: Vec<Effect>,
    /// Instantiated, deduplicated effect dependency graph.
    /// Nodes are effect instances, edges encode dependencies
    /// and the alias used by the dependent.
    pub effect_graph: EffectGraph,
    /// The test to execute.
    pub test: Test,
}

// ─── Effect Graph ───────────────────────────────────────────
// DAG of effect instances. An instance is (definition, overlay).
// Same identity tuple = same node (deduplicated).
// Edge from B to A means "B must run before A" (B is a
// dependency of A). The edge carries the alias that A uses
// to refer to B's exported shell.

#[derive(Debug, Clone)]
pub struct EffectGraph {
    pub dag: daggy::Dag<EffectInstance, EffectEdge>,
}

/// A concrete instantiation of an effect definition with a
/// specific overlay. The runtime executes the definition's
/// body with the overlay applied to the environment.
#[derive(Debug, Clone)]
pub struct EffectInstance {
    /// Which effect definition this is an instance of.
    pub effect: EffectId,
    /// Explicit environment overlay for this instance.
    pub overlay: Vec<OverlayEntry>,
}

/// Edge in the effect DAG. Carries the alias that the
/// dependent uses to refer to the dependency's exported shell.
#[derive(Debug, Clone)]
pub struct EffectEdge {
    pub alias: Spanned<String>,
    /// Span of the effect name in the `need` statement (for cycle diagnostics).
    pub need_effect_span: Span,
}

// ─── Function ───────────────────────────────────────────────
// Reusable sequence of shell statements. Executes in the
// caller's shell context. Returns the value of its last
// expression.

#[derive(Debug, Clone)]
pub struct Function {
    pub name: Spanned<String>,
    pub params: Vec<Spanned<String>>,
    pub body: Vec<Spanned<ShellStmt>>,
    pub span: Span,
}

// ─── Condition Markers ──────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CondKind {
    Skip,
    Run,
    Flaky,
}

#[derive(Debug, Clone)]
pub enum CondModifier {
    If,
    Unless,
}

#[derive(Debug, Clone)]
pub struct Condition {
    pub kind: CondKind,
    pub cond: Option<CondExpr>,
}

#[derive(Debug, Clone)]
pub struct CondExpr {
    pub modifier: CondModifier,
    pub body: CondBody,
}

#[derive(Debug, Clone)]
pub enum CondBody {
    Bare(StringExpr),
    Eq(StringExpr, StringExpr),
    Regex(StringExpr, StringExpr),
}

// ─── Effect Definition ──────────────────────────────────────
// Template for effect instances. Contains the body and cleanup
// but NOT the dependency list — dependencies are captured by
// the DAG edges.

#[derive(Debug, Clone)]
pub struct Effect {
    pub name: Spanned<String>,
    pub exported_shell: Spanned<String>,
    pub conditions: Vec<Spanned<Condition>>,
    pub vars: Vec<Spanned<VarDecl>>,
    pub shells: Vec<Spanned<ShellBlock>>,
    pub cleanup: Option<Spanned<CleanupBlock>>,
    pub span: Span,
}

// ─── Test ───────────────────────────────────────────────────

/// Distinguishes inline test timeouts (not affected by `--multiplier`)
/// from inherited timeouts (from config/manifest, affected by `--multiplier`).
#[derive(Debug, Clone, PartialEq)]
pub enum TestTimeout {
    /// Set inline on the test definition: `test "name" ~5s { ... }`
    Explicit(Duration),
}

#[derive(Debug, Clone)]
pub struct Test {
    pub name: Spanned<String>,
    pub timeout: Option<TestTimeout>,
    pub doc: Option<Spanned<String>>,
    pub conditions: Vec<Spanned<Condition>>,
    /// Resolved references to effect instances in the DAG.
    pub needs: Vec<Spanned<TestNeed>>,
    pub vars: Vec<Spanned<VarDecl>>,
    pub shells: Vec<Spanned<ShellBlock>>,
    pub cleanup: Option<Spanned<CleanupBlock>>,
    pub span: Span,
}

/// A test's dependency on an effect instance.
#[derive(Debug, Clone)]
pub struct TestNeed {
    /// Node in the effect DAG.
    pub instance: InstanceId,
    /// Local alias for the instance's exported shell.
    pub alias: Spanned<String>,
}

// ─── Overlay ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OverlayEntry {
    pub key: Spanned<String>,
    pub value: Spanned<StringExpr>,
}

// ─── Shell Block ────────────────────────────────────────────
// A named shell context. Multiple blocks with the same name
// within a test/effect refer to the same shell (switching the
// active shell).

#[derive(Debug, Clone)]
pub struct ShellBlock {
    pub name: Spanned<String>,
    pub stmts: Vec<Spanned<ShellStmt>>,
}

// ─── Shell Statements ───────────────────────────────────────
// Valid inside shell blocks and function bodies.

#[derive(Debug, Clone)]
pub enum ShellStmt {
    /// Set fail pattern (regex). Replaces any previous fail pattern.
    FailRegex(StringExpr),
    /// Set fail pattern (literal). Replaces any previous fail pattern.
    FailLiteral(StringExpr),
    /// Clear the active fail pattern, resetting it to none.
    ClearFailPattern,
    /// Set match timeout for subsequent matches in this shell.
    Timeout(Duration),
    /// Declare a variable, optionally with initial value.
    Let(VarDecl),
    /// Reassign an existing variable from an outer scope.
    Assign(VarAssign),
    /// Expression evaluated for its value or side effects.
    Expr(Expr),
}

// ─── Cleanup Statements ─────────────────────────────────────
// Restricted subset: only sends and variable operations.
// No matches, no function calls, no fail patterns, no timeouts.
// Runs in a fresh implicit shell.

#[derive(Debug, Clone)]
pub struct CleanupBlock {
    pub stmts: Vec<Spanned<CleanupStmt>>,
}

#[derive(Debug, Clone)]
pub enum CleanupStmt {
    Send(StringExpr),
    SendRaw(StringExpr),
    Let(VarDecl),
    Assign(VarAssign),
}

// ─── Expressions ────────────────────────────────────────────
// Every expression evaluates to a string value.

#[derive(Debug, Clone)]
pub enum Expr {
    /// Quoted string, possibly with interpolation.
    String(StringExpr),
    /// Variable or capture group reference (e.g. "name" or "1").
    Var(String),
    /// Function call. Value: last expression in the function body.
    Call(FnCall),
    /// Send with newline. Value: the sent string.
    Send(StringExpr),
    /// Send without newline. Value: the sent string.
    SendRaw(StringExpr),
    /// Match regex against shell output. Value: full match ($0).
    /// Blocks until match or timeout. Sets capture groups.
    MatchRegex(MatchExpr),
    /// Match literal against shell output. Value: matched text.
    /// Blocks until match or timeout.
    MatchLiteral(MatchExpr),
    /// Assert regex does NOT appear in output within timeout.
    /// Succeeds if timeout expires without match. Value: empty string.
    NegMatchRegex(MatchExpr),
    /// Assert literal does NOT appear in output within timeout.
    /// Succeeds if timeout expires without match. Value: empty string.
    NegMatchLiteral(MatchExpr),
    /// Reset (drop) the output buffer by advancing the cursor to the end.
    /// Discards all unmatched output. Value: empty string.
    BufferReset,
}

/// Unified match expression carrying a pattern and optional one-shot timeout.
#[derive(Debug, Clone)]
pub struct MatchExpr {
    pub pattern: StringExpr,
    pub timeout_override: Option<Duration>,
}

// ─── String Expression ──────────────────────────────────────
// A string that may contain interpolated variables or capture
// group references. Used for payloads, string literals, and
// overlay values.

#[derive(Debug, Clone)]
pub struct StringExpr {
    pub parts: Vec<Spanned<StringPart>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StringPart {
    /// Literal text segment.
    Literal(String),
    /// Variable interpolation. Stripped name (e.g. "name" not "${name}").
    Interp(String),
    /// Escaped dollar sign — resolves to literal "$".
    EscapedDollar,
}

// ─── Variable Operations ────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: Spanned<String>,
    pub value: Option<Spanned<Expr>>,
}

#[derive(Debug, Clone)]
pub struct VarAssign {
    pub name: Spanned<String>,
    pub value: Spanned<Expr>,
}

// ─── Function Call ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FnCall {
    pub name: Spanned<String>,
    pub args: Vec<Spanned<Expr>>,
}
