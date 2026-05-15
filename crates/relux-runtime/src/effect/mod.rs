pub mod registry;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use futures::future::join_all;

use crate::RuntimeContext;
use crate::effect::registry::AcquiredEffect;
use crate::effect::registry::EffectGuard;
use crate::effect::registry::EffectHandle;
use crate::effect::registry::EffectInstanceKey;
use crate::effect::registry::EffectRegistry;
use crate::effect::registry::EffectSlot;
use crate::effect::registry::ExportedEffect;
use crate::effect::registry::ReleaseOutcome;
use crate::effect::registry::ShellInstanceKey;
use crate::effect::registry::ShellMap;
use crate::effect::registry::VarMap;
use crate::observe::structured::SpanId;
use crate::observe::structured::SpanKind;
use crate::report::result::Failure;
use crate::report::result::FailureContext;
use crate::vm::Vm;
use crate::vm::context::ExecutionContext;
use crate::vm::context::Scope;
use crate::vm::context::ShellState;
use relux_core::pure::Env;
use relux_core::pure::LayeredEnv;
use relux_core::pure::VarScope;
use relux_ir::IrCleanupBlock;
use relux_ir::IrEffectItem;
use relux_ir::IrEffectStart;
use relux_ir::IrNode;
use relux_ir::IrPureLetStmt;

// ─── Warning / CleanupSource ────────────────────────────────

#[derive(Debug, Clone)]
pub enum CleanupSource {
    Test,
    Effect { name: String },
}

#[derive(Debug, Clone)]
pub enum Warning {
    CleanupFailed {
        source: CleanupSource,
        failure: Failure,
    },
}

// ─── EffectManager ──────────────────────────────────────────

pub struct EffectManager {
    registry: Arc<EffectRegistry>,
    pub(crate) rt_ctx: RuntimeContext,
    /// Guards for the test's direct effect acquires (`start E as a`
    /// at the top of a test). Drained by `cleanup_all`. Per-test by
    /// construction: each test instantiates its own `EffectManager`.
    top_level_guards: TokioMutex<Vec<EffectGuard>>,
}

impl EffectManager {
    pub fn new(registry: Arc<EffectRegistry>, rt_ctx: RuntimeContext) -> Self {
        Self {
            registry,
            rt_ctx,
            top_level_guards: TokioMutex::new(Vec::new()),
        }
    }

    /// Acquire all starts. Each start recursively acquires its own
    /// dependencies before bootstrapping itself.
    /// `caller_vars` contains the caller's accumulated variable scope,
    /// allowing overlay expressions to reference the caller's `let` bindings.
    /// `caller_env` is the layered environment visible to the caller.
    /// Returns (key, exported-shells-map) per start declaration.
    #[allow(clippy::type_complexity)]
    pub fn instantiate<'a>(
        &'a self,
        starts: &'a [IrEffectStart],
        caller_vars: &'a VarScope,
        caller_env: &'a Arc<LayeredEnv>,
        parent_span: SpanId,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<(ExportedEffect, EffectGuard)>, Failure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let mut results: Vec<(ExportedEffect, EffectGuard)> = Vec::with_capacity(starts.len());
            for start in starts {
                let evaluated = self
                    .eval_overlay(start, caller_vars, caller_env, parent_span)
                    .await?;

                let expect_names: Vec<&str> = self
                    .rt_ctx
                    .tables
                    .effects
                    .get(start.effect())
                    .and_then(|r| r.as_ref().ok())
                    .map(|eff| eff.expects().iter().map(|e| e.name()).collect())
                    .unwrap_or_default();
                let key = EffectInstanceKey::from_expects(
                    start.effect().clone(),
                    &expect_names,
                    &evaluated,
                );

                match self
                    .acquire(&key, start, caller_vars, caller_env, evaluated, parent_span)
                    .await
                {
                    Ok((acquired, guard)) => {
                        results.push((
                            ExportedEffect {
                                key,
                                shells: acquired.shells,
                                vars: acquired.vars,
                            },
                            guard,
                        ));
                    }
                    Err(failure) => {
                        // Release everything we already acquired in this batch
                        // before propagating the error. Sequential for simplicity.
                        for (_exported, guard) in results.drain(..) {
                            self.release_and_teardown(guard, parent_span).await;
                        }
                        return Err(failure);
                    }
                }
            }
            Ok(results)
        })
    }

    /// Public top-level entry point used by the test runner. Acquires every
    /// `start` in `starts`, stashes the resulting guards on the
    /// `EffectManager` so `cleanup_all` can drain them, and returns the
    /// shells/vars exports for the caller's shell map.
    pub async fn instantiate_top_level(
        &self,
        starts: &[IrEffectStart],
        caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        test_span: SpanId,
    ) -> Result<Vec<ExportedEffect>, Failure> {
        let pairs = self
            .instantiate(starts, caller_vars, caller_env, test_span)
            .await?;
        let mut top = self.top_level_guards.lock().await;
        let mut exported = Vec::with_capacity(pairs.len());
        for (ex, guard) in pairs {
            top.push(guard);
            exported.push(ex);
        }
        Ok(exported)
    }

    /// Drain the test's top-level guards and release each concurrently.
    /// The slot mutex + refcount guarantee that for each dedup'd slot,
    /// exactly one releaser sees `refcount == 0` and runs the cleanup
    /// body; other releasers return `None` and short-circuit.
    ///
    /// Every `EffectCleanup` span opened here is parented under
    /// `test_span`. Cleanups are operationally test-level activity
    /// (scheduled at test teardown), and the `EffectSetup` span has
    /// long since closed.
    pub async fn cleanup_all(&self, test_span: SpanId) -> Vec<Warning> {
        let guards: Vec<EffectGuard> = std::mem::take(&mut *self.top_level_guards.lock().await);
        let futures = guards
            .into_iter()
            .map(|g| self.release_and_teardown(g, test_span));
        join_all(futures).await.into_iter().flatten().collect()
    }

    async fn acquire(
        &self,
        key: &EffectInstanceKey,
        start: &IrEffectStart,
        caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        evaluated_overlay: Env,
        parent_span: SpanId,
    ) -> Result<(AcquiredEffect, EffectGuard), Failure> {
        let slot = self.registry.slot(key);
        // The slot lock is held only across state inspection and transitions
        // (`Empty -> Loading`, `Loading -> Ready/Failed`). `bootstrap_effect`
        // runs WITHOUT the slot lock, so concurrent acquirers that hit
        // `Loading` can wait without blocking the bootstrap task. Per-test
        // serial use means this lock-free window is dead code today, but
        // removing it would re-introduce a deadlock surface if instantiation
        // ever runs concurrently.
        let mut evaluated_overlay = Some(evaluated_overlay);
        loop {
            let mut guard = slot.lock().await;
            match &mut *guard {
                EffectSlot::Ready { refcount, handle } => {
                    *refcount += 1;
                    let acquired = AcquiredEffect {
                        shells: handle.exposed_shells(),
                        vars: handle.exposed_vars.clone(),
                    };
                    let marker = handle.marker.clone();
                    drop(guard);

                    // Emit a zero-duration reuse span under the caller's
                    // parent so the dedup hit is visible in the viewer.
                    // The marker matches the bootstrap span's marker —
                    // the viewer hops back by marker on pill click.
                    let overlay = evaluated_overlay
                        .take()
                        .expect("Ready slot reachable only once per acquire");
                    let reuse_span = self.rt_ctx.log.open_span(
                        SpanKind::EffectSetup {
                            effect: start.effect().name.to_string(),
                            overlay: Self::evaluated_overlay_pairs(&overlay),
                            alias: start.alias().map(String::from),
                            marker,
                            is_reuse: true,
                        },
                        Some(parent_span),
                        Some(start.span()),
                    );
                    reuse_span.close();
                    return Ok((acquired, EffectGuard::new(slot.clone())));
                }
                EffectSlot::Failed(failure) => return Err(failure.clone()),
                EffectSlot::Loading(notify) => {
                    let notify = notify.clone();
                    drop(guard);
                    notify.notified().await;
                    // Slot is now Ready, Failed, or (rarely, on bootstrap
                    // panic in another task) still Loading. Loop and re-check.
                    continue;
                }
                EffectSlot::Empty => {
                    let notify = Arc::new(tokio::sync::Notify::new());
                    *guard = EffectSlot::Loading(notify.clone());
                    drop(guard);

                    // `Some` on the first iteration; the loop only continues
                    // through `Loading`, which doesn't consume the overlay.
                    let overlay = evaluated_overlay
                        .take()
                        .expect("Empty slot reachable only once per acquire");
                    let bootstrap_result = self
                        .bootstrap_effect(key, start, caller_vars, caller_env, overlay, parent_span)
                        .await;

                    let mut guard = slot.lock().await;
                    match bootstrap_result {
                        Ok(handle) => {
                            let acquired = AcquiredEffect {
                                shells: handle.exposed_shells(),
                                vars: handle.exposed_vars.clone(),
                            };
                            *guard = EffectSlot::Ready {
                                refcount: 1,
                                handle: Box::new(handle),
                            };
                            drop(guard);
                            notify.notify_waiters();
                            return Ok((acquired, EffectGuard::new(slot.clone())));
                        }
                        Err(failure) => {
                            self.rt_ctx
                                .log
                                .emit_error(parent_span, "", "", &failure.summary());
                            *guard = EffectSlot::Failed(failure.clone());
                            drop(guard);
                            notify.notify_waiters();
                            return Err(failure);
                        }
                    }
                }
            }
        }
    }

    async fn bootstrap_effect(
        &self,
        key: &EffectInstanceKey,
        start: &IrEffectStart,
        _caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        evaluated_overlay: Env,
        parent_span: SpanId,
    ) -> Result<EffectHandle, Failure> {
        let marker = key.marker();
        let overlay_pairs = Self::evaluated_overlay_pairs(&evaluated_overlay);
        let setup_span = self.rt_ctx.log.open_span(
            SpanKind::EffectSetup {
                effect: start.effect().name.to_string(),
                overlay: overlay_pairs,
                alias: start.alias().map(String::from),
                marker: marker.clone(),
                is_reuse: false,
            },
            Some(parent_span),
            Some(start.span()),
        );

        let effect_result = self
            .rt_ctx
            .tables
            .effects
            .get(start.effect())
            .ok_or_else(|| Failure::Runtime {
                message: format!("effect {:?} not found in table", start.effect()),
                span: None,
                shell: None,
                context: FailureContext::default(),
            })?;
        let effect = effect_result.as_ref().map_err(|e| Failure::Runtime {
            message: format!("effect resolution failed: {e:?}"),
            span: None,
            shell: None,
            context: FailureContext::default(),
        })?;
        let setup_span_id = setup_span.id();

        // 1. Create layered env from pre-evaluated overlay (inherits caller's env)
        let effect_env = Arc::new(LayeredEnv::child(caller_env.clone(), evaluated_overlay));

        // 2. Create effect scope
        let scope = Scope::Effect {
            name: effect.name().name().to_string(),
            vars: Arc::new(TokioMutex::new(VarScope::new())),
            _timeout: None,
            env: effect_env.clone(),
        };

        // 3. Evaluate effect-level lets into scope (parser enforces lets before starts)
        for item in effect.body() {
            if let IrEffectItem::Let { stmt, .. } = item {
                self.eval_effect_let(stmt, &scope, &effect_env, setup_span_id)
                    .await;
            }
        }

        // 4. Recursively instantiate sub-dependencies. Each pair = (export, guard).
        //    The `?` below is safe without guard release: `dep_guards` hasn't
        //    been populated yet, and `instantiate`'s own partial-batch
        //    rollback handles anything it acquired before failing.
        let effect_vars = scope.vars().lock().await.clone();
        let exported_deps = self
            .instantiate(effect.starts(), &effect_vars, &effect_env, setup_span_id)
            .await?;

        // From here on, `dep_guards` accumulates the guards for the
        // successfully-instantiated deps. Every fallible step between this
        // point and the final `Ok(EffectHandle { ... dep_guards ... })` is
        // wrapped in `try_guards!`, which releases the accumulated guards
        // via `release_and_teardown` before propagating the error.

        let mut dep_shells: HashMap<String, ShellMap> = HashMap::new();
        let mut dep_vars: HashMap<String, VarMap> = HashMap::new();
        let mut dep_guards: Vec<EffectGuard> = Vec::with_capacity(exported_deps.len());
        let mut alias_to_effect_name: HashMap<String, String> = HashMap::new();
        for (sub_start, (exported, guard)) in effect.starts().iter().zip(exported_deps) {
            dep_guards.push(guard);
            if let Some(alias) = sub_start.alias() {
                dep_shells.insert(alias.to_string(), exported.shells);
                dep_vars.insert(alias.to_string(), exported.vars);
                alias_to_effect_name.insert(alias.to_string(), sub_start.effect().name.0.clone());
            }
        }

        // Local helper: every fallible step below releases any dep_guards
        // collected so far before propagating the failure.
        macro_rules! try_guards {
            ($e:expr) => {{
                match $e {
                    Ok(v) => v,
                    Err(failure) => {
                        for g in std::mem::take(&mut dep_guards) {
                            self.release_and_teardown(g, parent_span).await;
                        }
                        return Err(failure);
                    }
                }
            }};
        }

        // 5b. Reset imported VMs into this scope's POV.
        let mut reset_seen = HashSet::new();
        for (alias, shells_map) in &dep_shells {
            let source_effect_name = alias_to_effect_name.get(alias).cloned();
            for (shell_local_name, vm_arc) in shells_map.iter() {
                let ptr = Arc::as_ptr(vm_arc) as usize;
                if reset_seen.insert(ptr) {
                    vm_arc.lock().await.reset_for_export(
                        scope.clone(),
                        Some(alias.clone()),
                        source_effect_name.clone(),
                        shell_local_name.clone(),
                    );
                }
            }
        }

        // Build local shells map, pre-populated with aliased dependency shells.
        // When a dependency is aliased (e.g. `start SetupDb as db`), its exported
        // shells are accessible by alias in the effect body (`shell db { ... }`
        // reuses the dependency's shell).
        let mut shells: HashMap<String, Arc<TokioMutex<Vm>>> = HashMap::new();
        for (alias, dep_exported) in &dep_shells {
            if dep_exported.len() == 1 {
                let vm_arc = dep_exported.values().next().unwrap().clone();
                shells.insert(alias.clone(), vm_arc);
            }
        }

        // 5c. Inject dependency-exposed variables into the effect scope so
        //      they're accessible via ${Alias.var_name} in shell blocks.
        {
            let mut vars = scope.vars().lock().await;
            for (alias, var_map) in &dep_vars {
                for (var_name, value) in var_map {
                    vars.insert(format!("{alias}.{var_name}"), value.clone());
                }
            }
        }

        // 6. Walk IrEffectItems (lets already evaluated, starts already instantiated)
        let mut cleanup_block = None;
        for item in effect.body() {
            match item {
                IrEffectItem::Comment { .. }
                | IrEffectItem::Expect { .. }
                | IrEffectItem::Start { .. }
                | IrEffectItem::Expose { .. }
                | IrEffectItem::Let { .. } => continue,
                IrEffectItem::Shell { block, .. } => {
                    let switch_span = block.name().span();
                    if let Some(qualifier) = block.qualifier() {
                        // Qualified: alias.shell { ... }
                        let alias = qualifier.name();
                        let shell_name = block.name().name();
                        let display = format!("{alias}.{shell_name}");
                        let block_span = self.rt_ctx.log.open_span(
                            SpanKind::ShellBlock {
                                shell: display.clone(),
                            },
                            Some(setup_span_id),
                            Some(switch_span),
                        );
                        let block_span_id = block_span.id();
                        let dep =
                            try_guards!(dep_shells.get(alias).ok_or_else(|| Failure::Runtime {
                                message: format!("unknown effect alias `{alias}`"),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }));
                        let vm_arc =
                            try_guards!(dep.get(shell_name).ok_or_else(|| Failure::Runtime {
                                message: format!(
                                    "effect alias `{alias}` does not expose shell `{shell_name}`"
                                ),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }));
                        let exec_result = {
                            let mut vm = vm_arc.lock().await;
                            let vm_name = vm.current_name();
                            let vm_marker = vm.shell_marker().to_string();
                            self.rt_ctx
                                .log
                                .emit_shell_switch(block_span_id, &vm_name, &vm_marker);
                            vm.set_block_span(block_span_id);
                            vm.exec_stmts(block.body()).await
                            // vm lock drops at end of this block, BEFORE try_guards! awaits any
                            // release_and_teardown that would re-lock the same vm via
                            // teardown_effect::shutdown.
                        };
                        try_guards!(exec_result);
                        // block_span drops here, closing the span.
                    } else {
                        // Unqualified: shell name { ... }
                        let name = block.name().name().to_string();
                        let block_span = self.rt_ctx.log.open_span(
                            SpanKind::ShellBlock {
                                shell: name.clone(),
                            },
                            Some(setup_span_id),
                            Some(switch_span),
                        );
                        let block_span_id = block_span.id();
                        if !shells.contains_key(&name) {
                            let shell_state = ShellState::new(name.clone());
                            let ctx = ExecutionContext::new(
                                scope.clone(),
                                shell_state,
                                self.rt_ctx.shell.default_timeout.clone(),
                                self.rt_ctx.env.clone(),
                                block_span_id,
                            );
                            let shell_key = ShellInstanceKey::Effect {
                                effect: key.clone(),
                                shell_name: name.clone(),
                            };
                            let vm = try_guards!(
                                Vm::new(name.clone(), shell_key.marker(), ctx, &self.rt_ctx).await
                            );
                            shells.insert(name.clone(), Arc::new(TokioMutex::new(vm)));
                        }
                        let exec_result = {
                            let vm_arc = shells.get(&name).expect("shell just inserted above");
                            let mut vm = vm_arc.lock().await;
                            let display_name = vm.current_name();
                            let display_marker = vm.shell_marker().to_string();
                            self.rt_ctx.log.emit_shell_switch(
                                block_span_id,
                                &display_name,
                                &display_marker,
                            );
                            vm.set_block_span(block_span_id);
                            vm.exec_stmts(block.body()).await
                            // vm lock drops at end of this block, BEFORE try_guards! awaits any
                            // release_and_teardown that would re-lock the same vm.
                        };
                        try_guards!(exec_result);
                        // block_span drops here, closing the span.
                    }
                }
                IrEffectItem::Cleanup { block, .. } => {
                    cleanup_block = Some(block.clone());
                }
            }
        }

        // 7. Resolve expose declarations — mark which shells/vars are exposed
        let mut exposed: HashSet<String> = HashSet::new();
        let mut exposed_vars: HashMap<String, String> = HashMap::new();

        let effect_vars = scope.vars().lock().await;
        for expose in effect.exposes() {
            let exposed_name = expose.exposed_name().to_string();
            match expose.kind() {
                relux_ir::IrExposeKind::Shell => {
                    if let Some(qualifier) = expose.qualifier() {
                        let dep = try_guards!(dep_shells.get(qualifier).ok_or_else(|| {
                            Failure::Runtime {
                                message: format!(
                                    "effect `{}` expose references unknown alias `{}`",
                                    effect.name().name(),
                                    qualifier,
                                ),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }
                        }));
                        let vm_arc = try_guards!(dep.get(expose.target()).ok_or_else(|| {
                            Failure::Runtime {
                                message: format!(
                                    "effect `{}` expose references shell `{}` not exposed by `{}`",
                                    effect.name().name(),
                                    expose.target(),
                                    qualifier,
                                ),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }
                        }));
                        shells.insert(exposed_name.clone(), vm_arc.clone());
                        exposed.insert(exposed_name.clone());
                    } else {
                        if !shells.contains_key(expose.target()) {
                            try_guards!(Err::<(), _>(Failure::Runtime {
                                message: format!(
                                    "effect `{}` expose references unknown shell `{}`",
                                    effect.name().name(),
                                    expose.target(),
                                ),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }));
                        }
                        if exposed_name != expose.target() {
                            let vm_arc = shells.get(expose.target()).unwrap().clone();
                            shells.insert(exposed_name.clone(), vm_arc);
                        }
                        exposed.insert(exposed_name.clone());
                    }
                    self.rt_ctx.log.emit_effect_expose_shell(
                        setup_span_id,
                        &exposed_name,
                        expose.target(),
                        expose.qualifier(),
                    );
                }
                relux_ir::IrExposeKind::Var => {
                    let value = if let Some(qualifier) = expose.qualifier() {
                        // Re-expose a variable from a dependency
                        let qualifier_vars =
                            try_guards!(dep_vars.get(qualifier).ok_or_else(|| {
                                Failure::Runtime {
                                    message: format!(
                                        "effect `{}` expose references unknown alias `{}`",
                                        effect.name().name(),
                                        qualifier,
                                    ),
                                    span: None,
                                    shell: None,
                                    context: FailureContext::default(),
                                }
                            }));
                        try_guards!(qualifier_vars.get(expose.target()).ok_or_else(|| {
                            Failure::Runtime {
                                message: format!(
                                    "effect `{}` expose references var `{}` not exposed by `{}`",
                                    effect.name().name(),
                                    expose.target(),
                                    qualifier,
                                ),
                                span: None,
                                shell: None,
                                context: FailureContext::default(),
                            }
                        }))
                        .clone()
                    } else {
                        // Expose a local let-bound variable
                        effect_vars.get(expose.target()).unwrap_or("").to_string()
                    };
                    exposed_vars.insert(exposed_name.clone(), value.clone());
                    self.rt_ctx.log.emit_effect_expose_var(
                        setup_span_id,
                        &exposed_name,
                        expose.target(),
                        expose.qualifier(),
                        &value,
                    );
                }
            }
        }
        drop(effect_vars);

        // 8. Terminate non-exposed local shells (deduplicate by Arc pointer).
        //    Collect pointers of exposed VMs first — a non-exposed key may alias
        //    the same Arc as an exposed key (e.g. backwards-compat single-shell alias),
        //    so we must not shut those down.
        let exposed_ptrs: HashSet<usize> = shells
            .iter()
            .filter(|(k, _)| exposed.contains(k.as_str()))
            .map(|(_, v)| Arc::as_ptr(v) as usize)
            .collect();
        let non_exposed_keys: Vec<String> = shells
            .keys()
            .filter(|k| !exposed.contains(k.as_str()))
            .cloned()
            .collect();
        for key in non_exposed_keys {
            if let Some(vm_arc) = shells.remove(&key) {
                let ptr = Arc::as_ptr(&vm_arc) as usize;
                if !exposed_ptrs.contains(&ptr) {
                    vm_arc.lock().await.shutdown().await;
                }
            }
        }

        // setup_span drops here, closing the span.

        Ok(EffectHandle {
            scope,
            shells,
            exposed,
            exposed_vars,
            dep_guards,
            cleanup: cleanup_block,
            setup_span: setup_span_id,
            key: key.clone(),
            marker,
            alias: start.alias().map(String::from),
        })
    }

    /// Surface form of an evaluated overlay, used wherever a structured
    /// `EffectSetup` span needs the overlay as `(key, value)` pairs.
    /// Same conversion used by bootstrap and reuse paths so dedup'd
    /// acquires render identically to bootstraps.
    fn evaluated_overlay_pairs(overlay: &Env) -> Vec<(String, String)> {
        overlay
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    async fn eval_overlay(
        &self,
        start: &IrEffectStart,
        caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        caller_span: SpanId,
    ) -> Result<Env, Failure> {
        let mut overlay = Env::new();
        let mut sink =
            crate::observe::structured::log_sink::LogSink::new(&self.rt_ctx.log, caller_span);
        for entry in start.overlay() {
            let value = relux_ir::evaluator::eval_pure_expr(
                entry.value(),
                caller_vars,
                caller_env,
                &self.rt_ctx.tables.pure_fns,
                &mut sink,
            );
            overlay.insert(entry.key().name().to_string(), value);
        }
        Ok(overlay)
    }

    async fn eval_effect_let(
        &self,
        stmt: &IrPureLetStmt,
        scope: &Scope,
        effect_env: &Arc<LayeredEnv>,
        setup_span: SpanId,
    ) {
        let mut vars = scope.vars().lock().await;
        let mut sink =
            crate::observe::structured::log_sink::LogSink::new(&self.rt_ctx.log, setup_span);
        let value = if let Some(expr) = stmt.value() {
            relux_ir::evaluator::eval_pure_expr(
                expr,
                &vars,
                effect_env,
                &self.rt_ctx.tables.pure_fns,
                &mut sink,
            )
        } else {
            String::new()
        };
        let name = stmt.name().name();
        vars.insert(name.to_string(), value.clone());
        drop(vars);
        self.rt_ctx
            .log
            .emit_var_let(setup_span, None, None, name, &value);
    }

    /// Glue: release one guard, then either run its cleanup body (when
    /// this caller was the last holder) or open a zero-duration
    /// deferred-cleanup span (otherwise).
    fn release_and_teardown<'a>(
        &'a self,
        guard: EffectGuard,
        parent_span: SpanId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Warning>> + Send + 'a>> {
        Box::pin(async move {
            match guard.release().await {
                ReleaseOutcome::LastHolder { handle } => {
                    self.teardown_effect(*handle, parent_span).await
                }
                ReleaseOutcome::Deferred {
                    effect,
                    alias,
                    setup_span,
                    marker,
                } => {
                    let deferred = self.rt_ctx.log.open_span(
                        SpanKind::EffectCleanup {
                            effect,
                            alias,
                            setup_span,
                            marker,
                            is_deferred: true,
                        },
                        Some(parent_span),
                        None,
                    );
                    deferred.close();
                    Vec::new()
                }
                ReleaseOutcome::Drift => Vec::new(),
            }
        })
    }

    /// Run cleanup for one effect we now exclusively own.
    ///
    /// Sequence:
    ///   1. Open `EffectCleanup` span (parent = `parent_span`).
    ///   2. Shut down all owned VMs (deduplicated by Arc pointer).
    ///   3. If a cleanup block exists, run it inside a `CleanupBlock`
    ///      span; collect `Warning::CleanupFailed` on error.
    ///   4. Close cleanup span.
    ///   5. Concurrently `release_and_teardown` every dep guard the
    ///      handle was holding.
    async fn teardown_effect(&self, handle: EffectHandle, parent_span: SpanId) -> Vec<Warning> {
        let effect_name = handle.scope.name().to_string();
        let setup_span = handle.setup_span;
        let alias = handle.alias.clone();
        let marker = handle.marker.clone();
        let mut warnings = Vec::new();

        let cleanup_span = self.rt_ctx.log.open_span(
            SpanKind::EffectCleanup {
                effect: effect_name.clone(),
                alias,
                setup_span,
                marker,
                is_deferred: false,
            },
            Some(parent_span),
            None,
        );
        let cleanup_span_id = cleanup_span.id();

        // Shut down all VMs (exposed and non-exposed, deduplicated).
        let mut seen = HashSet::new();
        for vm_arc in handle.shells.values() {
            let ptr = Arc::as_ptr(vm_arc) as usize;
            if seen.insert(ptr) {
                vm_arc.lock().await.shutdown().await;
            }
        }

        // Run cleanup block in fresh shell (best-effort).
        if let Some(cleanup_block) = &handle.cleanup {
            let block_loc = cleanup_block.span();
            let block_span = self.rt_ctx.log.open_span(
                SpanKind::CleanupBlock,
                Some(cleanup_span_id),
                Some(block_loc),
            );
            let block_span_id = block_span.id();
            let cleanup_shell_key = ShellInstanceKey::Effect {
                effect: handle.key.clone(),
                shell_name: "__cleanup".into(),
            };
            let cleanup_marker = cleanup_shell_key.marker();
            let cleanup_result = self
                .run_cleanup_block(cleanup_block, &handle.scope, &cleanup_marker, block_span_id)
                .await;
            if let Err(failure) = cleanup_result {
                self.rt_ctx.log.emit_warning(
                    block_span_id,
                    "__cleanup",
                    &cleanup_marker,
                    &format!("effect {effect_name} cleanup failed"),
                );
                warnings.push(Warning::CleanupFailed {
                    source: CleanupSource::Effect {
                        name: effect_name.clone(),
                    },
                    failure,
                });
            }
            // block_span drops here, closing the span.
        }

        // Concurrently release dep guards under our own cleanup span:
        // dep cleanups (including deferred-release spans for the
        // diamond's non-last holder) parent under this cleanup, not
        // under our caller. The diamond serialization still happens
        // inside `release` (atomic decrement under slot mutex);
        // join_all lets independent branches make progress.
        let dep_futures = handle
            .dep_guards
            .into_iter()
            .map(|g| self.release_and_teardown(g, cleanup_span_id));
        let dep_warnings: Vec<Warning> =
            join_all(dep_futures).await.into_iter().flatten().collect();
        warnings.extend(dep_warnings);

        // Close cleanup_span AFTER the recursion so deferred-cleanup
        // spans emitted by dep releases (and nested final-cleanup spans
        // from dep release-to-zero) are well-ordered children.
        cleanup_span.close();

        warnings
    }

    async fn run_cleanup_block(
        &self,
        cleanup_block: &IrCleanupBlock,
        scope: &Scope,
        cleanup_marker: &str,
        block_span: SpanId,
    ) -> Result<(), Failure> {
        let shell_state = ShellState::new("__cleanup".to_string());
        let ctx = ExecutionContext::new(
            scope.clone(),
            shell_state,
            self.rt_ctx.shell.default_timeout.clone(),
            self.rt_ctx.env.clone(),
            block_span,
        );
        // Cleanup uses its own uncancellable token
        let mut cleanup_rt_ctx = self.rt_ctx.clone();
        cleanup_rt_ctx.cancel = CancellationToken::new();
        let mut vm = Vm::new(
            "__cleanup".to_string(),
            cleanup_marker.to_string(),
            ctx,
            &cleanup_rt_ctx,
        )
        .await?;
        vm.exec_stmts(cleanup_block.body()).await?;
        vm.shutdown().await;
        Ok(())
    }
}
