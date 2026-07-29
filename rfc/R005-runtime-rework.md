# R005: Runtime Rework

- **Status**: implemented
- **Created**: 2026-03-25

## Motivation

The R004 resolver rework replaced the old IR with a new type system (`IrTest`, `IrEffect`, `IrShellStmt`, etc.) and removed the structures the runtime depended on (`Plan`, `EffectGraph`, `IndexVec`-based registries in `legacy.rs`), leaving the runtime's `CodeServer` and `ScopeStack` orphaned from the new IR types. The `cmd_run` entry point is currently stubbed with `todo!("R004: runtime adaptation")`. The runtime code still compiles against the legacy IR types in `legacy.rs`, but cannot execute anything.

This RFC adapts the runtime to consume the new IR directly and removes the legacy IR bridge.

## Blocking Prerequisite: Pure IR Types

This change must land before R005 implementation begins. The resolver currently uses `IrLetStmt` (containing `IrExpr`) for test/effect-level `let` items, and `IrExpr` for overlay values. These positions have no shell context and cannot execute impure expressions — they are semantically pure. Switching them to the pure IR types (`IrPureLetStmt` / `IrPureExpr`, which already exist) enforces purity at resolve time and lets the runtime call `crate::evaluator` directly with no `IrExpr` -> `IrPureExpr` conversion layer at every evaluation site. Concretely: `IrTestItem::Let` and `IrEffectItem::Let` use `IrPureLetStmt`, and `IrOverlayEntry.value` uses `IrPureExpr`.

## Design

### Execution Context

The VM operates exclusively through an `ExecutionContext` — it never accesses config, test metadata, or environment directly. The context holds:

- **`Scope`** (`Test` or `Effect`): shared across all shell blocks in the test/effect via an `Arc<Mutex<VarScope>>`, so a `let` in one shell block is visible in later ones. An effect scope also carries an immutable `env_overlay` (process env plus the effect's overlay, merged).
- **`ShellState`**: per-shell mutable state — local vars, captures, timeout, fail pattern, and (for exported effect shells) an inherited `env_overlay`.
- **`CallFrame` stack**: function-call nesting; each frame is an independent barrier.
- **`default_timeout`** (from `RunContext`) and the process **`env`** snapshot, as final fallbacks.

**Captures** (indexed `${0}`, `${1}`, ... and named `${name}`) come from the most recent regex match and live on the current context.

**Timeout** is an enum: `Tolerance` carries a multiplier (scaled by `--timeout-multiplier`), `Assertion` has no multiplier and is never scaled. The orchestrator applies the multiplier to the default timeout at construction; per-statement `~` timeouts inherit the multiplier, `@` assertion timeouts never scale.

**Names.** Each component carries the name the user wrote in source (test name, effect name, shell name/alias, call-site name reflecting import aliases). `current_name()` returns the top call frame's name, else the shell alias, else the shell name — so logs show what the user sees (a function imported as `my_func as mf` logs as `mf`).

**Variable lookup** depends on whether the call stack is active:

- **Inside a function call**: top call frame vars, then env. A function sees only its own arguments and locals plus environment variables — never the caller's variables, captures, overlay, or scope globals. This is a hard barrier.
- **Direct shell execution**: shell vars, shell `env_overlay`, scope (test/effect) vars, effect `env_overlay`, then env.

**Timeout and fail pattern** are per-context state set by `~` and `!?`/`!=`, applied to the current context (top call frame, or shell state if the call stack is empty) and not propagated to parents. The effective match timeout walks: top call frame, shell state, then `default_timeout`. `Scope.timeout` is not in this chain — it is the per-test deadline used by the orchestrator's `tokio::time::timeout` wrapper, as is the suite-level timeout.

**Shell export.** When an effect's shell is exported to a test (`need Effect as alias`), `reset_for_export` swaps in the caller's scope and clears effect-internal `vars`/`captures`, while preserving `timeout`, `fail_pattern`, and `env_overlay`. This transfers the effect's behavioral contract with the shell while keeping implementation details private; the scope swap makes the test's `let` variables visible in the exported shell. Exported VMs are shared via `Arc<tokio::sync::Mutex<Vm>>`, so diamond dependencies (two effects needing the same child) alias the same VM; deduplication by `Arc` pointer ensures each VM is reset exactly once per acquisition level.

### Effect Manager

The `EffectManager` coordinates effect lifecycle — bootstrap, deduplication, and cleanup — and is used by both tests and effects to instantiate their dependencies. Its `EffectRegistry` holds one slot per effect instance, keyed by `(effect_id, canonical_overlay)`; a slot is `Empty`, `Ready { refcount, handle }`, or `Failed(failure)` (cached, so all later acquirers get the same failure). A brief outer mutex guards the slot map; each slot has its own `tokio` mutex, so distinct instances bootstrap concurrently.

- **Acquire**: `Empty` triggers bootstrap; `Ready` increments the refcount and returns the shared exported VM `Arc`; `Failed` returns the cached failure. When two parents need the same effect, the first holds the slot lock through bootstrap while the second blocks, then sees `Ready`. Cycles are impossible (rejected by the resolver).
- **Bootstrap**: recursively instantiate sub-dependencies, build the shell map from their exported shells, evaluate the overlay into an `env_overlay` (pure expressions over env only), create the effect scope, reset the imported VMs to that scope, walk the effect items (lets, shells, cleanup), extract the exported shell, and terminate the non-exported ones.
- **Cleanup**: refcount-based. The last releaser shuts down the exported VM, runs the cleanup block in a fresh shell (best-effort per R002 — failures become warnings, never test failures), then recursively releases dependencies in parallel.

`instantiate` and `cleanup` both operate on a slice of needs, acquiring/releasing in parallel.

### Run

The orchestrator receives a `Suite` from the resolver and executes each plan. `Runnable` plans go through a `run_test` / `run_test_body` path that creates the test scope, instantiates effects (parallel, recursive), swaps each exported VM into the test scope, walks the test items (lets via the shared evaluator, shell blocks), terminates shells in reverse order, and runs test cleanup (fresh shell, best-effort). `Skipped` and `Invalid` plans emit their result from the cause IDs with no execution.

Tests run one at a time — no concurrent test execution — each with a fresh `EffectManager`, so effect instances are deduplicated within a single test's dependency tree but never shared across tests. Per-test and suite-level timeouts are `tokio::time::timeout` wrappers around the test future and the whole run, respectively; an inline test timeout takes precedence over the config/manifest default. Fail-fast stops after the first failure.

Configuration reaches the runtime as a plain `RunContext` (run id and directories, shell command/prompt, default/test/suite timeouts, strategy), replacing the old `Runtime` struct. The base `Env` merges the process environment with relux-specific variables (`__RELUX_RUN_ID`, `__RELUX_TEST_ARTIFACTS`, `__RELUX_SHELL_PROMPT`, `__RELUX_SUITE_ROOT`, `__RELUX_EXECUTABLE`), and each test adds `__RELUX_TEST_ROOT` pointing at its file's directory.

**Condition markers** are evaluated entirely at resolve time — by the time the runtime receives a `Suite`, every test is already `Runnable`, `Skipped`, or `Invalid`. The runtime never evaluates conditions; the old `evaluate_conditions()` and its tests are deleted.

**Warnings** (cleanup failures, per R002) are collected during execution and attached to the `TestResult`; they never change the outcome. The reporter renders them after the outcome (e.g. "passed (1 warning: effect Db cleanup failed: match timeout)").

### VM

The VM owns a PTY and an `ExecutionContext`; its public interface is `exec_stmts` and `shutdown`. Statement dispatch, expression evaluation, and function calls are all `impl Vm` methods operating through the context — the VM never touches config, metadata, or env directly.

Function-call dispatch keys `IrCallExpr.resolved` (an `FnId`) directly into the shared `FnTable`/`PureFnTable` — no `CodeServer` lookup, since the resolver already validated and resolved every call. User-defined impure functions push a call frame and execute their body; pure functions delegate to the shared `crate::evaluator`; BIFs dispatch by name. `interpolate`, `lookup`, `capture`, `let_insert`, `assign`, the timeout/fail-pattern accessors, and `current_name` become `ExecutionContext` methods that operate on the current active context (top call frame, else shell state).

The PTY plumbing carries over unchanged: `OutputBuffer`, the wait-loop-with-fail-check match logic, `check_fail`, `send_bytes`, `shutdown`, prompt init, the BIF traits and implementations, and the event/log emission points (same `LogEventKind` variants, now over the new IR types). Removed: `CodeServer`/`Callable`, `ScopeStack`, the `legacy.rs` IR types and `Timeout` enum, `runtime::pure::exec_pure_body` (replaced by the shared evaluator), the `Runtime`/`TestRunContext` structs, and `evaluate_conditions`.

### Module organization

The flat `runtime/` layout is regrouped by responsibility into `vm/` (VM, BIFs, execution context), `effect/` (manager, registry), `report/` (results, HTML/JUnit/TAP), and `observe/` (event log, shell log, progress), with orchestration in `mod.rs`. `crate::evaluator` stays at the crate root as shared infrastructure used by both the resolver (marker evaluation) and the runtime (test/effect lets, overlay evaluation) — moving it under `runtime/` would invert the pipeline into a `resolver -> runtime` dependency.
