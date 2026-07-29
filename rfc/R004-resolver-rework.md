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

Module-not-found and parse errors are recorded as `Cause::Invalid` and do not block loading of other modules; any test that transitively depends on a missing or unparseable module becomes `Plan::Invalid` referencing that cause. The `CircularImport` diagnostic is removed entirely.

### Stage 3: Plan Building (per test)

Walk the `AstTable`, find every `AstItem::Test`, and build a `Plan` for each. Name resolution, IR lowering, and validation all happen inline — no separate scope-building pass. Plan building evaluates the test's own markers, lowers the test, and wraps the result into a `Plan` variant (`Runnable`, `Skipped`, or `Invalid`). Each lowering call may recursively trigger lowering of dependencies (functions, effects) through the shared registries.

Name resolution from module A: look up in A's own AST items, then follow A's import declarations into the target module's AST items in the `AstTable`. The `ModuleScope`, `ModuleExports`, and their builder functions are removed.

#### Lowering with memoization

All lowering results are cached in shared Suite-level registries. For any definition (function, pure function, effect), the lowering process is:

1. Check registry — if entry exists, return cached status
2. Evaluate own markers — if skip triggers, insert `Skipped`, return
3. Lower dependencies — if any returns `Skipped` or `Invalid`, propagate and cache, return
4. Lower own body — if error, insert `Invalid`, return
5. Insert `Lowered(IR)`, return

Since markers are always checked before body lowering, `Skipped` takes natural precedence over `Invalid` — you never attempt to lower a body whose markers already skip it.

Skip and invalid statuses propagate transitively and are cached at every intermediate level. Since the DSL is declarative — no conditionals, no dynamic dispatch — the reachability graph from any definition is fixed, making cached results valid for all tests.

#### Test outcome categories

A test's plan building produces one of three `Plan` variants:

- **Runnable**: everything lowered successfully, ready for execution
- **Skipped**: a marker triggered on the test or any reachable dependency
- **Invalid**: a lowering error in the test or any reachable dependency

A test can have both skip and invalid causes simultaneously. Invalid takes precedence over skip when determining the plan variant. The plan carries all cause IDs regardless — the reporter can distinguish them. `Invalid` is distinct from a test failure (nothing executed) and from skip (not intentional). The suite continues running other tests regardless.

### Marker evaluation

Markers are pure expressions evaluated at resolve time. Given the same AST and environment, a marker always produces the same result — no shell state, no runtime context, no deferred evaluation. They are resolved eagerly during lowering rather than stored in the `Plan`.

Evaluation proceeds in two phases:

1. **Lower** the marker's condition expressions as `IrPureExpr`. This may trigger lowering (and caching) of any user-defined pure functions called in the expression. If any fails to lower (cycle, invalid expression), the marker evaluation fails and the definition becomes `Cause::Invalid`.
2. **Interpret** the lowered expression to a string value, then apply condition logic: bare (non-empty = true), equality, or regex match. Only environment variables are accessible via `${}` — no test/effect-scoped variables exist at marker-evaluation time.

The interpreter is a pure expression evaluator — a subset of the runtime VM with no shell, no captures, no mutable variables. It lives in a crate-level `evaluator` module (not in the resolver or runtime) and operates on IR types, so it can be shared by the resolver (marker evaluation) and the runtime (pure function execution), avoiding duplicated evaluation logic across stages.

**Caveat: Relux purity vs. FP purity.** In Relux, "pure" means "does not use a shell" — it does not mean referentially transparent. Some built-in functions like `uuid()` and `timestamp()` are pure in Relux's sense (no shell required) but non-deterministic. These are legal to call in marker conditions since the type system does not distinguish them, but they are semantically nonsensical there. This is an acceptable tradeoff: the marker system relies on Relux purity (shell-free evaluation), and users are expected to use deterministic expressions in marker conditions. This should be documented as a best-practice guideline rather than enforced by the resolver.

## IR conventions

### Naming and spans

All IR structs and enums are prefixed with `Ir`, mirroring the `Ast` prefix convention in the parser (`IrExpr`, `IrShellStmt`, `IrFn`, `IrEffect`, `IrTest`). The crate-level `Span` (opaque byte-offset pair) is wrapped with a `FileId` into `IrSpan` for cross-file diagnostics. Every IR node carries its own `IrSpan` — no `Spanned<T>` wrapper. The IR only reads spans, never mutates them; all span arithmetic happens at the AST level before lowering. Raw `String` names are replaced by dedicated `AstIdent`/`IrIdent` identifier types.

### AST -> IR mirroring

IR nodes map 1:1 to AST nodes. The IR does not restructure the AST — it enriches and validates it:

- **Span enrichment**: `Span` -> `IrSpan` (pairs with `FileId`)
- **Name resolution**: call names -> resolved function keys, effect names -> resolved identities
- **Validation**: purity enforcement, undefined name detection, regex validation
- **Removal**: comments dropped, imports resolved away, markers evaluated and removed
- **Timeout parsing**: moved to the parser

No structural transformations: statements stay statements, timed and untimed match variants stay distinct. Purity is enforced structurally by keeping `IrFn` and `IrPureFn` as separate types, with pure subsets of statements (`IrPureStmt`) and expressions (`IrPureExpr`) that cannot express shell operations or capture references. A `Send` in a pure body is a type-level absence caught at lowering as a diagnostic, not a runtime check.

### Lowering and caching

Lowering happens in a single traversal driven by an `IrNodeLowering` trait. When a `lower` implementation encounters a dependency (a function call, a `need`), it resolves it inline through a cached `from_ast` path that performs cache lookup, cycle detection (via in-progress stacks for functions and effects), and recursive lowering. Cacheable definitions (`IrFn`, `IrPureFn`, `IrEffect`) are memoized once in shared Suite-level registries keyed by module-qualified `FnId`/`EffectId` and reused by every test that reaches them. Built-in functions are pre-inserted into the registries under a reserved synthetic module path, so every local lookup table sees them without special-casing.

### Plans and the Suite

`resolve()` returns a `Suite` holding all `Plan`s, a shared source map, the environment snapshot used for marker evaluation, and the shared cause/warning tables. A `Runnable` plan is self-contained — its IR owns local lookup tables that hold `Arc` clones of the shared registries, so no external context is needed to execute it. `Skipped` and `Invalid` plans carry `CauseId`s that reference the shared `CauseTable`, which is printed once to stderr after resolution rather than duplicated per test. The immutable tables (`AstTable`, source map) are `Arc`-only; the IR registries and cause table use interior mutability since lowering populates them incrementally, which keeps the door open to parallel plan building with low contention.

### Effect graph

The `daggy::Dag`-based effect graph is replaced by a recursive structure: each `IrEffect` carries its resolved `needs`, and the full dependency graph is implicit (follow each need to its cached `IrEffect`, recurse). This is both composable (a test's graph is the union of its direct needs and their transitive needs) and cacheable (each effect resolved once) — `daggy` was neither, requiring index remapping to merge and rebuilding per test. Cycle detection uses the in-progress stacks managed by `from_ast`. Effect-instance identity uses a canonical overlay form (sorted entries, string-normalized) as a deduplication key; overlay values are still evaluated at runtime.

## Diagnostics module

All diagnostic types move into a crate-level `diagnostics` module shared by the resolver and runtime, since both produce and render diagnostics. `Diagnostic` wraps ariadne reports with a private constructor — the only way to produce one is via `From<T>` impls on the specific cause types (`InvalidReport`, `SkipReport`, `Warning`). Causes and warnings are collected into shared tables keyed by stable mnemonic IDs, printed once after resolution, and referenced by ID in `Plan` variants. Warnings never affect test outcomes. The misleadingly-named `dsl::resolver::error` module (it held diagnostic types, not error handling) is removed in favor of this shared module.

## Parser changes

Duration parsing (for `timeout` statements) moves from the resolver to the parser: the parser parses duration literals into structured values while preserving spans, and the resolver copies them into the IR. No other parser or lexer changes are required — the parser remains a pure `&str -> AstModule` function with no awareness of tables, `FileId`, or module paths.

## Scope

The scope of this RFC is the resolver only. The runtime will stop compiling due to changed IR types and removed data structures — this is expected. Runtime code that fails to compile should be commented out with `// TODO(R004)` markers. This RFC implementation is not expected to produce a working binary. Runtime adaptation is a separate follow-up.

## What is removed

- `CircularImport` diagnostic and the loader's `loading_stack`
- `ModuleScope`, `ModuleExports` structs
- `build_module_scope`, `build_module_exports` functions
- `scope.rs` module
- Eager discovery of all library files
- Per-plan `IndexVec` function/effect registries (replaced by shared `HashMap` registries keyed by module-qualified `FnId`/`EffectId`)
- Old typed index newtypes `FnId`, `PureFnId` (replaced by unified `FnId { module, name, arity }`)
- `daggy::Dag` effect graph and `daggy` crate dependency (replaced by recursive `needs` on `IrEffect`)
- `EffectGraphBuilder`
- `FnKey` struct (replaced by `FnId` for global registry, `LocalFnKey` for local lookup)
- `FunctionRegistry` helper (replaced by `LocalTable`)
- Runtime evaluation of markers (evaluated eagerly during lowering)
- Timeout multiplier from the resolver (duration parsing moves to the parser; the multiplier is applied at runtime only)
- `RuntimeContext` / `SuiteContext` (plans are self-contained via embedded local tables)
- `dsl::resolver::error` module (replaced by crate-level `diagnostics` module)
- `DiagnosticWarning` / `DiagnosticError` enums (replaced by specific types with `From<T> for Diagnostic` impls)
- `crate::error` module (replaced by crate-level `diagnostics` module)
- Auto-incremented `FileId` newtype index and `IndexVec<FileId, SourceFile>` (replaced by path-derived `FileId` and a frozen table)
- `SourceMap` struct (replaced by a `SourceTable` type alias)
