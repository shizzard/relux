# R004: Resolver Rework

- **Status**: done
- **Created**: 2026-03-20

## Motivation

The current resolver has unnecessary coupling between stages and redundant intermediate structures. Module loading eagerly loads all library files regardless of whether they're referenced. Circular imports are treated as errors despite being semantically harmless. Scope building is a separate pass that constructs `ModuleScope` and `ModuleExports` structs, only for the same information to be re-traversed during IR lowering. Each test plan builds its own isolated function and effect registries, duplicating lowering work across tests that share dependencies.

## Design

The resolver pipeline is simplified to three stages with shared, memoized registries.

### Stage 1: File Discovery

Find all test-containing module paths from the configured test directory (or CLI-specified paths). Library modules are not discovered here — they are loaded on demand when imported.

### Stage 2: Module Loading (demand-driven)

Worklist algorithm seeded with test module paths from stage 1:

1. Pop a path from the queue
2. Load source via `SourceLoader`, parse into `AstModule`, store `(FileId, AstModule)` in the `AstTable`
3. Walk AST for `import` items, enqueue any module paths not yet in the `AstTable`
4. Repeat until the queue is empty

No circular import detection needed — the `AstTable` itself acts as the visited set. If module A imports B and B imports A, B is already in the table when revisited, so it's skipped.

The `CauseTable` and `WarningTable` are created before stage 2 and shared across all stages. `ModuleNotFound` is emitted when `SourceLoader::load` returns `None` — inserted into the `CauseTable` as `Cause::Invalid`. Parse errors are similarly recorded as `Cause::Invalid`. Neither blocks loading of other modules. Any test that transitively depends on a missing or unparseable module becomes `Plan::Invalid` with a reference to the corresponding cause ID. `CircularImport` diagnostic is removed entirely.

### Stage 3: Plan Building (per test)

Walk the `AstTable`, find every `AstItem::Test`, and build a `Plan` for each. Name resolution, IR lowering, and validation all happen inline — no separate scope-building pass.

```rust
for (module_path, (file_id, module)) in ast_table.iter() {
    for item in &module.items {
        if let AstItem::Test { def, .. } = &item.node {
            let plan = build_plan(def, file_id, &item.span, &mut ctx);
            plans.push(plan);
        }
    }
}
```

`build_plan` evaluates the test's own markers, calls `IrTest::from_ast`, and wraps the result into a `Plan` variant (`Runnable`, `Skipped`, or `Invalid`). Each `from_ast` call may recursively trigger lowering of dependencies (functions, effects) through the shared registries.

When resolving names (function calls, effect references) from module A:

1. Look up in A's own AST items
2. Follow A's import declarations, look up directly in the target module's AST items in the `AstTable`

The `ModuleScope`, `ModuleExports`, and `build_module_scope`/`build_module_exports` functions are removed.

#### Lowering with memoization

All lowering results are cached in shared Suite-level registries. For any definition (function, pure function, effect), the lowering process is:

1. Check registry — if entry exists, return cached status
2. Evaluate own markers — if skip triggers, insert `Skipped`, return
3. Lower dependencies — if any returns `Skipped` or `Invalid`, propagate and cache, return
4. Lower own body — if error, insert `Invalid`, return
5. Insert `Lowered(IR)`, return

Since markers are always checked before body lowering, `Skipped` takes natural precedence over `Invalid` — you never attempt to lower a body whose markers already skip it.

Skip and invalid statuses propagate transitively and are cached at every intermediate level. If a deeply nested function is skipped, the entire chain of effects and functions that depend on it is marked as skipped. Since the DSL is declarative — no conditionals, no dynamic dispatch — the reachability graph from any definition is fixed, making cached results valid for all tests.

#### Test outcome categories

A test's plan building produces one of three `Plan` variants:

- **Runnable**: everything lowered successfully, ready for execution
- **Skipped**: a marker triggered on the test or any reachable dependency
- **Invalid**: a lowering error in the test or any reachable dependency

A test can have both skip and invalid causes simultaneously (e.g., one dependency is skipped, another has a lowering error). Invalid takes precedence over skip when determining the plan variant. The plan carries all cause IDs regardless — the reporter can distinguish them.

`Invalid` is distinct from a test failure — nothing executed. It is also distinct from skip — it's not intentional. The suite continues running other tests regardless.

### Output types

#### Table types

All table types live in a crate-level `table` module (`src/table.rs`), used by both the resolver and the runtime.

```rust
/// Mutable shared table — populated incrementally, potentially from multiple threads.
struct SharedTable<K, V> {
    map: Arc<Mutex<HashMap<K, V>>>,
}

/// Immutable shared table — frozen after population, shared for reads only.
struct FrozenTable<K, V> {
    map: Arc<HashMap<K, V>>,
}

/// Local name resolution table — maps local keys to global keys,
/// backed by a SharedTable for registry lookups.
struct LocalTable<K, GK, V> {
    locals: HashMap<K, GK>,
    registry: SharedTable<GK, V>,
}

impl<K, V> TryFrom<SharedTable<K, V>> for FrozenTable<K, V> {
    type Error = SharedTable<K, V>;
    fn try_from(shared: SharedTable<K, V>) -> Result<Self, Self::Error> { ... }
}

/// Absolute file path, used as the stable file identity.
/// Replaces the current auto-incremented FileId index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileId {
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct SourceFile {
    path: PathBuf,
    source: String,
}

type AstTable = FrozenTable<ModulePath, (FileId, AstModule)>;
type SourceTable = FrozenTable<FileId, SourceFile>;
type FnTable = SharedTable<FnId, Result<IrFn, LoweringBail>>;
type PureFnTable = SharedTable<FnId, Result<IrPureFn, LoweringBail>>;
type EffectTable = SharedTable<EffectId, Result<IrEffect, LoweringBail>>;
type CauseTable = SharedTable<CauseId, Cause>;
type WarningTable = SharedTable<WarningId, Warning>;
```

`SharedTable` and `FrozenTable` are crate-level generic data structures, not resolver-specific. `AstTable` and `SourceTable` are built as `SharedTable` during stage 2 (module loading, potentially parallel), then frozen via `TryFrom` before stage 3. The `try_into()` call succeeds when all worker clones are dropped — failure indicates a leaked `Arc` clone.

All error types (`LoweringBail`, `Cause`, `CycleReport`) are implemented using the `thiserror` crate.

#### CauseTable

Lowering errors and skip triggers are collected into a shared `CauseTable` rather than duplicated per test. Each cause has a stable, human-readable identifier derived from the definition's identity hash.

```rust
/// Stable mnemonic identifier, e.g. "broken-walrus-0042".
/// Derived from a hash of (module, name, arity, error_kind).
/// Format: {adjective}-{noun}-{4-digit suffix}.
/// ~650M combinations (256 adjectives × 256 nouns × 10000 suffixes).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CauseId {
    id: String,
}

#[derive(Debug)]
enum Cause {
    Skip(SkipReport),
    Invalid(InvalidReport),
}
```

The `CauseTable` lives on both `LoweringContext` (populated during resolution) and `Suite` (read during reporting). Full diagnostics are printed once from the `CauseTable` to stderr after resolution completes, before the runtime starts. Individual tests only reference cause IDs — no duplicate diagnostics.

#### Plan

```rust
#[derive(Debug)]
struct TestMeta {
    name: String,
    docstring: Option<String>,
    timeout: Option<IrTimeout>,
    span: IrSpan,
}

#[derive(Debug)]
enum Plan {
    Runnable {
        meta: TestMeta,
        test: IrTest,
        warnings: Vec<WarningId>,
    },
    Skipped {
        meta: TestMeta,
        causes: Vec<CauseId>,
        warnings: Vec<WarningId>,
    },
    Invalid {
        meta: TestMeta,
        causes: Vec<CauseId>,
        warnings: Vec<WarningId>,
    },
}
```

The `Runnable` variant is self-contained: `IrTest` owns its local lookup tables, which hold `Arc` clones to the shared registries. All functions, pure functions, and effects reachable from the test are accessible through the embedded tables. No external context is needed to execute a plan.

The `Skipped` and `Invalid` variants carry cause IDs that reference entries in the `CauseTable`. An `Invalid` plan may contain both invalid and skip causes; a `Skipped` plan contains only skip causes. The runtime reports test outcomes with cause IDs only — e.g., `"test X: invalid [broken-walrus-0042, sad-fox-1337]"`.

#### Suite

```rust
#[derive(Debug)]
struct Suite {
    plans: Vec<Plan>,
    source_map: SourceTable,
    env: Arc<Env>,
    causes: CauseTable,
    warnings: WarningTable,
}
```

The resolver entry point returns a `Suite`:

```rust
fn resolve(
    source_loader: &dyn SourceLoader,
    test_paths: Vec<ModulePath>,
    env: Arc<Env>,
) -> Suite
```

`test_paths` is the output of stage 1 (file discovery). `source_loader` is used by stage 2 (module loading). `env` is a snapshot of environment variables, captured once before resolution. The function runs stages 2 and 3 internally. The `env` snapshot is stored on the `Suite` so the runtime uses the same environment that was used for marker evaluation.

The `source_map` is shared across all plans for diagnostic rendering during both resolution and runtime. The `causes` table is printed to stderr after resolution, before runtime execution. The `LoweringContext` is dropped after resolution — its `Arc`s survive through the local tables in each plan's IR nodes and through the `Suite`'s `source_map`.

#### Marker evaluation

Markers are pure expressions evaluated at resolve time. Given the same AST and environment, a marker always produces the same result. No shell state, no runtime context, no deferred evaluation. Markers are resolved eagerly during lowering rather than stored in the `Plan` for runtime evaluation.

Evaluation proceeds in two phases:

1. **Lower** the marker's condition expressions as `IrPureExpr`. This may trigger `IrPureFn::from_ast` for any user-defined pure functions called in the expression — those functions are lowered (and cached) as a side effect of marker evaluation. If any called function fails to lower (cycle, invalid expression), the marker evaluation fails and the definition becomes `Cause::Invalid`.

2. **Interpret** the lowered `IrPureExpr` to produce a string value, then apply condition logic:
   - **Bare**: non-empty string = true
   - **Eq**: string equality of lhs and rhs
   - **Regex**: lhs is the value, rhs is the regex pattern

The interpreter is a pure expression evaluator — a subset of the runtime VM with no shell, no captures, no mutable variables. It handles:
- String literals and interpolation (only environment variables are accessible via `${}` — no test/effect-scoped variables exist at marker evaluation time)
- Pure BIF calls (`env()`, `concat()`, etc.)
- User-defined pure function calls (recursively interprets their lowered `IrPureFn` body)

This interpreter is new code — the current resolver does not evaluate markers. It lives in a crate-level `evaluator` module (`src/evaluator/mod.rs`), not in the resolver or runtime. The evaluator operates on IR types (`IrPureExpr`, `IrPureFn`) and is designed to be used by both the resolver (marker evaluation) and the runtime (pure function execution). This avoids duplicating pure expression evaluation logic across pipeline stages.

#### Evaluator interface

```rust
// src/evaluator/mod.rs

pub fn eval_pure_expr(expr: &IrPureExpr, vars: &VarScope, fns: &PureFnTable) -> String {
    todo!()
}

pub fn eval_pure_fn(func: &IrPureFn, args: Vec<String>, vars: &VarScope, fns: &PureFnTable) -> String {
    todo!()
}
```

Both functions are infallible — all failure modes (undefined functions, wrong arity, cycles, disallowed statements) are caught at lowering time. Missing variables evaluate to empty string.

#### VarScope

A generic variable storage type living in a crate-level `stack` module (`src/stack.rs`). The module will later house the full scope stack used by the runtime; for now it contains only `VarScope`.

```rust
// src/stack.rs

#[derive(Debug, Default)]
pub struct VarScope {
    vars: HashMap<String, String>,
}

impl VarScope {
    pub fn new() -> Self {
        Self { vars: HashMap::new() }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.vars.insert(key, value);
    }

    pub fn assign(&mut self, key: &str, value: String) -> bool {
        if let Some(slot) = self.vars.get_mut(key) {
            *slot = value;
            true
        } else {
            false
        }
    }
}
```

The evaluator creates a `VarScope` per function call (populated with parameters), with env vars as the lookup fallback. The runtime's `ScopeStack` will be refactored to use `VarScope` for its frame variables, replacing the current raw `HashMap<String, String>`.

**Caveat: Relux purity vs. FP purity.** In Relux, "pure" means "does not use a shell" — it does not mean referentially transparent. Some built-in functions like `uuid()` and `timestamp()` are pure in Relux's sense (no shell required) but non-deterministic. These are legal to call in marker conditions since the type system does not distinguish them, but they are semantically nonsensical there — there is no reason to skip a test based on a random UUID. This is an acceptable tradeoff: the marker system relies on Relux purity (shell-free evaluation), and users are expected to use deterministic expressions in marker conditions. This should be documented as a best-practice guideline rather than enforced by the resolver.

### Global registry keys

Registry keys uniquely identify a definition across the entire suite. Module path is included to avoid collisions between same-named definitions from different modules.

```rust
/// Uniquely identifies a function (pure or impure) across the suite.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FnId {
    module: ModulePath,
    name: String,
    arity: usize,
}

/// Uniquely identifies an effect definition across the suite.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EffectId {
    module: ModulePath,
    name: EffectName,
}
```

### Shared registries

All registries live on `LoweringContext` (see below), each field independently wrapped in `Arc` for fine-grained sharing (see type aliases in "Output types").

Registry values are `Result<T, LoweringBail>` — `Ok` for successfully lowered IR, `Err` for skip/invalid. Absence from the registry means the definition has not yet been visited. The `HashMap::entry` API handles the "check if exists, compute if not" pattern.

```rust
#[derive(Debug, Clone)]
enum LoweringBail {
    Skip(SkipReport),
    Invalid(InvalidReport),
}

#[derive(Debug, Clone)]
enum InvalidReport {
    /// Function or effect references itself transitively.
    Cycle(CycleReport),
    /// Shell operator or CaptureRef used in a pure function context.
    PurityViolation { span: IrSpan },
    /// Call to a function name/arity not in scope.
    UndefinedFunctionCall { name: String, arity: usize, span: IrSpan },
    /// Need referencing an effect name not in scope.
    UndefinedEffectNeed { name: String, span: IrSpan },
    /// Selective import references a function not exported by the target module.
    UndefinedFunctionImport { name: String, module_path: ModulePath, span: IrSpan },
    /// Selective import references an effect not exported by the target module.
    UndefinedEffectImport { name: String, module_path: ModulePath, span: IrSpan },
    /// Import targets a module path that was not loaded (missing or failed to parse).
    UndefinedModuleImport { module_path: ModulePath, span: IrSpan },
    /// Two definitions with the same name in scope (own definitions or imports).
    NameConflict { name: String, first: IrSpan, second: IrSpan },
    /// Regex pattern that fails to compile.
    InvalidRegex { pattern: String, error: String, span: IrSpan },
}

impl From<InvalidReport> for Diagnostic { ... }
impl From<SkipReport> for Diagnostic { ... }

/// Which definition triggered the skip.
#[derive(Debug, Clone)]
enum DefinitionRef {
    Fn(FnId),
    Effect(EffectId),
    Test { name: String, module: ModulePath },
}

#[derive(Debug, Clone)]
struct SkipReport {
    definition: DefinitionRef,
    marker_span: IrSpan,
    evaluation: SkipEvaluation,
}

#[derive(Debug, Clone)]
enum SkipEvaluation {
    /// @skip with no condition.
    Unconditional,
    /// @skip(expr) or @run(expr) — bare expression.
    Bare {
        value: String,
        met: bool,
    },
    /// @skip(lhs = rhs) or @run(lhs = rhs) — literal equality.
    Eq {
        lhs: String,
        rhs: String,
        met: bool,
    },
    /// @skip(expr ? /pattern/) or @run(expr ? /pattern/) — regex match.
    Regex {
        value: String,
        pattern: String,
        met: bool,
    },
}

/// CycleReport is embedded in InvalidReport::Cycle.
/// It never lives in a registry or on a plan directly.
#[derive(Debug, Clone)]
struct FnCycleEntry {
    id: FnId,
    call_span: IrSpan,
}

#[derive(Debug, Clone)]
struct EffectCycleEntry {
    id: EffectId,
    need_span: IrSpan,
}

#[derive(Debug, Clone)]
enum CycleReport {
    Function { chain: Vec<FnCycleEntry> },
    Effect { chain: Vec<EffectCycleEntry> },
}

```

The `ast_table` and `source_map` are immutable after stage 2 — `Arc` only, no mutex. The IR registries and cause table use interior mutability (`Mutex`) since lowering populates them incrementally. This structure enables parallel plan building in the future — each test's lowering checks/populates the shared registries independently, with low mutex contention since most lookups hit cached entries.

### Name resolution and local lookup tables

Name resolution is performed inline during lowering — no separate scope-building pass. Each definition's `lower` implementation creates local lookup tables that map locally-visible names (possibly aliased via imports) to global registry keys. The local tables also hold `Arc` clones of the shared registries, encapsulating the full lookup path: local name → global key → registry entry. Local tables are transient during lowering — only `IrTest` retains them for runtime access (see "Plan" section).

#### Local key types

Local keys are distinct from global registry keys. They live in a separate module to avoid mixing with other data types.

```rust
/// Local function key — used by both fn and pure fn local tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LocalFnKey {
    name: String,
    arity: usize,
}

/// Local effect key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LocalEffectKey {
    name: EffectName,
}
```

#### LocalTable

`LocalTable` (defined in the `table` module) encapsulates the local→global mapping and a `SharedTable` clone for registry lookups. The caller does one `get(local_key)` and receives the registry entry without ever touching the global registry directly. `None` means the local key is not in scope; the registry lookup is transparent.

#### Per-node table ownership

Each IR node creates the local tables it needs from `LoweringContext` factory methods, then populates them from own definitions and import declarations during lowering.

| Node | fn table | pure fn table | effect table |
|------|----------|---------------|--------------|
| IrTest | yes | yes | yes |
| IrEffect | yes | yes | yes |
| IrFn | yes | yes | no |
| IrPureFn | no | yes | no |

`IrFn` gets both fn tables because impure functions can call pure ones. `IrPureFn` only gets the pure table — structural purity enforcement.

#### Population

When lowering a node from module A:

1. Insert A's own definitions as identity mappings (local key = canonical key)
2. Walk A's import declarations in the `AstTable`. For each imported name, insert a mapping from the local name (or alias) to the canonical `FnId`/`EffectId` derived from the target module
3. Duplicate name detection happens at insert time — conflicting imports produce diagnostics

## IR conventions

### Naming

All IR structs and enums are prefixed with `Ir`, mirroring the `Ast` prefix convention in the parser. For example: `IrExpr`, `IrShellStmt`, `IrFn`, `IrEffect`, `IrTest`.

### IrSpan

The crate-level `Span` (opaque `{ start, end }` byte-offset pair) is reused as-is. The IR wraps it with a `FileId` for cross-file diagnostics:

```rust
#[derive(Debug, Clone)]
pub struct IrSpan {
    file: FileId,
    span: Span,
}
```

Fields are private. Access via `file(&self) -> &FileId` and `span(&self) -> &Span`. No span arithmetic exists on `IrSpan` — all span adjustments happen at the AST level before lowering. The IR only reads spans, never modifies them.

### IrNode trait

All IR structs have a `span` field of type `IrSpan`. All IR enum variants use named fields, each including a `span: IrSpan` field. The `IrNode` trait provides common accessors, implemented via macros matching the AST pattern:

```rust
pub trait IrNode {
    fn span(&self) -> &IrSpan;
}

macro_rules! impl_ir_node_struct {
    ($($ty:ty),* $(,)?) => { ... };
}

macro_rules! impl_ir_node_enum {
    ($ty:ty { $($variant:ident),* $(,)? }) => { ... };
}
```

Every IR type carries its span. No `Spanned<T>` wrapper needed — the span is part of the node itself. Exception: `IrFn` and `IrPureFn` are enums where the `Builtin` variant has no span — `IrNode` is not implemented for these types (see "LoweringContext" section).

### Identifiers

`AstIdent` and `IrIdent` are introduced as dedicated identifier types, replacing raw `String` for names throughout the AST and IR.

```rust
// parser/ast.rs — no span; used as Spanned<AstIdent> in parent structs,
// following the AST wrapping pattern.
#[derive(Debug, Clone, PartialEq)]
struct AstIdent {
    name: String,
}

// resolver/ir.rs — carries its own IrSpan, following the IR pattern.
#[derive(Debug, Clone)]
struct IrIdent {
    name: String,
    span: IrSpan,
}
impl_ir_node_struct!(IrIdent);
```

`AstIdent` replaces `Spanned<String>` → `Spanned<AstIdent>` in function names, parameter names, effect names, shell names, variable names, import names, aliases. `IrIdent` internalizes the span as all IR types do. No subtypes or enum variants; the parent struct's field conveys what kind of identifier it is. Lowering is infallible 1:1.

### AST → IR mirroring

IR nodes map 1:1 to AST nodes. The IR does not restructure the AST — it enriches and validates it:

- **Span enrichment**: `Span` → `IrSpan` (pairs with `FileId`)
- **Name resolution**: call names → resolved function keys, effect names → resolved identities
- **Validation**: purity enforcement, undefined name detection, regex validation
- **Removal**: comments dropped, imports resolved away, markers evaluated and removed
- **Timeout parsing**: moved to the parser (not the resolver's job — the parser should parse durations while preserving original spans)

No structural transformations: statements stay statements (no promotion to expressions), no merging of timed match variants with untimed ones. Purity is enforced by keeping `IrFn` and `IrPureFn` as separate types (mirroring `AstFnDef`/`AstPureFnDef`), with a pure subset of statements (`IrPureStmt`) and expressions (`IrPureExpr`) that structurally guarantee no shell operations in pure contexts.

### IR type catalog

#### AST types with IR counterparts

| AST type | IR type | Notes |
|----------|---------|-------|
| `AstExpr` | `IrExpr` | All variants: `String`, `Var`, `Call`, `CaptureRef` |
| `AstExpr` (pure subset) | `IrPureExpr` | No `CaptureRef` variant — structurally enforced. Current impl already validates this via `TryFrom` |
| `AstInterpolation` | `IrInterpolation` | |
| `AstStringPart` | `IrStringPart` | |
| `AstCallExpr` | `IrCallExpr` | Resolved name → function key |
| `AstStmt` | `IrShellStmt` | Shell-context statements |
| `AstStmt` (pure subset) | `IrPureStmt` | No shell operators (`Send`, `Match*`, `Timeout`, etc.) |
| `AstLetStmt` | `IrLetStmt` | |
| `AstAssignStmt` | `IrAssignStmt` | |
| `AstShellBlock` | `IrShellBlock` | |
| `AstCleanupBlock` | `IrCleanupBlock` | |
| `AstFnDef` | `IrFn::UserDefined` | Cacheable, memoized in `FnTable`. `IrFn` is an enum — see below |
| `AstPureFnDef` | `IrPureFn::UserDefined` | Cacheable, memoized in `PureFnTable`. `IrPureFn` is an enum — see below |
| `AstEffectDef` | `IrEffect` | Cacheable, memoized in `EffectTable` |
| `AstEffectItem` | `IrEffectItem` | |
| `AstTestDef` | `IrTest` | Not cacheable (each test is unique) |
| `AstTestItem` | `IrTestItem` | |
| `AstNeedDecl` | `IrEffectNeed` | Resolved effect reference + canonical overlay + alias |
| `AstOverlayEntry` | `IrOverlayEntry` | No AST passes to runtime — overlay values must be IR |
| `AstTimeoutKind` | `IrTimeoutKind` | |
| `AstIdent` | `IrIdent` | Infallible 1:1, span enrichment only |

#### AST types with NO IR counterpart

| AST type | Reason |
|----------|--------|
| `AstMarkerDecl` | Evaluated eagerly at resolve time; result feeds `SkipReport` or is discarded |
| `AstMarkerKind` | Part of marker evaluation, not preserved |
| `AstMarkerCond` | Part of marker evaluation, not preserved |
| `AstMarkerCondBody` | Part of marker evaluation, not preserved |
| `AstCondModifier` | Part of marker evaluation, not preserved |
| `AstImport` | Resolved into local lookup table entries during lowering |
| `AstImportName` | Resolved into local lookup table entries during lowering |
| `AstModule` | Dissolved — items registered into shared tables |
| `AstItem` | Dissolved — each variant registered independently |
| `Comment` (in all contexts) | Dropped |
| `DocString` (in `AstTestItem`) | Moved to `TestMeta` or dropped |

#### Pure subset enforcement

`IrPureExpr` and `IrPureStmt` are structurally restricted subsets, not aliases:

- **`IrPureExpr`**: `String`, `Var`, `Call` — no `CaptureRef` variant. Capture references (`$1`, `$2`) are meaningless in pure contexts because there is no preceding regex match.
- **`IrPureStmt`**: `Let`, `Assign`, `Expr` — no shell operators (`Send`, `SendRaw`, `Match*`, `TimedMatch*`, `Timeout`, `FailRegex`, `FailLiteral`, `ClearFailPattern`, `BufferReset`). Comments are dropped during lowering.

These are separate enums, not runtime-checked subsets. If a pure function body contains a `Send` statement, it is a type error at the lowering level — the `lower` implementation for `IrPureStmt` simply has no arm for it and produces a diagnostic.

**Cleanup blocks** use `IrShellStmt` — the same statement type as regular shell blocks. Cleanup semantics (fresh implicit shell, reverse execution order) are a runtime concern, not enforced structurally at the IR level. Future RFCs may introduce an `IrCleanupStmt` restricted subset if needed.

### IrNodeLowering trait

A single trait handles all IR lowering. The AST is borrowed (`&Self::Ast`) because the `AstTable` is immutable and shared after stage 2. Default no-op implementations for caching and cycle detection methods mean non-cacheable types only need to implement `lower`.

```rust
trait IrNodeLowering: Sized + Clone {
    type Ast;

    /// Return `None` for non-cacheable types (default).
    /// Return `Some(Some(result))` if already resolved.
    /// Return `Some(None)` if cacheable but not yet visited.
    fn cached(_ast: &Self::Ast, _ctx: &LoweringContext) -> Option<Option<Result<Self, LoweringBail>>> {
        None
    }

    fn cache(_ast: &Self::Ast, _result: Result<Self, LoweringBail>, _ctx: &mut LoweringContext) {}

    fn check_cycle(_ast: &Self::Ast, _ctx: &LoweringContext) -> Option<CycleReport> {
        None
    }

    fn push_in_progress(_ast: &Self::Ast, _ctx: &mut LoweringContext) {}

    fn pop_in_progress(_ctx: &mut LoweringContext) {}

    /// AST → IR lowering for a single node.
    ///
    /// Cacheable types (IrFn, IrPureFn, IrEffect) must not be lowered
    /// through this method directly — use `from_ast` instead, which
    /// handles caching, cycle detection, and in-progress tracking.
    fn lower(ast: &Self::Ast, file: FileId, ctx: &mut LoweringContext) -> Result<Self, LoweringBail>;

    fn from_ast(ast: &Self::Ast, file: FileId, ctx: &mut LoweringContext) -> Result<Self, LoweringBail> {
        match Self::cached(ast, ctx) {
            None => Self::lower(ast, file, ctx),
            Some(Some(result)) => result,
            Some(None) => {
                if let Some(cycle) = Self::check_cycle(ast, ctx) {
                    let bail = LoweringBail::Invalid(InvalidReport::Cycle(cycle));
                    Self::cache(ast, Err(bail.clone()), ctx);
                    return Err(bail);
                }
                Self::push_in_progress(ast, ctx);
                let result = Self::lower(ast, file, ctx);
                Self::pop_in_progress(ctx);
                Self::cache(ast, result.clone(), ctx);
                result
            }
        }
    }
}
```

#### Implementation tiers

**Non-cacheable types** (expressions, statements, blocks, etc.): implement `lower` only. All other methods default to no-ops. `from_ast` calls `lower` directly.

**Cacheable types** (`IrFn`, `IrPureFn`, `IrEffect`): implement `lower` + override five one-liner methods (`cached`, `cache`, `check_cycle`, `push_in_progress`, `pop_in_progress`) pointing to the appropriate `LoweringContext` fields.

#### Implementors

| IR type | Tier | AST type | Notes |
|---------|------|----------|-------|
| `IrFn` | cacheable | `AstFnDef` | `FnTable`, `fn_stack` |
| `IrPureFn` | cacheable | `AstPureFnDef` | `PureFnTable`, `fn_stack` |
| `IrEffect` | cacheable | `AstEffectDef` | `EffectTable`, `effect_stack` |
| `IrTest` | non-cacheable | `AstTestDef` | Each test is unique |
| `IrExpr` | non-cacheable | `AstExpr` | |
| `IrPureExpr` | non-cacheable | `AstExpr` | Rejects `CaptureRef` |
| `IrShellStmt` | non-cacheable | `AstStmt` | |
| `IrPureStmt` | non-cacheable | `AstStmt` | Rejects shell operators |
| `IrLetStmt` | non-cacheable | `AstLetStmt` | |
| `IrAssignStmt` | non-cacheable | `AstAssignStmt` | |
| `IrInterpolation` | non-cacheable | `AstInterpolation` | |
| `IrStringPart` | non-cacheable | `AstStringPart` | |
| `IrCallExpr` | non-cacheable | `AstCallExpr` | Triggers cacheable `from_ast` on callee |
| `IrShellBlock` | non-cacheable | `AstShellBlock` | |
| `IrCleanupBlock` | non-cacheable | `AstCleanupBlock` | |
| `IrEffectItem` | non-cacheable | `AstEffectItem` | |
| `IrTestItem` | non-cacheable | `AstTestItem` | |
| `IrEffectNeed` | non-cacheable | `AstNeedDecl` | Triggers cacheable `from_ast` on effect |
| `IrOverlayEntry` | non-cacheable | `AstOverlayEntry` | |

Types with **no impl** (consumed inline): markers (`AstMarkerDecl`, `AstMarkerKind`, `AstMarkerCond`, `AstMarkerCondBody`, `AstCondModifier`), imports (`AstImport`, `AstImportName`), comments, docstrings, `AstModule`, `AstItem`.

#### LoweringContext

The `LoweringContext` is the central transient struct during resolution. It owns all shared registries (wrapped in `Arc` for sharing with local lookup tables) and the in-progress stacks for cycle detection. It is created at the start of stage 3, used throughout plan building, and dropped when resolution completes — the `Arc`s survive through the local tables embedded in IR nodes.

```rust
struct LoweringContext {
    ast_table: AstTable,
    source_map: SourceTable,
    env: Arc<Env>,
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
    causes: CauseTable,
    warnings: WarningTable,
    fn_stack: Vec<(FnId, IrSpan)>,
    effect_stack: Vec<(EffectId, IrSpan)>,
}
```

Before plan building begins, all built-in functions (BIFs) are pre-inserted into `FnTable` and `PureFnTable` as `Ok(IrFn::Builtin { .. })` / `Ok(IrPureFn::Builtin { .. })` entries. They use a reserved synthetic module path (e.g., `@builtin`) that cannot collide with filesystem-derived paths (`lib/...`, `tests/...`). Every local table cloned from the shared registries automatically sees them — no special-casing during name resolution or population.

`IrFn` and `IrPureFn` are enums, not structs:

```rust
#[derive(Debug, Clone)]
enum IrFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrShellStmt>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}

#[derive(Debug, Clone)]
enum IrPureFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrPureStmt>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}
```

The `Builtin` variant is opaque — the runtime dispatches on the name. It has no body, no span, and no source file. `IrNode` is not implemented for `IrFn`/`IrPureFn` since the `Builtin` variant has no span; callers that need a definition span (e.g., "defined here" labels in diagnostics) match on the variant.

Two in-progress stacks: one for functions (both pure and impure share `FnId` identity and the same stack) and one for effects. The stacks are independent — function lowering never touches `effect_stack`, and effect lowering uses both stacks (its own needs on `effect_stack`, function calls in shell blocks on `fn_stack`).

Factory methods create local lookup tables by cloning the relevant `Arc`:

```rust
impl LoweringContext {
    fn local_fn_table(&self) -> LocalTable<LocalFnKey, FnId, Result<IrFn, LoweringBail>> {
        LocalTable::new(self.functions.clone())
    }

    fn local_pure_fn_table(&self) -> LocalTable<LocalFnKey, FnId, Result<IrPureFn, LoweringBail>> {
        LocalTable::new(self.pure_functions.clone())
    }

    fn local_effect_table(&self) -> LocalTable<LocalEffectKey, EffectId, Result<IrEffect, LoweringBail>> {
        LocalTable::new(self.effects.clone())
    }
}
```

#### Single-pass lowering

Lowering happens in a single traversal. When `lower` encounters a dependency (a function call in a statement, a `need` declaration in an effect), it resolves it inline by calling `T::from_ast` on the callee. This goes through the full `from_ast` path — cache check, cycle detection, recursive lowering — all within the same pass. No separate collection pass is needed.

This means any `lower` implementation can trigger `from_ast` on cacheable types, and the stacks on `LoweringContext` are always available for cycle detection. Effects push/pop on `effect_stack`, both `IrFn` and `IrPureFn` push/pop on `fn_stack`. The stacks are independent — function lowering never touches `effect_stack`, and effect lowering uses both stacks (its own needs on `effect_stack`, function calls in shell blocks on `fn_stack`).

### Effect graph

The current `daggy::Dag`-based effect graph is replaced with a recursive structure embedded in the `IrEffect` itself.

#### Key types

```rust
/// Registry key — see "Global registry keys" section.
/// struct EffectId { module: ModulePath, name: EffectName }

/// A resolved `need` declaration: an effect reference with applied overlay and alias.
/// The `canonical_overlay` is the canonical string form of the overlay, derived from the
/// AST via canonical(). Overlay entries are sorted for normalization. Two overlays with
/// the same canonical form are considered identical regardless of runtime values.
/// Overlay evaluation stays at runtime — the canonical form is purely a deduplication key.
#[derive(Debug, Clone)]
struct IrEffectNeed {
    effect: EffectId,
    canonical_overlay: String,
    overlay: Vec<IrOverlayEntry>,
    alias: Option<String>,
    span: IrSpan,
}

#[derive(Debug, Clone)]
struct IrEffect {
    name: IrIdent,
    exported_shell: IrIdent,
    needs: Vec<IrEffectNeed>,
    body: Vec<IrEffectItem>,
    span: IrSpan,
}
```

#### Caching

Each `IrEffect` carries its resolved `needs` — the effects it depends on. Since the DSL has no conditionals, an effect's need declarations are fixed. The full `IrEffect` (including its `needs` list) is cached in the shared registry as `Result<IrEffect, LoweringBail>`, resolved once and reused by every test that reaches it.

The full dependency graph is implicit: follow each `IrEffectNeed.effect` to its `IrEffect` in the registry, then follow its `needs` recursively. No separate graph data structure is needed.

A test holds its own `Vec<IrEffectNeed>` for direct needs.

#### Why not daggy

`daggy::Dag` is not composable (no native merge, requires index remapping) and not cacheable (per-test, rebuilt from scratch). The recursive `needs` structure is both — each effect is resolved once, and a test's graph is the union of its direct needs and their transitive `needs`.

#### Cycle detection

Circular dependencies (both effect needs and function calls) are detected via the in-progress stacks on `LoweringContext`, managed by `IrNodeLowering::from_ast`. If `check_cycle` finds a definition already on the stack, the cycle members and their spans are extracted for diagnostics.

## Diagnostics module

All diagnostic types are extracted into a crate-level `diagnostics` module (`src/diagnostics/`). The current `dsl::resolver::error` module (`error.rs`) is misleadingly named — it contains diagnostic types, not general error handling. It is replaced by this crate-level module. Diagnostics are not resolver-specific — the runtime also produces and renders diagnostics (runtime errors, timeout failures, match failures). A shared module avoids duplication and ensures consistent rendering across pipeline stages.

### Diagnostic struct

`Diagnostic` is the core rendering type, wrapping ariadne reports. Its constructor is private — the only way to produce a `Diagnostic` from outside the module is via `From<T>` impls.

```rust
#[derive(Debug)]
pub struct Diagnostic {
    severity: Severity,
    message: String,
    labels: Vec<ReportLabel>,
    help: Option<String>,
    note: Option<String>,
}

impl Diagnostic {
    /// Private — only callable from crate::diagnostics and children.
    fn new(severity: Severity, message: String) -> Self { ... }

    /// Public rendering.
    pub fn eprint(&self, source_map: &SourceTable) { ... }
}
```

### Module contents

The `diagnostics` module contains all types that implement `From<T> for Diagnostic`, along with their supporting types:

- `Diagnostic`, `Severity`, `ReportLabel` — core rendering types
- `LoweringBail` — resolver error enum (`Skip`, `Invalid`)
- `SkipReport`, `SkipEvaluation`, `DefinitionRef` — skip cause details
- `InvalidReport` — invalidity cause enum (cycle, purity violation, undefined names, etc.)
- `CycleReport`, `FnCycleEntry`, `EffectCycleEntry` — cycle details (embedded in `InvalidReport::Cycle`)
- `Cause`, `CauseId` — shared cause table entries
- `Warning`, `WarningId` — warning table entries (for future use, no variants defined in this RFC)
- `From<InvalidReport> for Diagnostic`, `From<SkipReport> for Diagnostic`, `From<Warning> for Diagnostic`

### Warning type

`Warning` is an enum — each variant defines its own fields since different warnings may reference different numbers of spans or no spans at all. It implements `From<Warning> for Diagnostic`. No warning variants are defined in this RFC; the type is introduced for future use.

```rust
#[derive(Debug)]
enum Warning {
    // Variants to be defined as needed. Examples:
    // StaticMarkerCondition { span: IrSpan },
    // UninitializedValue { name: String, span: IrSpan },
    // UnusedEffectShell { name: String, span: IrSpan },
    // DuplicateNeedAlias { effect: EffectId, span1: IrSpan, span2: IrSpan },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WarningId {
    id: String,  // stable mnemonic, same scheme as CauseId
}

type WarningTable = SharedTable<WarningId, Warning>;

impl From<Warning> for Diagnostic { ... }
```

### Warning reporting

Warnings follow the same pattern as causes: collected into a `WarningTable` on `LoweringContext` during resolution, printed once to stderr after resolution, and referenced by `WarningId` in `Plan` variants (see "Plan" and "Suite" sections above). Warnings never affect test outcomes — they are purely informational.

## Parser changes

Duration parsing (for `timeout` statements) is moved from the resolver to the parser. The resolver currently receives timeout values as raw strings and parses them during lowering. This is the parser's job — it should parse duration literals into structured values (`Duration` or a dedicated AST node) while preserving the original span for diagnostics. The resolver then simply copies the parsed value into the IR.

No other parser or lexer changes are required. The parser remains a pure `&str → AstModule` function with no awareness of `SharedTable`, `FileId`, or module paths. The resolver's module loading stage is responsible for placing parsed results into the shared tables.

## Scope

The scope of this RFC is the resolver only. The runtime will stop compiling due to changed IR types and removed data structures — this is expected. Runtime code that fails to compile should be commented out with `// TODO(R004)` markers. This RFC implementation is not expected to produce a working binary. Runtime adaptation is a separate follow-up.

## What is removed

- `CircularImport` diagnostic and the loader's `loading_stack`
- `ModuleScope`, `ModuleExports` structs
- `build_module_scope`, `build_module_exports` functions
- `scope.rs` module
- Eager discovery of all library files
- Per-plan `IndexVec<FnId, Function>`, `IndexVec<PureFnId, PureFunction>`, `IndexVec<EffectId, Effect>` (replaced by shared `HashMap` registries keyed by module-qualified `FnId`/`EffectId`)
- Old typed index newtypes `FnId`, `PureFnId` (replaced by unified `FnId { module, name, arity }`)
- `daggy::Dag` effect graph and `daggy` crate dependency (replaced by recursive `needs` on `IrEffect`)
- `EffectGraphBuilder`
- `FnKey` struct (replaced by `FnId` for global registry, `LocalFnKey` for local lookup)
- `FunctionRegistry` helper (replaced by `LocalTable`)
- Runtime evaluation of markers (evaluated eagerly during lowering)
- Timeout multiplier from the resolver — currently threaded through `LoweringContext` and applied during lowering. With duration parsing moved to the parser, the resolver passes through parsed `Duration` values as-is. The multiplier is applied at runtime only
- `RuntimeContext` / `SuiteContext` (plans are self-contained via embedded local tables)
- `dsl::resolver::error` module (replaced by crate-level `diagnostics` module)
- `DiagnosticWarning` enum (replaced by `Warning` enum in `diagnostics` module)
- `DiagnosticError` enum (replaced by specific types with `From<T> for Diagnostic` impls)
- `crate::error` module (replaced by crate-level `diagnostics` module)
- Auto-incremented `FileId` newtype index and `IndexVec<FileId, SourceFile>` (replaced by path-derived `FileId` and `FrozenTable<FileId, SourceFile>`)
- `SourceMap` struct (replaced by `SourceTable` type alias)
