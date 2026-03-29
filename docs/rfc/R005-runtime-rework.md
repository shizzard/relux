# R005: Runtime Rework

- **Status**: implemented
- **Created**: 2026-03-25

## Motivation

The R004 resolver rework replaced the old IR with a new type system (`IrTest`, `IrEffect`, `IrShellStmt`, etc.) and removed the structures the runtime depended on (`Plan`, `EffectGraph`, `IndexVec`-based registries in `legacy.rs`), leaving the runtime's `CodeServer` and `ScopeStack` orphaned from the new IR types. The `cmd_run` entry point is currently stubbed with `todo!("R004: runtime adaptation")`. The runtime code still compiles against the legacy IR types in `legacy.rs`, but cannot execute anything.

This RFC adapts the runtime to consume the new IR directly and removes the legacy IR bridge.

## Blocking Prerequisite: Pure IR Types

**This change must land before R005 implementation begins.** The resolver currently uses `IrLetStmt` (containing `IrExpr`) for test/effect-level `let` items, and `IrExpr` for `IrOverlayEntry.value`. These positions have no shell context and cannot execute impure expressions — they are semantically pure. Without this change, the runtime would need a type conversion layer from `IrExpr` to `IrPureExpr` at every evaluation site.

Required changes:

- `IrTestItem::Let` → use `IrPureLetStmt` (contains `IrPureExpr`) instead of `IrLetStmt`
- `IrEffectItem::Let` → use `IrPureLetStmt` instead of `IrLetStmt`
- `IrOverlayEntry.value` → use `IrPureExpr` instead of `IrExpr`
- `IrOverlayEntry::new()` → accept `IrPureExpr` instead of `IrExpr`
- `IrOverlayEntry::value()` → return `&IrPureExpr` instead of `&IrExpr`

`IrPureLetStmt` and `IrPureExpr` already exist in the IR. The lowering code in `effect.rs` and `test_def.rs` must switch from `IrLetStmt::lower` to `IrPureLetStmt::lower`, and `IrExpr::lower` to `IrPureExpr::lower` for overlay entries. This enforces purity at resolve time and lets the runtime call `crate::evaluator` directly with no type conversion.

## Design

### Execution Context

The `ExecutionContext` is the runtime state for a single VM. It lives in `crate::stack` alongside `Env` and `VarScope`. The VM operates exclusively through the execution context — it does not access config, test metadata, or environment directly.

#### Types

```rust
struct ExecutionContext {
    scope: Scope,                    // test or effect — shared across shells
    shell: ShellState,               // per-shell mutable state
    call_stack: Vec<CallFrame>,      // function call nesting
    default_timeout: IrTimeout,      // from RunContext — final fallback for match operations
    env: Arc<Env>,                   // process env snapshot, final fallback
}

enum Scope {
    Test {
        name: String,                // test name, from TestMeta.name
        vars: Arc<Mutex<VarScope>>,  // shared across test's shell blocks
        timeout: Option<IrTimeout>,
    },
    Effect {
        name: String,                // effect name, from IrEffect.name
        vars: Arc<Mutex<VarScope>>,  // shared across effect's shell blocks
        _timeout: Option<IrTimeout>,   // reserved — always None until effect-level timeouts are implemented
        env_overlay: Arc<Env>,       // process env + overlay merged; immutable after construction
    },
}

impl Scope {
    fn name(&self) -> &str {
        match self {
            Scope::Test { name, .. } | Scope::Effect { name, .. } => name,
        }
    }

    fn vars(&self) -> &Arc<Mutex<VarScope>> {
        match self {
            Scope::Test { vars, .. } | Scope::Effect { vars, .. } => vars,
        }
    }
}

struct ShellState {
    name: String,                    // shell block name, from IrShellBlock.name
    alias: Option<String>,           // effect shell alias, from IrEffectNeed.alias
    vars: VarScope,
    captures: Captures,
    timeout: Option<IrTimeout>,
    fail_pattern: Option<FailPattern>,
    env_overlay: Option<Arc<Env>>,   // inherited from effect on export, None for test-local shells
}

struct CallFrame {
    name: String,                    // call-site name, from IrCallExpr.name (reflects import aliases)
    vars: VarScope,
    captures: Captures,
    timeout: Option<IrTimeout>,
    fail_pattern: Option<FailPattern>,
}
```

`Env` and `VarScope` already exist in `crate::stack`. `FailPattern` (currently in `runtime::vars`) is moved to `crate::stack`. `Captures` is a new type added to `crate::stack`. The legacy `Timeout` enum in `legacy.rs` is deleted — `IrTimeout` is used directly (see Timeout section below).

```rust
/// Ordered capture groups from the most recent regex match.
/// Index 0 is the full match, 1+ are numbered groups, named groups
/// are keyed by name.
struct Captures {
    indexed: Vec<String>,            // ${0}, ${1}, ...
    named: HashMap<String, String>,  // ${name}
}

impl Captures {
    fn new() -> Self { Self { indexed: Vec::new(), named: HashMap::new() } }
    fn get_indexed(&self, i: usize) -> Option<&str> { self.indexed.get(i).map(|s| s.as_str()) }
    fn get_named(&self, name: &str) -> Option<&str> { self.named.get(name).map(|s| s.as_str()) }
}
```

#### Timeout

`IrTimeout` becomes an enum. `IrTimeoutKind` is removed. Tolerance timeouts carry the multiplier; assertion timeouts have no multiplier field:

```rust
#[derive(Debug, Clone)]
pub enum IrTimeout {
    Tolerance { duration: Duration, multiplier: f64, span: IrSpan },
    Assertion { duration: Duration, span: IrSpan },
}

impl IrTimeout {
    /// Convenience constructor for config-derived tolerance timeouts (no source span).
    pub fn tolerance(duration: Duration) -> Self {
        Self::Tolerance { duration, multiplier: 1.0, span: IrSpan::SYNTHETIC }
    }

    /// Apply a multiplier. Tolerance timeouts store it; assertion timeouts are unaffected.
    pub fn apply_multiplier(&mut self, m: f64) {
        if let Self::Tolerance { multiplier, .. } = self {
            *multiplier = m;
        }
    }

    /// Effective duration: raw duration × multiplier for tolerance, raw duration for assertion.
    pub fn value(&self) -> Duration {
        match self {
            Self::Tolerance { duration, multiplier, .. } => duration.mul_f64(*multiplier),
            Self::Assertion { duration, .. } => *duration,
        }
    }
}
```

The orchestrator calls `apply_multiplier()` on the `RunContext.default_timeout` during construction (from the `--timeout-multiplier` CLI flag). Per-statement `~` timeouts carry their multiplier from the default at creation time. `@` assertion timeouts are never scaled.

`Scope` is `Clone` — all fields are `Arc` or `Copy`. The orchestrator creates one `Scope::Test` or `Scope::Effect`, clones it into each VM's `ExecutionContext`. Multiple shell blocks within the same test or effect share the same `vars` through the `Arc<Mutex<VarScope>>`. A `let` in one shell block is visible to subsequent shell blocks.

`Scope.env_overlay` is `Arc<Env>` — immutable after construction, cheap to clone into exported shell states.

#### Names

Each component carries a name that matches what the user wrote in their `.relux` source. The VM passes `ctx.current_name()` to the event collector for rich log output without computing names itself.

| Component | `name` source | `alias` source |
|-----------|---------------|----------------|
| `Scope::Test` | `TestMeta.name` — the test's quoted name | — |
| `Scope::Effect` | `IrEffect.name` — the effect definition name | — |
| `ShellState` | `IrShellBlock.name` — the shell block identifier | `IrEffectNeed.alias` — the alias from a `need` declaration, if present |
| `CallFrame` | `IrCallExpr.name` — the name at the call site, reflecting import aliases | — |

`current_name()` returns: the top call frame's name if the call stack is non-empty, otherwise the shell's alias if present, otherwise the shell's name. This ensures logs show the name the user sees in their code — if a function is imported as `mf`, the log says `mf`, not the original `my_func`.

#### Variable lookup

Lookup depends on whether the call stack is active:

**With call stack non-empty** (inside a function call):

1. Check top `CallFrame.vars`
2. Fall through to `ExecutionContext.env`

A function only sees: its own arguments (pre-filled into `vars`), its own local variables, and environment variables. It cannot see the caller's variables, captures, overlay, or scope-level globals. This is a hard barrier.

**With call stack empty** (direct shell execution):

1. Check `ShellState.vars`
2. Check `ShellState.env_overlay` (if present)
3. Check `Scope.vars` (test or effect globals)
4. If `Scope::Effect`: check `Scope.env_overlay`
5. Fall through to `ExecutionContext.env`

#### Function calls

- **`push_call(name, args)`** — pushes a `CallFrame`. Copies timeout and fail pattern from the current active context (top call frame, or shell state if call stack is empty). Pre-fills `vars` with the provided `(name, value)` argument pairs. Fresh captures.
- **`pop_call()`** — removes the top `CallFrame`.

When a function returns (call frame popped), its timeout and fail pattern changes are discarded — the caller's context is unaffected. Nested function calls stack: each is an independent barrier.

#### Timeout and fail pattern

Timeout and fail pattern are per-context state, modified by `~` and `!?`/`!=` statements:

- **`~` (timeout)**: sets `timeout` on the current context (top call frame, or `ShellState` if call stack is empty). Does not propagate to parent contexts.
- **`!?`/`!=` (fail pattern)**: sets `fail_pattern` on the current context (top call frame, or `ShellState`).
- **`!clear`**: clears `fail_pattern` on the current context.

`timeout()` returns the effective timeout by walking the fallback chain:

1. Top `CallFrame.timeout` (if call stack non-empty)
2. `ShellState.timeout`
3. `ExecutionContext.default_timeout` (from `RunContext`, always present)

`Scope.timeout` is **not** part of this chain — it is the per-test deadline used by the orchestrator's `tokio::time::timeout` wrapper, not a match-level default. The suite-level timeout (`RunContext.suite_timeout`) similarly wraps the entire run and is never consulted by the VM.

#### Shell export

When an effect's shell is exported to the test (via `need Effect as alias`), the VM calls `reset_for_export()` which clears implementation details but preserves the behavioral contract:

```rust
impl Vm {
    fn reset_for_export(&mut self, new_scope: Scope) {
        self.ctx.scope = new_scope;
        self.ctx.shell.vars = VarScope::new();
        self.ctx.shell.captures = Captures::new();
        // timeout, fail_pattern, env_overlay are preserved
    }
}
```

After export:
- **`scope`**: replaced — the VM adopts the caller's scope (test or parent effect), so scope-level `let` variables and the caller's `Arc<Mutex<VarScope>>` are visible through the lookup chain
- **PTY**: owned by the VM, shared via `Arc<Mutex<Vm>>`
- **`env_overlay`**: preserved — overlay variables remain accessible
- **`timeout`**: preserved — effect's configured timeout carries over
- **`fail_pattern`**: preserved — effect's configured fail pattern carries over
- **`vars`**: cleared — effect-internal variables are not exposed
- **`captures`**: cleared — effect-internal captures are not exposed

This ensures the effect's behavioral contract transfers with the shell, while implementation details remain private. The scope swap means the test's `let` variables are visible in the exported shell — the shell participates in the test's variable sharing just like a test-local shell would. The overlay flows from the test to the effect (`need Db(port="5432")`), and back to the test through the exported shell's `env_overlay`.

The exported VM is always shared via `Arc<tokio::sync::Mutex<Vm>>`. Multiple aliases may point to the same Arc — when two needs resolve to the same effect instance (same identity + overlay), `acquire` returns clones of the same Arc. Callers lock the mutex when executing shell blocks.

In diamond dependencies (two effects both need the same child effect), both parents lock the child's mutex to execute their shell blocks during bootstrap. The slot-level locking ensures sequential access.

`reset_for_export(new_scope)` is called by the caller — not by `bootstrap_effect` or `acquire` — because only the caller knows the target scope. When a test acquires exported VMs, it passes its `Scope::Test`. When a parent effect acquires child VMs, it passes its `Scope::Effect`. Deduplication by Arc pointer ensures each VM is reset exactly once per acquisition level, even when multiple aliases point to the same instance.

In a diamond dependency (effects A and B both need child C), the child VM is reset twice across the lifecycle: once when it's exported from C to whichever parent bootstraps first, and again when it's exported from the final parent to the test. Each reset replaces the scope — the last caller's scope wins. This is correct: the child VM ultimately participates in the test's variable sharing, and intermediate scopes are transient. The deduplication within a single bootstrap (step 4b's `reset_seen` set) prevents redundant resets at the same level — e.g., if an effect acquires the same child through two aliases, the child is reset once with that effect's scope, not twice.

### Effect Manager

The `EffectManager` coordinates effect lifecycle — bootstrap, deduplication, and cleanup. Both tests and effects use the same `EffectManager` to instantiate their dependencies. The manager is `Clone` (all fields are `Arc` or trivially clonable) and passed into recursive calls.

#### Types

```rust
#[derive(Clone)]
struct EffectManager {
    registry: Arc<EffectRegistry>,
    effect_table: EffectTable,       // SharedTable — already Arc internally
    fn_table: FnTable,               // SharedTable — already Arc internally
    pure_fn_table: PureFnTable,      // SharedTable — already Arc internally
    env: Arc<Env>,
    shell_command: Arc<str>,
    shell_prompt: Arc<str>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct EffectInstanceKey {
    effect_id: DiagEffectId,
    canonical_overlay: String,
}

impl From<&IrEffectNeed> for EffectInstanceKey {
    fn from(need: &IrEffectNeed) -> Self {
        Self {
            effect_id: need.effect().clone(),
            canonical_overlay: need.canonical_overlay().to_string(),
        }
    }
}

struct EffectRegistry {
    slots: std::sync::Mutex<HashMap<EffectInstanceKey, Arc<tokio::sync::Mutex<EffectSlot>>>>,
}

enum EffectSlot {
    Empty,
    Ready {
        refcount: usize,
        handle: EffectHandle,
    },
    Failed(Failure),  // cached — all subsequent acquirers get this failure
}

struct EffectHandle {
    scope: Scope,                              // Scope::Effect — shared vars for cleanup access
    exported_vm: Arc<tokio::sync::Mutex<Vm>>,  // shared via Arc for diamond dependencies
    dependencies: Vec<EffectInstanceKey>,       // for recursive cleanup
    cleanup: Option<IrCleanupBlock>,
}
```

#### Public API

The `EffectManager` exposes two methods. Both test and effect init code use the same interface:

```rust
impl EffectManager {
    /// Acquire all needs in parallel. Each need recursively acquires its own
    /// dependencies before bootstrapping itself.
    /// The same Arc may appear multiple times if two needs resolve to the same
    /// effect instance (different aliases, same identity + overlay).
    async fn instantiate(&self, needs: &[IrEffectNeed]) -> Result<Vec<Arc<tokio::sync::Mutex<Vm>>>, Failure> {
        let futures: Vec<_> = needs.iter().map(|need| {
            let key = EffectInstanceKey::from(need);
            self.registry.acquire(&key, need, self)
        }).collect();
        try_join_all(futures).await
    }

    /// Release all needs in parallel. Refcount-based — last releaser runs
    /// cleanup and recursively releases dependencies.
    /// Returns warnings from any cleanup failures (best-effort per R002).
    async fn cleanup(&self, needs: &[IrEffectNeed]) -> Vec<Warning> {
        let futures: Vec<_> = needs.iter().map(|need| {
            let key = EffectInstanceKey::from(need);
            self.registry.run_cleanup(&key, self)
        }).collect();
        join_all(futures).await.into_iter().flatten().collect()
    }
}
```

#### Slot locking

The outer `std::sync::Mutex` on the `HashMap` is held only for the brief duration of a slot lookup/insert. Each slot has its own `tokio::sync::Mutex`, locked independently. Multiple effects bootstrap concurrently as long as they are different instances.

When two parents need the same effect, the first acquirer holds the slot lock for the entire duration of bootstrap. The second acquirer blocks on the slot's `tokio::sync::Mutex` until bootstrap completes, then sees `Ready` and increments the refcount. This is safe because cycles are impossible (rejected by the resolver).

```rust
impl EffectRegistry {
    fn new() -> Self {
        Self {
            slots: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn slot(&self, key: &EffectInstanceKey) -> Arc<tokio::sync::Mutex<EffectSlot>> {
        self.slots.lock().expect("slot map mutex poisoned")
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(EffectSlot::Empty)))
            .clone()
        // outer lock released here — slot lock acquired separately by caller
    }
}
```

#### Acquire

```rust
async fn acquire(
    &self,
    key: &EffectInstanceKey,
    need: &IrEffectNeed,
    manager: &EffectManager,
) -> Result<Arc<tokio::sync::Mutex<Vm>>, Failure> {
    let slot = self.slot(key);
    let mut guard = slot.lock().await;

    match &mut *guard {
        EffectSlot::Ready { refcount, handle } => {
            *refcount += 1;
            Ok(handle.exported_vm.clone())
        }
        EffectSlot::Failed(failure) => {
            Err(failure.clone())
        }
        EffectSlot::Empty => {
            match manager.bootstrap_effect(need).await {
                Ok(handle) => {
                    let vm_arc = handle.exported_vm.clone();
                    *guard = EffectSlot::Ready { refcount: 1, handle };
                    Ok(vm_arc)
                }
                Err(failure) => {
                    *guard = EffectSlot::Failed(failure.clone());
                    Err(failure)
                }
            }
        }
    }
}
```

#### Bootstrap

```rust
impl EffectManager {
async fn bootstrap_effect(
    &self,
    need: &IrEffectNeed,
) -> Result<EffectHandle, Failure> {
    let effect = self.effect_table.get(need.effect())
        .expect("effect must exist in table")
        .expect("effect must have lowered successfully");

    // 1. Recursively instantiate sub-dependencies
    let exported_deps = self.instantiate(effect.needs()).await?;

    // 2. Build shell map from dependency exported shells
    //    The same Arc may appear under multiple aliases (diamond dependencies).
    //    NOTE: reset_for_export() is deferred to after step 4 (scope creation),
    //    so that child VMs adopt this effect's scope.
    let mut shells: HashMap<String, Arc<tokio::sync::Mutex<Vm>>> = HashMap::new();
    for (sub_need, vm_arc) in effect.needs().iter().zip(exported_deps) {
        if let Some(alias) = sub_need.alias() {
            shells.insert(alias.to_string(), vm_arc);
        }
    }

    // 3. Evaluate overlay → build env_overlay
    //    Each IrOverlayEntry has a key and a pure expression value.
    //    Evaluate values against the parent env, merge with env.
    //    Overlay values are evaluated with an empty VarScope — they can only
    //    reference environment variables and call pure functions.
    let env_overlay = {
        let mut overlay = self.env.as_ref().clone();
        let empty_vars = VarScope::new();
        for entry in need.overlay() {
            let value = crate::evaluator::eval_pure_expr(
                entry.value(), &empty_vars, &self.env, &self.pure_fn_table,
            );
            overlay.insert(entry.key().name().to_string(), value);
        }
        Arc::new(overlay)
    };

    // 4. Create effect scope
    let scope = Scope::Effect {
        name: effect.name().to_string(),
        vars: Arc::new(Mutex::new(VarScope::new())),
        _timeout: None,
        env_overlay: env_overlay.clone(),
    };

    // 4b. Reset imported VMs — swap their scope to this effect's scope
    let mut reset_seen = std::collections::HashSet::new();
    for (_, vm_arc) in &shells {
        let ptr = Arc::as_ptr(vm_arc) as usize;
        if reset_seen.insert(ptr) {
            vm_arc.lock().await.reset_for_export(scope.clone());
        }
    }

    // 5. Walk IrEffectItems
    let mut cleanup_block = None;
    for item in effect.body() {
        match item {
            IrEffectItem::Comment { .. } => continue,
            IrEffectItem::Need { .. } => continue,   // already instantiated in step 1
            IrEffectItem::Let { stmt, .. } => {
                // evaluate pure expr via crate::evaluator, insert into scope.vars
                let vars = scope.vars().lock().await;
                let value = crate::evaluator::eval_pure_expr(
                    stmt.value(), &vars, &env_overlay, &self.pure_fn_table,
                );
                drop(vars);
                scope.vars().lock().await.insert(stmt.name().to_string(), value);
            }
            IrEffectItem::Shell { block, .. } => {
                let name = block.name().to_string();
                if !shells.contains_key(&name) {
                    let ctx = ExecutionContext::new(scope.clone(), ShellState::new(&name));
                    let vm = Vm::new(ctx, &self.shell_command, &self.shell_prompt,
                                     self.fn_table.clone(), self.pure_fn_table.clone(),
                                     None, Path::new(""), Instant::now(), None).await?;
                    shells.insert(name.clone(), Arc::new(tokio::sync::Mutex::new(vm)));
                }
                let vm_arc = shells.get(&name).expect("shell just inserted above");
                let _ = vm_arc.lock().await.exec_stmts(block.body()).await?;
            }
            IrEffectItem::Cleanup { block, .. } => {
                cleanup_block = Some(block.clone());
            }
        }
    }

    // 6. Extract exported shell
    let exported_name = effect.exported_shell().to_string();
    let exported_vm = shells.remove(&exported_name)
        .expect("exported shell must exist");

    // 7. Terminate non-exported shells
    //    Deduplicate by Arc pointer — diamond dependencies may alias the same VM.
    let mut seen = std::collections::HashSet::new();
    for (_, vm_arc) in shells.drain() {
        let ptr = Arc::as_ptr(&vm_arc) as usize;
        if seen.insert(ptr) {
            vm_arc.lock().await.shutdown().await;
        }
    }

    // 8. Return handle — reset_for_export() called by the caller who knows the target scope
    Ok(EffectHandle {
        scope,
        exported_vm,
        dependencies: effect.needs().iter().map(EffectInstanceKey::from).collect(),
        cleanup: cleanup_block,
    })
}
} // impl EffectManager
```

#### Cleanup

```rust
async fn run_cleanup(
    &self,
    key: &EffectInstanceKey,
    manager: &EffectManager,
) -> Vec<Warning> {
    let slot = self.slot(key);
    let mut guard = slot.lock().await;
    let mut warnings = Vec::new();

    match &mut *guard {
        EffectSlot::Ready { refcount, handle } => {
            *refcount -= 1;

            if *refcount == 0 {
                let effect_name = handle.scope.name().to_string();

                // 1. Shut down the exported VM (the effect's main PTY)
                handle.exported_vm.lock().await.shutdown().await;

                // 2. Run cleanup block in fresh shell (best-effort per R002)
                if let Some(cleanup_block) = &handle.cleanup {
                    let ctx = ExecutionContext::new(
                        handle.scope.clone(), ShellState::new("cleanup"),
                    );
                    let cleanup_vm = Vm::new(
                        ctx, &manager.shell_command, &manager.shell_prompt,
                        manager.fn_table.clone(), manager.pure_fn_table.clone(),
                        None, Path::new(""), Instant::now(), None,
                    ).await;
                    if let Ok(mut vm) = cleanup_vm {
                        if let Err(failure) = vm.exec_stmts(cleanup_block.body()).await {
                            warnings.push(Warning::CleanupFailed {
                                source: CleanupSource::Effect { name: effect_name },
                                failure,
                            });
                        }
                        vm.shutdown().await;
                    }
                }

                let deps = handle.dependencies.clone();
                *guard = EffectSlot::Empty;
                drop(guard);

                // 3. Recursively release dependencies (parallel)
                let futures: Vec<_> = deps.iter()
                    .map(|dep| self.run_cleanup(dep, manager))
                    .collect();
                let child_warnings: Vec<Warning> = join_all(futures).await
                    .into_iter().flatten().collect();
                warnings.extend(child_warnings);
            }
        }
        EffectSlot::Failed(_) => {
            // nothing to clean up — bootstrap never completed
        }
        EffectSlot::Empty => unreachable!("releasing unacquired effect"),
    }

    warnings
}
```

### Run

The orchestrator receives a `Suite` from the resolver and executes each plan.

#### Test run flow

For each `Plan::Runnable`, the `run_test` function (shown in the entry point section) handles setup, progress, timing, and cleanup. It delegates to `run_test_body` for the actual test execution:

```rust
async fn run_test_body(
    meta: &TestMeta,
    test: &IrTest,
    manager: &EffectManager,
    warnings: &mut Vec<Warning>,
    env: &Arc<Env>,
    log_dir: &Path,
    test_start: Instant,
    event_collector: &EventCollector,
    progress_tx: ProgressTx,
) -> Result<(), Failure> {
    // 1. Create test scope
    let scope = Scope::Test {
        name: meta.name().to_string(),
        vars: Arc::new(Mutex::new(VarScope::new())),
        timeout: meta.timeout().cloned(),
    };

    // 2. Instantiate effects — parallel, recursive
    //    The same Arc may appear multiple times (different aliases, same instance).
    let exported = manager.instantiate(test.needs()).await?;

    // 3. Build shell map from exported effect shells
    //    Swap each VM's scope from Scope::Effect to this test's Scope::Test,
    //    clear effect-internal vars/captures, preserve timeout/fail_pattern/env_overlay.
    let mut shells: IndexMap<String, Arc<tokio::sync::Mutex<Vm>>> = IndexMap::new();
    let mut reset_seen = std::collections::HashSet::new();
    for (need, vm_arc) in test.needs().iter().zip(exported) {
        let ptr = Arc::as_ptr(&vm_arc) as usize;
        if reset_seen.insert(ptr) {
            vm_arc.lock().await.reset_for_export(scope.clone());
        }
        if let Some(alias) = need.alias() {
            shells.insert(alias.to_string(), vm_arc);
        }
    }

    // 4. Walk IrTestItems
    let mut cleanup_block = None;
    let body_result: Result<(), Failure> = async {
        for item in test.body() {
            match item {
                IrTestItem::Comment { .. } | IrTestItem::DocString { .. } => continue,
                IrTestItem::Need { .. } => continue,
                IrTestItem::Let { stmt, .. } => {
                    let vars = scope.vars().lock().await;
                    let value = crate::evaluator::eval_pure_expr(
                        stmt.value(), &vars, &manager.env, &manager.pure_fn_table,
                    );
                    drop(vars);
                    scope.vars().lock().await.insert(stmt.name().to_string(), value);
                }
                IrTestItem::Shell { block, .. } => {
                    let name = block.name().to_string();
                    if !shells.contains_key(&name) {
                        let ctx = ExecutionContext::new(scope.clone(), ShellState::new(&name));
                        let vm = Vm::new(ctx, &manager.shell_command, &manager.shell_prompt,
                                         manager.fn_table.clone(), manager.pure_fn_table.clone(),
                                         Some(progress_tx.clone()), log_dir, test_start,
                                         Some(event_collector.clone())).await?;
                        shells.insert(name.clone(), Arc::new(tokio::sync::Mutex::new(vm)));
                    }
                    let vm_arc = shells.get(&name).expect("shell just inserted above");
                    let _ = vm_arc.lock().await.exec_stmts(block.body()).await?;
                }
                IrTestItem::Cleanup { block, .. } => {
                    cleanup_block = Some(block.clone());
                }
            }
        }
        Ok(())
    }.await;

    // 5. Terminate all test shells (reverse insertion order, deduplicated by Arc pointer)
    let mut seen = std::collections::HashSet::new();
    for (_, vm_arc) in shells.drain(..).rev() {
        let ptr = Arc::as_ptr(&vm_arc) as usize;
        if seen.insert(ptr) {
            vm_arc.lock().await.shutdown().await;
        }
    }

    // 6. Run test cleanup (fresh shell, best-effort per R002)
    if let Some(cleanup) = &cleanup_block {
        let ctx = ExecutionContext::new(scope.clone(), ShellState::new("cleanup"));
        if let Ok(mut cleanup_vm) = Vm::new(ctx, &manager.shell_command, &manager.shell_prompt,
                                             manager.fn_table.clone(), manager.pure_fn_table.clone(),
                                             Some(progress_tx.clone()), log_dir, test_start,
                                             Some(event_collector.clone())).await {
            if let Err(failure) = cleanup_vm.exec_stmts(cleanup.body()).await {
                warnings.push(Warning::CleanupFailed {
                    source: CleanupSource::Test,
                    failure,
                });
            }
            cleanup_vm.shutdown().await;
        }
    }

    body_result
}
```

For `Plan::Skipped` and `Plan::Invalid`: emit result with cause IDs, no execution.

#### Entry point

`cmd_run` in `bin/relux.rs` wires the resolver output to the runtime. It replaces the current `todo!("R004: runtime adaptation")` stub.

```rust
async fn cmd_run(matches: &clap::ArgMatches) {
    // 1. Resolve project and config
    let (project_root, config) = resolve_project(matches);
    let test_paths = resolve_test_paths(matches, &project_root);
    let loader = build_source_loader(&project_root);
    let env = Arc::new(Env::capture());

    // 2. Run resolver pipeline → Suite + tables
    let (suite, effect_table, fn_table, pure_fn_table) = resolve_with_tables(&*loader, test_paths, env);

    // Bail early if all plans are invalid
    let has_invalid = suite.plans.iter().any(|p| matches!(p, Plan::Invalid { .. }));
    if has_invalid && !suite.plans.iter().any(|p| matches!(p, Plan::Runnable { .. })) {
        process::exit(1);
    }

    // 3. Build RunContext from CLI args + config
    let multiplier: f64 = *matches.get_one("multiplier").expect("clap default guarantees presence");
    let strategy = match matches.get_one::<String>("strategy").map(|s| s.as_str()) {
        Some("fail-fast") => RunStrategy::FailFast,
        _ => RunStrategy::All,
    };
    let run_id = Uuid::new_v4().to_string();
    let run_dir = project_root.join("relux/out").join(&run_id);
    let artifacts_dir = run_dir.join("artifacts");
    let _ = std::fs::create_dir_all(&artifacts_dir);

    let mut default_timeout = IrTimeout::tolerance(config.timeout.match_timeout);
    default_timeout.apply_multiplier(multiplier);

    let test_timeout = config.timeout.test.map(|d| {
        let mut t = IrTimeout::tolerance(d);
        t.apply_multiplier(multiplier);
        t
    });

    let run_ctx = RunContext {
        run_id,
        run_dir: run_dir.clone(),
        artifacts_dir: artifacts_dir.clone(),
        project_root: project_root.clone(),
        shell_command: config.shell.command.clone(),
        shell_prompt: config.shell.prompt.clone(),
        default_timeout,
        test_timeout,
        suite_timeout: config.timeout.suite,
        strategy,
    };

    // 4. Execute
    let results = execute(&suite, &run_ctx, &effect_table, &fn_table, &pure_fn_table).await;

    // 5. Report
    let summary = RunSummary::from(&results);
    summary.print();

    if matches.get_flag("tap") {
        let tap = tap::render(&results);
        std::fs::write(artifacts_dir.join("results.tap"), tap).ok();
    }
    if matches.get_flag("junit") {
        let junit = junit::render(&results);
        std::fs::write(artifacts_dir.join("results.xml"), junit).ok();
    }

    // 6. Exit code
    if summary.has_failures() {
        process::exit(1);
    }
}
```

`resolve_with_tables` is a new variant of `resolve()` that returns the `LoweringContext`'s tables alongside the `Suite`:

```rust
pub fn resolve_with_tables(
    source_loader: &dyn SourceLoader,
    test_paths: Vec<ModulePath>,
    env: Arc<Env>,
) -> (Suite, EffectTable, FnTable, PureFnTable) {
    // ... same as resolve() up to build_all_plans ...
    let plans = build_all_plans(&mut ctx);
    ctx.print_diagnostics();
    let effect_table = ctx.effects().clone();
    let fn_table = ctx.functions().clone();
    let pure_fn_table = ctx.pure_functions().clone();
    let suite = ctx.into_suite(plans);
    (suite, effect_table, fn_table, pure_fn_table)
}
```

The orchestrator runs each plan sequentially. Tests run one at a time — no concurrent test execution. A fresh `EffectManager` is created per test.

#### RunContext

The orchestrator receives a `RunContext` with all runtime configuration. This replaces the current `Runtime` struct.

```rust
pub struct RunContext {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub project_root: PathBuf,
    pub shell_command: String,
    pub shell_prompt: String,
    pub default_timeout: IrTimeout,
    pub test_timeout: Option<IrTimeout>,
    pub suite_timeout: Option<Duration>,
    pub strategy: RunStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStrategy {
    All,
    FailFast,
}
```

#### Environment variables

The orchestrator builds the base `Env` by merging the process environment with relux-specific variables. These are set once and inherited by all VMs.

```rust
fn build_env(ctx: &RunContext) -> Arc<Env> {
    let mut env = std::env::vars().collect::<HashMap<_, _>>();
    env.insert("__RELUX_RUN_ID".into(),          ctx.run_id.clone());
    env.insert("__RELUX_TEST_ARTIFACTS".into(),   ctx.artifacts_dir.display().to_string());
    env.insert("__RELUX_SHELL_PROMPT".into(),    ctx.shell_prompt.clone());
    env.insert("__RELUX_SUITE_ROOT".into(),      ctx.project_root.display().to_string());
    env.insert("__RELUX_EXECUTABLE".into(),      std::env::current_exe()
                                                     .unwrap_or_default()
                                                     .display().to_string());
    Arc::new(env)
}
```

Per-test, `__RELUX_TEST_ROOT` is added to a cloned env pointing to the test file's parent directory:

```rust
fn test_env(base: &Arc<Env>, test_file: &Path) -> Arc<Env> {
    let mut env = base.as_ref().clone();
    if let Some(dir) = test_file.parent() {
        env.insert("__RELUX_TEST_ROOT".into(), dir.display().to_string());
    }
    Arc::new(env)
}
```

#### Run directory and shell logging

Each test gets its own log directory derived from the source file path and test name:

```rust
fn test_log_dir(run_dir: &Path, source_map: &SourceTable, meta: &TestMeta, project_root: &Path) -> PathBuf {
    let source_path = &source_map.get(&meta.span().file()).expect("test file must be in source map").path;
    let relative = source_path.strip_prefix(project_root).unwrap_or(source_path);
    run_dir.join("logs").join(relative.with_extension("")).join(slugify(meta.name()))
}
```

The log directory is created before test execution. Each `Vm::new()` call receives the log directory so it can create a `ShellLogger` — same as today. The `Vm::new` signature gains `log_dir: &Path` and `test_start: Instant` parameters alongside the existing `progress_tx` and `event_collector` parameters.

#### Progress reporting

Progress events are emitted at the same points as the current runtime. The orchestrator creates a `(ProgressTx, ProgressRx)` pair per test, spawns a printer task, and passes `ProgressTx` into `Vm::new()`. The VM emits progress events via the channel — this is unchanged from today.

```rust
async fn run_test(
    meta: &TestMeta,
    test: &IrTest,
    manager: &EffectManager,
    run_ctx: &RunContext,
    source_map: &SourceTable,
    base_env: &Arc<Env>,
) -> TestResult {
    let test_start = Instant::now();
    let log_dir = test_log_dir(&run_ctx.run_dir, source_map, meta, &run_ctx.project_root);
    let _ = std::fs::create_dir_all(&log_dir);
    let event_collector = EventCollector::new(test_start);

    let display_id = test_display_id(meta, source_map, &run_ctx.project_root);
    eprint!("test {display_id}: ");
    let _ = std::io::stderr().flush();

    let (progress_tx, progress_rx) = progress::channel();
    let printer_handle = progress::spawn_printer(progress_rx);

    let source_file = &source_map.get(&meta.span().file()).expect("test file must be in source map").path;
    let test_env = test_env(base_env, source_file);
    let mut warnings = Vec::new();

    let outcome = run_test_body(meta, test, manager, &mut warnings,
                                &test_env, &log_dir, test_start,
                                &event_collector, progress_tx.clone()).await;

    // Release effects — parallel, refcount-based recursive cleanup
    let effect_warnings = manager.cleanup(test.needs()).await;
    warnings.extend(effect_warnings);

    drop(progress_tx);
    let progress_string = printer_handle.await.unwrap_or_default();
    let duration = test_start.elapsed();

    // Write event log
    event_collector.write_json(&log_dir).await;

    match outcome {
        Ok(()) => TestResult::passed(meta, warnings, duration, progress_string, Some(log_dir)),
        Err(failure) => TestResult::failed(meta, failure, warnings, duration, progress_string, Some(log_dir)),
    }
}
```

#### Suite execution

```rust
pub async fn execute(
    suite: &Suite,
    run_ctx: &RunContext,
    effect_table: &EffectTable,
    fn_table: &FnTable,
    pure_fn_table: &PureFnTable,
) -> Vec<TestResult> {
    let base_env = build_env(run_ctx);

    eprintln!("\nrunning {} tests", suite.plans.len());

    let run_fut = run_all(suite, run_ctx, effect_table, fn_table, pure_fn_table, &base_env);

    // Suite-level timeout — hard deadline for the entire run
    match run_ctx.suite_timeout {
        Some(timeout) => match tokio::time::timeout(timeout, run_fut).await {
            Ok(results) => results,
            Err(_) => {
                eprintln!("suite timeout ({timeout:?}) exceeded");
                Vec::new()
            }
        },
        None => run_fut.await,
    }
}

async fn run_all(
    suite: &Suite,
    run_ctx: &RunContext,
    effect_table: &EffectTable,
    fn_table: &FnTable,
    pure_fn_table: &PureFnTable,
    base_env: &Arc<Env>,
) -> Vec<TestResult> {
    let mut results = Vec::with_capacity(suite.plans.len());

    for plan in &suite.plans {
        let result = match plan {
            Plan::Runnable { meta, test, .. } => {
                // Per-test timeout — inline test timeout takes precedence
                // over the inherited config/manifest timeout.
                let effective_timeout = meta.timeout()
                    .map(|t| t.value())
                    .or(run_ctx.test_timeout.as_ref().map(|t| t.value()));

                // Fresh EffectManager per test — no cross-test effect sharing
                let manager = EffectManager::new(
                    base_env.clone(),
                    run_ctx.shell_command.clone(),
                    run_ctx.shell_prompt.clone(),
                    effect_table.clone(),
                    fn_table.clone(),
                    pure_fn_table.clone(),
                );

                let test_fut = run_test(meta, test, &manager, run_ctx,
                                        &suite.source_map, base_env);

                match effective_timeout {
                    Some(timeout) => match tokio::time::timeout(timeout, test_fut).await {
                        Ok(result) => result,
                        Err(_) => TestResult::timeout(meta, timeout),
                    },
                    None => test_fut.await,
                }
            }
            Plan::Skipped { meta, causes, .. } => {
                TestResult::skipped(meta, causes)
            }
            Plan::Invalid { meta, causes, .. } => {
                TestResult::invalid(meta, causes)
            }
        };

        let failed = result.is_failure();
        results.push(result);

        // Fail-fast — stop after first failure
        if failed && run_ctx.strategy == RunStrategy::FailFast {
            break;
        }
    }

    results
}
```

A fresh `EffectManager` is created for each test. Effect instances are deduplicated within a single test's dependency tree via the registry — if two effects within the same test both need the same child effect (same identity + overlay), the child is bootstrapped once and shared. Across tests, no sharing occurs: each test gets its own effect instances and cleans them all up before the next test begins.

Test-level and effect-level `let` statements are evaluated using `crate::evaluator` — the same pure expression evaluator used by the resolver for marker evaluation. These are not shell operations; they produce values without a PTY.

#### Condition markers

Condition markers (`skip`, `run`, `flaky`) are evaluated entirely at resolve time. The resolver reads the process environment and evaluates marker conditions during plan building — by the time the runtime receives a `Suite`, every test is already categorized as `Runnable`, `Skipped`, or `Invalid`. The runtime does not evaluate conditions. The same applies to effect-level markers — the resolver skips or invalidates tests that depend on skipped/invalid effects.

The existing `evaluate_conditions()` function in `runtime/mod.rs` and its 14 unit tests are deleted. Condition evaluation now lives in `dsl::resolver::marker::eval_marker`.

#### Warnings

Cleanup failures are best-effort (R002): they produce warnings but never change the test outcome. Warnings are collected during execution and attached to the `TestResult`.

```rust
enum Warning {
    CleanupFailed {
        source: CleanupSource,
        failure: Failure,
    },
}

enum CleanupSource {
    Test,
    Effect { name: String },
}
```

`TestResult` gains a `warnings: Vec<Warning>` field. The reporter renders warnings after the test outcome — e.g. "test passed (1 warning: effect Db cleanup failed: match timeout)".

Constructor helpers are added to `TestResult`. The existing `Outcome` enum (`Pass`, `Fail(Failure)`, `Skipped(String)`) is retained:

```rust
impl TestResult {
    fn passed(meta: &TestMeta, warnings: Vec<Warning>, duration: Duration,
              progress: String, log_dir: Option<PathBuf>) -> Self { /* ... */ }
    fn failed(meta: &TestMeta, failure: Failure, warnings: Vec<Warning>, duration: Duration,
              progress: String, log_dir: Option<PathBuf>) -> Self { /* ... */ }
    fn timeout(meta: &TestMeta, deadline: Duration) -> Self { /* ... */ }
    fn skipped(meta: &TestMeta, causes: &[CauseId]) -> Self { /* ... */ }
    fn invalid(meta: &TestMeta, causes: &[CauseId]) -> Self { /* ... */ }
}
```

Each constructor extracts `test_name` from `meta.name()` and `test_path` from the source map. `timeout` produces a `Fail(Failure::Runtime { message: "test timeout ... exceeded", .. })`. `skipped` and `invalid` produce `Skipped(reason)` outcomes where the reason is derived from the cause IDs.

### VM

The VM owns a PTY and an `ExecutionContext`. Its public interface is `exec_stmts` and `shutdown`. All statement dispatch, expression evaluation, and function calls are `impl Vm` methods. The VM never accesses config, test metadata, or environment directly — everything flows through the `ExecutionContext`.

#### Struct

```rust
pub struct Vm {
    // PTY
    writer: pty_process::OwnedWritePty,
    child: Child,
    output_buf: OutputBuffer,
    read_task: tokio::task::JoinHandle<()>,

    // Execution state
    ctx: ExecutionContext,

    // Function tables (read-only, shared across VMs)
    fn_table: FnTable,           // SharedTable — Arc internally
    pure_fn_table: PureFnTable,  // SharedTable — Arc internally

    // Shell init
    shell_prompt: String,

    // Observability
    progress_tx: Option<ProgressTx>,
    shell_log: Arc<Mutex<ShellLogger>>,
    event_collector: Option<EventCollector>,
}
```

Changes from current VM:
- `ScopeStack` → `ExecutionContext`
- `Arc<CodeServer>` → `FnTable` + `PureFnTable` (direct table lookup via `FnId`)
- `shell_name: String` → lives in `ctx.shell.name`

#### Statement dispatch

```rust
impl Vm {
    pub async fn exec_stmts(&mut self, stmts: &[IrShellStmt]) -> Result<String, Failure> {
        let mut last = String::new();
        for stmt in stmts {
            last = self.exec_stmt(stmt).await?;
        }
        self.drain_recv_event().await;
        Ok(last)
    }

    async fn exec_stmt(&mut self, stmt: &IrShellStmt) -> Result<String, Failure> {
        self.drain_recv_event().await;
        self.check_fail(stmt.span()).await?;

        match stmt {
            IrShellStmt::Comment { .. } => Ok(String::new()),

            IrShellStmt::Send { payload, span } => {
                let text = self.ctx.interpolate(payload).await;
                self.send_line(&text, span).await?;
                Ok(text)
            }
            IrShellStmt::SendRaw { payload, span } => {
                let text = self.ctx.interpolate(payload).await;
                self.send_raw(text.as_bytes(), span).await?;
                Ok(text)
            }

            IrShellStmt::MatchRegex { pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                let timeout = self.ctx.timeout().value();
                self.do_match_regex(&pat, timeout, span).await
            }
            IrShellStmt::MatchLiteral { pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                let timeout = self.ctx.timeout().value();
                self.do_match_literal(&pat, timeout, span).await
            }
            IrShellStmt::TimedMatchRegex { timeout, pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                self.do_match_regex(&pat, timeout.duration(), span).await
            }
            IrShellStmt::TimedMatchLiteral { timeout, pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                self.do_match_literal(&pat, timeout.duration(), span).await
            }

            IrShellStmt::Timeout { timeout, .. } => {
                self.ctx.set_timeout(timeout.clone());
                Ok(String::new())
            }
            IrShellStmt::FailRegex { pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                let re = build_regex(&pat, span)?;
                self.ctx.set_fail_pattern(Some(FailPattern::Regex(re)));
                self.check_fail(span.clone()).await?;
                Ok(String::new())
            }
            IrShellStmt::FailLiteral { pattern, span } => {
                let pat = self.ctx.interpolate(pattern).await;
                self.ctx.set_fail_pattern(Some(FailPattern::Literal(pat)));
                self.check_fail(span.clone()).await?;
                Ok(String::new())
            }
            IrShellStmt::ClearFailPattern { .. } => {
                self.ctx.set_fail_pattern(None);
                Ok(String::new())
            }
            IrShellStmt::BufferReset { .. } => {
                self.output_buf.clear().await;
                Ok(String::new())
            }

            IrShellStmt::Let { stmt, .. } => {
                let value = match stmt.value() {
                    Some(expr) => self.eval_expr(expr).await?,
                    None => String::new(),
                };
                self.ctx.let_insert(stmt.name().name(), value.clone());
                Ok(value)
            }
            IrShellStmt::Assign { stmt, span } => {
                let value = self.eval_expr(stmt.value()).await?;
                self.ctx.assign(stmt.name().name(), value.clone()).await?;
                Ok(value)
            }
            IrShellStmt::Expr { expr, .. } => {
                self.eval_expr(expr).await
            }
        }
    }
}
```

#### Expression evaluation

```rust
impl Vm {
    async fn eval_expr(&mut self, expr: &IrExpr) -> Result<String, Failure> {
        match expr {
            IrExpr::String { value, .. } => Ok(self.ctx.interpolate(value).await),
            IrExpr::Var { name, .. } => Ok(self.ctx.lookup(name).await.unwrap_or_default()),
            IrExpr::CaptureRef { index, .. } => Ok(self.ctx.capture(*index).unwrap_or_default()),
            IrExpr::Call { call, .. } => self.eval_call(call).await,
        }
    }
}
```

#### Function call dispatch

`IrCallExpr.resolved` is an `IrFnId` (aliased from `diagnostics::FnId`) that keys directly into the function tables. `FnTable` is `SharedTable<IrFnId, Result<IrFn, LoweringBail>>`. The table stores `Result` values because lowering can fail, but by the time a `Runnable` plan reaches the runtime, all referenced functions have succeeded lowering. The `expect()` on the `Result` is safe — the resolver guarantees it.

No `CodeServer` lookup needed — the resolver already validated and resolved every call.

```rust
impl Vm {
    async fn eval_call(&mut self, call: &IrCallExpr) -> Result<String, Failure> {
        // 1. Evaluate args
        let mut args = Vec::with_capacity(call.args().len());
        for arg in call.args() {
            args.push(self.eval_expr(arg).await?);
        }

        let fn_id = call.resolved();
        let call_name = call.name().name().to_string();

        // 2. Dispatch — fn_table first, then pure_fn_table
        //    .get() returns Option<Result<IrFn, LoweringBail>>; only Runnable plans
        //    reach the runtime, so all referenced functions have lowered successfully.
        if let Some(ir_fn) = self.fn_table.get(fn_id).map(|r| r.expect("resolver guarantees successful lowering")) {
            match ir_fn {
                IrFn::UserDefined { params, body, .. } => {
                    let named_args: Vec<(String, String)> = params.iter()
                        .zip(args.iter())
                        .map(|(p, v)| (p.name().to_string(), v.clone()))
                        .collect();
                    self.ctx.push_call(&call_name, &named_args);
                    let result = self.exec_stmts(&body).await;
                    self.ctx.pop_call();
                    result
                }
                IrFn::Builtin { name, arity } => {
                    let bif = bifs::lookup_impure(name, *arity)
                        .expect("resolved BIF must exist");
                    bif.call(self, args, call.span()).await
                }
            }
        } else if let Some(ir_pure_fn) = self.pure_fn_table.get(fn_id).map(|r| r.expect("resolver guarantees successful lowering")) {
            match ir_pure_fn {
                IrPureFn::UserDefined { params, body, .. } => {
                    // delegate to crate::evaluator
                    let named_args: Vec<(String, String)> = params.iter()
                        .zip(args.iter())
                        .map(|(p, v)| (p.name().to_string(), v.clone()))
                        .collect();
                    crate::evaluator::eval_pure_fn(body, &named_args, &self.ctx.env, &self.pure_fn_table)
                }
                IrPureFn::Builtin { name, arity } => {
                    let bif = bifs::lookup_pure(name, *arity)
                        .expect("resolved pure BIF must exist");
                    bif.call(self, args, call.span()).await
                }
            }
        } else {
            unreachable!("resolver guarantees FnId is valid")
        }
    }
}
```

#### Methods on ExecutionContext

`interpolate()` moves from being a free function to an `impl ExecutionContext` method. It walks `IrInterpolation` parts, looks up variables and captures via the context's own lookup chain, and concatenates.

```rust
impl ExecutionContext {
    async fn interpolate(&self, interp: &IrInterpolation) -> String { /* ... */ }
    async fn lookup(&self, name: &str) -> Option<String> { /* ... */ }
    fn capture(&self, index: usize) -> Option<String> { /* ... */ }
    fn let_insert(&mut self, name: &str, value: String) { /* ... */ }
    async fn assign(&mut self, name: &str, value: String) -> Result<(), Failure> { /* ... */ }
    fn push_call(&mut self, name: &str, args: &[(String, String)]) { /* ... */ }
    fn pop_call(&mut self) { /* ... */ }
    /// Returns the effective timeout: top call frame → shell state → scope → default_timeout.
    fn timeout(&self) -> &IrTimeout { /* ... */ }
    fn set_timeout(&mut self, t: IrTimeout) { /* ... */ }
    fn fail_pattern(&self) -> Option<&FailPattern> { /* ... */ }
    fn set_fail_pattern(&mut self, p: Option<FailPattern>) { /* ... */ }
    fn current_name(&self) -> &str { /* ... */ }
}
```

`timeout`, `fail_pattern`, `set_timeout`, `set_fail_pattern`, `let_insert`, `assign`, `lookup`, and `capture` all operate on the current active context: the top `CallFrame` if the call stack is non-empty, otherwise `ShellState`. The lookup chain follows the rules defined in the Execution Context section.

#### Unchanged

These parts of the current VM stay as-is:

- `OutputBuffer` — same struct, same API
- `wait_consume_literal` / `wait_consume_regex` — same wait-loop-with-fail-check logic
- `check_fail` / `make_fail_pattern_error` — same
- `send_bytes` — same
- `shutdown` — same
- `init_prompt` — same (reads timeout from `ctx.timeout()`)
- `Vm::new(ctx, shell_command, shell_prompt, fn_table, pure_fn_table, progress_tx, log_dir, test_start, event_collector)` — same PTY spawn logic, takes `ExecutionContext` instead of `ScopeStack`. Shell command and prompt come from caller (the `EffectManager` or orchestrator), not from config directly. Observability parameters (`progress_tx`, `log_dir`, `test_start`, `event_collector`) are passed through from the orchestrator for test VMs; effect bootstrap and cleanup VMs pass `None`/defaults for these since they don't participate in test-level reporting
- BIF traits (`Bif`, `PureBif`, `PureContext`, `VmContext`) — same
- All BIF implementations — same
- `build_regex()` — stays as a free utility function
- `EventCollector`, `LogEvent`, `LogEventKind` — same struct, same variants, same API
- `ShellLogger`, `ProgressTx` — same
- Event emission points — same locations in `exec_stmt`, `eval_expr`, and `eval_call` (every statement and expression emits the same `LogEventKind` variants as today, just operating on `IrShellStmt`/`IrExpr`/`IrCallExpr` instead of `ShellStmt`/`Expr`/`FnCall`)

#### Removed

- `CodeServer` struct and `Callable` enum — replaced by direct `FnTable`/`PureFnTable` lookup via `FnId`
- `ScopeStack` — replaced by `ExecutionContext`
- All legacy IR types in `legacy.rs` — the VM now consumes `IrShellStmt`, `IrExpr`, `IrCallExpr` directly
- `runtime::pure::exec_pure_body` — replaced by `crate::evaluator`
- `runtime::vars` — `VarScope`/`Env`/`Captures` stay in `crate::stack`; scoping logic absorbed into `ExecutionContext`
- `Runtime` struct — replaced by `RunContext` (plain data) + `execute()` (standalone function)
- `TestRunContext` — replaced by passing observability parameters directly to `Vm::new` and `run_test_body`
- Legacy `Timeout` enum in `legacy.rs` — replaced by `IrTimeout` with multiplier support
- `evaluate_conditions()` and its 14 tests — condition evaluation now happens at resolve time

### Module Structure

The current flat file layout under `runtime/` is reorganized into submodules grouped by responsibility.

```
runtime/
├── mod.rs              — re-exports, execute(), run_all(), run_test(), run_test_body()
│
├── vm/
│   ├── mod.rs          — Vm struct, exec_stmts, exec_stmt, eval_expr, eval_call, shutdown
│   ├── bifs.rs         — Bif/PureBif traits, all BIF implementations
│   └── context.rs      — ExecutionContext, Scope, ShellState, CallFrame (absorbs vars.rs)
│
├── effect/
│   ├── mod.rs          — EffectManager, bootstrap_effect, instantiate, cleanup
│   └── registry.rs     — EffectRegistry, EffectSlot, EffectHandle, EffectInstanceKey
│
├── report/
│   ├── mod.rs          — TestResult, Warning, CleanupSource, outcome enums, RunSummary
│   ├── html.rs         — HTML report generation
│   ├── junit.rs        — JUnit XML output
│   └── tap.rs          — TAP output
│
├── observe/
│   ├── mod.rs          — re-exports
│   ├── event_log.rs    — EventCollector, LogEvent, LogEventKind
│   ├── shell_log.rs    — ShellLogger
│   └── progress.rs     — ProgressTx
│
└── history.rs          — history subcommand (standalone)
```

#### What moved where

| Current file | Destination | Notes |
|---|---|---|
| `mod.rs` | `mod.rs` | Orchestration stays; VM code moves to `vm/` |
| `vm.rs` | `vm/mod.rs` | Core VM struct and dispatch |
| `bifs.rs` | `vm/bifs.rs` | Tightly coupled to VM |
| `vars.rs` | `vm/context.rs` | `ScopeStack` → `ExecutionContext`; `VarScope`/`Env`/`Captures` stay in `crate::stack` |
| `pure.rs` | *(deleted)* | Replaced by `crate::evaluator` |
| `result.rs` | `report/mod.rs` | Merged with `run_summary.rs` |
| `run_summary.rs` | `report/mod.rs` | Merged with `result.rs` |
| `html.rs` | `report/html.rs` | — |
| `junit.rs` | `report/junit.rs` | — |
| `tap.rs` | `report/tap.rs` | — |
| `event_log.rs` | `observe/event_log.rs` | — |
| `shell_log.rs` | `observe/shell_log.rs` | — |
| `progress.rs` | `observe/progress.rs` | — |
| `history.rs` | `history.rs` | Stays flat — standalone subcommand |
| *(new)* | `effect/mod.rs` | `EffectManager`, `bootstrap_effect` |
| *(new)* | `effect/registry.rs` | `EffectRegistry`, slot locking, acquire/cleanup |

`crate::evaluator` stays at crate root — it is shared infrastructure used by both the resolver (marker evaluation) and the runtime (test/effect-level lets, overlay evaluation). Moving it into `runtime/` would create a `resolver → runtime` dependency, inverting the pipeline.

### Test Coverage

Existing test counts per new module, with gaps to fill.

#### `vm/mod.rs` — 42 tests (from `vm.rs`), adequate

Covers `OutputBuffer` (append, consume_literal, consume_regex, clear, snapshot_tail), `truncate_before/after`, `regex_error_summary`, `check_fail_in_buffer`. Statement dispatch and expression evaluation are covered by e2e tests — no new unit tests needed.

#### `vm/bifs.rs` — 35 tests (from `bifs.rs`), adequate

Every BIF tested via `DummyVm` mock. `lookup_pure`/`lookup_impure` dispatch covered. No gaps.

#### `vm/context.rs` — 6 tests (from `vars.rs`), needs expansion

Existing tests cover timeout inherit/restore across push/pop and basic `let_insert`/`lookup`. New tests needed:

- **Lookup chain without call stack**: shell vars → shell env_overlay → scope vars → scope env_overlay → env fallback. Verify each layer is reached when higher layers miss.
- **Lookup chain with call stack**: call frame vars → env only. Verify scope vars, shell vars, overlay, and captures are all invisible.
- **`capture()`**: store captures in shell state, verify retrieval by index. Verify captures are invisible inside call frames.
- **`current_name()`**: returns call frame name when call stack non-empty, shell alias when present, shell name as fallback.
- **`push_call` / `pop_call`**: verify args are pre-filled into call frame vars. Verify timeout and fail_pattern are copied from current context. Verify pop discards the frame's timeout/fail_pattern changes.
- **`set_fail_pattern` / `fail_pattern`**: verify targeting (call frame vs shell state). Verify `!clear` clears only the current context.
- **`assign`**: verify it updates scope vars (not shell vars) when variable exists in scope. Verify error when variable doesn't exist anywhere.
- **`interpolate`**: verify `${var}` resolution through the lookup chain. Verify `${1}` capture references. Verify literal parts pass through unchanged.
- **Scope sharing via `Arc<Mutex<VarScope>>`**: two `ExecutionContext` instances with the same scope — `let_insert` in one is visible via `lookup` in the other.

#### `effect/registry.rs` — 0 tests, needs unit tests

All testable without PTY — use a mock VM factory.

- **Acquire empty slot**: verify slot transitions to `Ready` with refcount 1.
- **Acquire ready slot**: verify refcount increments, same `Arc` returned.
- **Acquire failed slot**: verify `Failure` is returned, no bootstrap attempted.
- **Bootstrap failure caches**: verify slot transitions to `Failed`, subsequent acquirers get cached failure.
- **Refcount decrement**: verify `run_cleanup` decrements. Verify slot stays `Ready` while refcount > 0.
- **Last release triggers cleanup**: verify `run_cleanup` runs cleanup block and transitions to `Empty` when refcount hits 0.
- **Recursive cleanup**: verify child dependencies are released when parent reaches refcount 0.
- **Cleanup warnings propagate**: verify warnings from child cleanup are collected and returned alongside parent warnings.
- **Concurrent acquire**: two tasks acquire same key — first bootstraps, second blocks then sees `Ready`.

#### `effect/mod.rs` — 0 tests, e2e only

`bootstrap_effect` and `instantiate` spawn PTY processes and walk IR bodies — not practical to unit-test. Covered by e2e tests.

#### `report/mod.rs` — 10 tests (from `result.rs` + `run_summary.rs`), needs expansion

- **`Warning` display**: verify `CleanupSource::Test` and `CleanupSource::Effect { name }` render correctly.
- **`TestResult` with warnings**: verify warnings are attached and rendered alongside outcome.

#### `report/html.rs`, `report/junit.rs`, `report/tap.rs` — 0, 6, 9 tests

JUnit and TAP have adequate coverage. HTML is template output — no new unit tests planned.

#### `observe/*` — 0 tests

Thin wrappers (`EventCollector`, `ShellLogger`, `ProgressTx`). Low risk, no new unit tests planned.

#### `history.rs` — 15 tests, adequate

Standalone subcommand. No changes in R005.

#### `mod.rs` (orchestration) — 14 tests, deleted

The existing 14 tests cover `evaluate_conditions` which uses `CodeServer` and legacy IR types. Condition evaluation now happens at resolve time in `dsl::resolver::marker::eval_marker` (already tested there). These tests are deleted along with the `evaluate_conditions` function. The new `run_test` / `run_test_body` orchestration is covered by e2e tests.
