use std::marker::PhantomData;
use std::ops::{Index, IndexMut, Range};
use std::path::PathBuf;
use std::time::Duration;

use crate::dsl::parser;
use thiserror::Error;

// ─── Typed Index Infrastructure ────────────────────────────
// Newtype indices and a typed vector that only accepts the
// matching index type. Indices have a private inner field so
// they can only be created by `IndexVec::push`.

macro_rules! define_index {
    ($(#[$meta:meta])* $vis:vis struct $Name:ident;) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis struct $Name(u32);

        impl From<usize> for $Name {
            fn from(v: usize) -> Self { Self(v as u32) }
        }

        impl $Name {
            /// Raw index value, for display/debugging only.
            pub fn to_usize(self) -> usize { self.0 as usize }
        }
    };
}

define_index! {
    /// Index into `SourceMap::files`.
    pub struct FileId;
}

define_index! {
    /// Index into `Plan::effects` (effect definitions).
    pub struct EffectId;
}

define_index! {
    /// Index into `Plan::functions`. Identifies a specific (name, arity) pair.
    pub struct FnId;
}

define_index! {
    /// Index into `Plan::pure_functions`.
    pub struct PureFnId;
}

/// A typed vector that can only be indexed by its associated index type `I`.
/// Indices are created exclusively by `push`, making out-of-bounds access
/// impossible when the index and collection travel together.
#[derive(Debug, Clone)]
pub struct IndexVec<I, T> {
    raw: Vec<T>,
    _marker: PhantomData<fn(I) -> I>,
}

impl<I: From<usize>, T> IndexVec<I, T> {
    pub fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    pub fn from_elem(value: T, count: usize) -> Self
    where
        T: Clone,
    {
        Self {
            raw: vec![value; count],
            _marker: PhantomData,
        }
    }

    /// Insert a value, returning its typed index.
    pub fn push(&mut self, value: T) -> I {
        let id = I::from(self.raw.len());
        self.raw.push(value);
        id
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.raw.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.raw.iter_mut()
    }

    pub fn get(&self, index: I) -> Option<&T>
    where
        I: HasIndex,
    {
        self.raw.get(index.to_usize())
    }
}

impl<I: From<usize>, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for typed indices that can be converted to `usize` for collection access.
/// Sealed to this module — external code cannot implement it.
pub trait HasIndex {
    fn to_usize(self) -> usize;
}

impl HasIndex for FileId {
    fn to_usize(self) -> usize {
        self.0 as usize
    }
}
impl HasIndex for EffectId {
    fn to_usize(self) -> usize {
        self.0 as usize
    }
}
impl HasIndex for FnId {
    fn to_usize(self) -> usize {
        self.0 as usize
    }
}
impl HasIndex for PureFnId {
    fn to_usize(self) -> usize {
        self.0 as usize
    }
}

impl<I: HasIndex, T> Index<I> for IndexVec<I, T> {
    type Output = T;
    fn index(&self, idx: I) -> &T {
        &self.raw[idx.to_usize()]
    }
}

impl<I: HasIndex, T> IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, idx: I) -> &mut T {
        &mut self.raw[idx.to_usize()]
    }
}

impl<I, T> IntoIterator for IndexVec<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.raw.into_iter()
    }
}

impl<'a, I, T> IntoIterator for &'a IndexVec<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.raw.iter()
    }
}

// ─── Timeout ────────────────────────────────────────────────

/// A timeout value that carries its kind for rich reporting.
///
/// Tolerance timeouts absorb environmental latency and are scaled by
/// `--timeout-multiplier`.  Assertion timeouts are semantic correctness
/// checks and are never scaled.
#[derive(Debug, Clone, PartialEq)]
pub enum Timeout {
    Tolerance { duration: Duration, multiplier: f64 },
    Assertion(Duration),
}

impl Timeout {
    /// Return the effective duration, applying the multiplier for tolerance
    /// timeouts.
    pub fn resolve(&self) -> Duration {
        match self {
            Timeout::Tolerance {
                duration,
                multiplier,
            } => Duration::from_secs_f64(duration.as_secs_f64() * multiplier),
            Timeout::Assertion(d) => *d,
        }
    }
}

// ─── Source Map ─────────────────────────────────────────────
// Maps FileId to the file path and source text, needed for
// rendering annotated error diagnostics.

#[derive(Debug, Clone)]
pub struct SourceMap {
    pub files: IndexVec<FileId, SourceFile>,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    pub fn new() -> Self {
        Self {
            files: IndexVec::new(),
            project_root: None,
        }
    }

    pub fn add(&mut self, path: PathBuf, source: String) -> FileId {
        self.files.push(SourceFile { path, source })
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

/// Node in the effect instance DAG.
pub type InstanceId = daggy::NodeIndex;

// ─── Plan ───────────────────────────────────────────────────
// Resolved execution plan for a single test. Contains only
// the functions and effects reachable from this test.
// One Plan per test — the runner executes plans independently.

#[derive(Debug, Clone)]
pub struct Plan {
    /// Impure functions reachable from this test (directly or via effects).
    pub functions: IndexVec<FnId, Function>,
    /// Pure functions reachable from this test.
    pub pure_functions: IndexVec<PureFnId, PureFunction>,
    /// Effect definitions (templates). Referenced by EffectId.
    pub effects: IndexVec<EffectId, Effect>,
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
/// When `alias` is `None`, the effect runs for side effects only
/// and its shell is not exposed to the dependent.
#[derive(Debug, Clone)]
pub struct EffectEdge {
    pub alias: Option<Spanned<String>>,
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

impl From<parser::AstMarkerKind> for CondKind {
    fn from(k: parser::AstMarkerKind) -> Self {
        match k {
            parser::AstMarkerKind::Skip { .. } => CondKind::Skip,
            parser::AstMarkerKind::Run { .. } => CondKind::Run,
            parser::AstMarkerKind::Flaky { .. } => CondKind::Flaky,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CondModifier {
    If,
    Unless,
}

impl From<parser::AstCondModifier> for CondModifier {
    fn from(m: parser::AstCondModifier) -> Self {
        match m {
            parser::AstCondModifier::If { .. } => CondModifier::If,
            parser::AstCondModifier::Unless { .. } => CondModifier::Unless,
        }
    }
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
    Bare(PureExpr),
    Eq(PureExpr, PureExpr),
    Regex(PureExpr, Interpolation),
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
    pub vars: Vec<Spanned<PureVarDecl>>,
    pub shells: Vec<Spanned<ShellBlock>>,
    pub cleanup: Option<Spanned<CleanupBlock>>,
    pub span: Span,
}

// ─── Test ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Test {
    pub name: Spanned<String>,
    /// Inline timeout from the test definition: `test "name" ~5s { ... }`.
    /// `None` means inherit from config/manifest.
    pub timeout: Option<Timeout>,
    pub doc: Option<Spanned<String>>,
    pub conditions: Vec<Spanned<Condition>>,
    /// Resolved references to effect instances in the DAG.
    pub needs: Vec<Spanned<TestNeed>>,
    pub vars: Vec<Spanned<PureVarDecl>>,
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
    /// `None` when bare `need Effect` is used — the effect runs
    /// but its shell is not exposed to the test.
    pub alias: Option<Spanned<String>>,
}

// ─── Overlay ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OverlayEntry {
    pub key: Spanned<String>,
    pub value: Spanned<PureExpr>,
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
    FailRegex(Interpolation),
    /// Set fail pattern (literal). Replaces any previous fail pattern.
    FailLiteral(Interpolation),
    /// Clear the active fail pattern, resetting it to none.
    ClearFailPattern,
    /// Set match timeout for subsequent matches in this shell.
    Timeout(Timeout),
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
    Send(Interpolation),
    SendRaw(Interpolation),
    Let(VarDecl),
    Assign(VarAssign),
}

// ─── Expressions ────────────────────────────────────────────
// Every expression evaluates to a string value.

#[derive(Debug, Clone)]
pub enum Expr {
    /// Quoted string, possibly with interpolation.
    String(Interpolation),
    /// Variable or capture group reference (e.g. "name" or "1").
    Var(String),
    /// Function call. Value: last expression in the function body.
    Call(FnCall),
    /// Send with newline. Value: the sent string.
    Send(Interpolation),
    /// Send without newline. Value: the sent string.
    SendRaw(Interpolation),
    /// Match regex against shell output. Value: full match ($0).
    /// Blocks until match or timeout. Sets capture groups.
    MatchRegex(MatchExpr),
    /// Match literal against shell output. Value: matched text.
    /// Blocks until match or timeout.
    MatchLiteral(MatchExpr),
    /// Reset (drop) the output buffer by advancing the cursor to the end.
    /// Discards all unmatched output. Value: empty string.
    BufferReset,
}

/// Unified match expression carrying a pattern and optional one-shot timeout.
#[derive(Debug, Clone)]
pub struct MatchExpr {
    pub pattern: Interpolation,
    pub timeout_override: Option<Timeout>,
}

// ─── String Expression ──────────────────────────────────────
// A string that may contain interpolated variables or capture
// group references. Used for payloads, string literals, and
// overlay values.

#[derive(Debug, Clone)]
pub struct Interpolation {
    pub parts: Vec<Spanned<StringPart>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StringPart {
    /// Literal text segment.
    Literal(String),
    /// Variable interpolation. Stripped name (e.g. "name" not "${name}").
    VarRef(String),
    /// Escaped dollar sign — resolves to literal "$".
    EscapedDollar,
    /// Capture group reference (e.g. `${1}`).
    CaptureRef(usize),
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

// ─── Pure Function Types ────────────────────────────────────
// Structurally cannot contain shell operations.

#[derive(Debug, Clone)]
pub struct PureFunction {
    pub name: Spanned<String>,
    pub params: Vec<Spanned<String>>,
    pub body: Vec<Spanned<PureStmt>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PureStmt {
    Let(PureVarDecl),
    Assign(PureVarAssign),
    Expr(PureExpr),
}

#[derive(Debug, Clone)]
pub struct PureVarDecl {
    pub name: Spanned<String>,
    pub value: Option<Spanned<PureExpr>>,
}

#[derive(Debug, Clone)]
pub struct PureVarAssign {
    pub name: Spanned<String>,
    pub value: Spanned<PureExpr>,
}

#[derive(Debug, Clone)]
pub enum PureExpr {
    String(Interpolation),
    Var(String),
    Call(PureFnCall),
}

#[derive(Debug, Clone)]
pub struct PureFnCall {
    pub name: Spanned<String>,
    pub args: Vec<Spanned<PureExpr>>,
}

// ─── Purity Validation ──────────────────────────────────────

#[derive(Debug, Clone, Error)]
#[error("{what} cannot be used in a pure context")]
pub struct IrPurityError {
    pub what: String,
    pub span: Span,
}

impl TryFrom<Spanned<Expr>> for Spanned<PureExpr> {
    type Error = IrPurityError;
    fn try_from(spanned: Spanned<Expr>) -> Result<Self, IrPurityError> {
        let span = spanned.span.clone();
        let pure = match spanned.node {
            Expr::String(s) => PureExpr::String(s),
            Expr::Var(name) => PureExpr::Var(name),
            Expr::Call(call) => {
                let pure_args = call
                    .args
                    .into_iter()
                    .map(Spanned::<PureExpr>::try_from)
                    .collect::<Result<Vec<_>, IrPurityError>>()?;
                PureExpr::Call(PureFnCall {
                    name: call.name,
                    args: pure_args,
                })
            }
            Expr::Send(_) => {
                return Err(IrPurityError {
                    what: "send operator".into(),
                    span,
                });
            }
            Expr::SendRaw(_) => {
                return Err(IrPurityError {
                    what: "send raw operator".into(),
                    span,
                });
            }
            Expr::MatchRegex(_) | Expr::MatchLiteral(_) => {
                return Err(IrPurityError {
                    what: "match operator".into(),
                    span,
                });
            }
            Expr::BufferReset => {
                return Err(IrPurityError {
                    what: "buffer reset".into(),
                    span,
                });
            }
        };
        Ok(Spanned::new(pure, span))
    }
}

impl TryFrom<Spanned<ShellStmt>> for Spanned<PureStmt> {
    type Error = IrPurityError;
    fn try_from(spanned: Spanned<ShellStmt>) -> Result<Self, IrPurityError> {
        let span = spanned.span.clone();
        let pure = match spanned.node {
            ShellStmt::Let(decl) => {
                let value = decl.value.map(Spanned::<PureExpr>::try_from).transpose()?;
                PureStmt::Let(PureVarDecl {
                    name: decl.name,
                    value,
                })
            }
            ShellStmt::Assign(assign) => {
                let pure_val = Spanned::<PureExpr>::try_from(assign.value)?;
                PureStmt::Assign(PureVarAssign {
                    name: assign.name,
                    value: pure_val,
                })
            }
            ShellStmt::Expr(e) => {
                let s = Spanned::new(e, span.clone());
                PureStmt::Expr(Spanned::<PureExpr>::try_from(s)?.node)
            }
            ShellStmt::Timeout(_) => {
                return Err(IrPurityError {
                    what: "timeout".into(),
                    span,
                });
            }
            ShellStmt::FailRegex(_) => {
                return Err(IrPurityError {
                    what: "fail pattern".into(),
                    span,
                });
            }
            ShellStmt::FailLiteral(_) => {
                return Err(IrPurityError {
                    what: "fail pattern".into(),
                    span,
                });
            }
            ShellStmt::ClearFailPattern => {
                return Err(IrPurityError {
                    what: "clear fail pattern".into(),
                    span,
                });
            }
        };
        Ok(Spanned::new(pure, span))
    }
}

// ─── Test Suite ─────────────────────────────────────────────

pub struct TestSuite {
    pub plan_results: Vec<crate::dsl::resolver::PlanResult>,
    pub source_map: SourceMap,
    pub warnings: Vec<crate::dsl::resolver::DiagnosticWarning>,
}
