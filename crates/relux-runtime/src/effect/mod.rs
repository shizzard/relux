pub mod registry;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use futures::future::join_all;

use crate::RuntimeContext;
use crate::effect::registry::EffectHandle;
use crate::effect::registry::EffectInstanceKey;
use crate::effect::registry::EffectRegistry;
use crate::effect::registry::EffectSlot;
use crate::report::result::Failure;
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

#[derive(Clone)]
pub struct EffectManager {
    registry: Arc<EffectRegistry>,
    pub(crate) rt_ctx: RuntimeContext,
}

impl EffectManager {
    pub fn new(registry: Arc<EffectRegistry>, rt_ctx: RuntimeContext) -> Self {
        Self { registry, rt_ctx }
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
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Vec<(EffectInstanceKey, HashMap<String, Arc<TokioMutex<Vm>>>)>,
                        Failure,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let mut results = Vec::with_capacity(starts.len());
            for start in starts {
                // Evaluate overlay first to build runtime identity key
                let evaluated = self.eval_overlay(start, caller_vars, caller_env).await?;

                // Look up effect's expect names for identity key
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

                let shells = self
                    .acquire(&key, start, caller_vars, caller_env, evaluated)
                    .await?;
                results.push((key, shells));
            }
            Ok(results)
        })
    }

    /// Release all effects acquired during this test run.
    /// Runs one `run_cleanup` per acquisition (matching the symmetric `acquire` calls),
    /// concurrently. The slot mutex serializes access and refcount ensures the last
    /// releaser triggers actual teardown + recursive dependency cleanup.
    pub async fn cleanup_all(&self) -> Vec<Warning> {
        let keys = self.registry.acquired_keys();
        let futures: Vec<_> = keys.iter().map(|key| self.run_cleanup(key)).collect();
        let results = join_all(futures).await;
        results.into_iter().flatten().collect()
    }

    async fn acquire(
        &self,
        key: &EffectInstanceKey,
        start: &IrEffectStart,
        caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        evaluated_overlay: Env,
    ) -> Result<HashMap<String, Arc<TokioMutex<Vm>>>, Failure> {
        let slot = self.registry.slot(key);
        let mut guard = slot.lock().await;

        let result = match &mut *guard {
            EffectSlot::Ready { refcount, handle } => {
                *refcount += 1;
                Ok(handle.exposed_shells())
            }
            EffectSlot::Failed(failure) => Err(failure.clone()),
            EffectSlot::Empty => match self
                .bootstrap_effect(start, caller_vars, caller_env, evaluated_overlay)
                .await
            {
                Ok(handle) => {
                    let exposed = handle.exposed_shells();
                    *guard = EffectSlot::Ready {
                        refcount: 1,
                        handle,
                    };
                    Ok(exposed)
                }
                Err(failure) => {
                    self.rt_ctx.events.emit_error("", failure.summary(), None);
                    *guard = EffectSlot::Failed(failure.clone());
                    Err(failure)
                }
            },
        };
        if result.is_ok() {
            self.registry.record_acquisition(key.clone());
        }
        result
    }

    async fn bootstrap_effect(
        &self,
        start: &IrEffectStart,
        _caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
        evaluated_overlay: Env,
    ) -> Result<EffectHandle, Failure> {
        let effect_name = start.effect().to_string();
        self.rt_ctx.events.emit_effect_setup("", &effect_name, None);

        let effect_result = self
            .rt_ctx
            .tables
            .effects
            .get(start.effect())
            .ok_or_else(|| Failure::Runtime {
                message: format!("effect {:?} not found in table", start.effect()),
                span: None,
                shell: None,
            })?;
        let effect = effect_result.as_ref().map_err(|e| Failure::Runtime {
            message: format!("effect resolution failed: {e:?}"),
            span: None,
            shell: None,
        })?;

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
                self.eval_effect_let(stmt, &scope, &effect_env).await;
            }
        }

        // 4. Recursively instantiate sub-dependencies (effect's vars available to sub-overlays)
        let effect_vars = scope.vars().lock().await.clone();
        let exported_deps = self
            .instantiate(effect.starts(), &effect_vars, &effect_env)
            .await?;

        // 5. Build dependency shells map (alias → exported shells) and collect dep keys
        let mut dep_shells: HashMap<String, HashMap<String, Arc<TokioMutex<Vm>>>> = HashMap::new();
        let mut dep_keys: Vec<EffectInstanceKey> = Vec::new();
        for (sub_start, (dep_key, exported)) in effect.starts().iter().zip(exported_deps) {
            dep_keys.push(dep_key);
            if let Some(alias) = sub_start.alias() {
                dep_shells.insert(alias.to_string(), exported);
            }
        }

        // 5b. Reset imported VMs
        let mut reset_seen = HashSet::new();
        for shells_map in dep_shells.values() {
            for vm_arc in shells_map.values() {
                let ptr = Arc::as_ptr(vm_arc) as usize;
                if reset_seen.insert(ptr) {
                    vm_arc.lock().await.reset_for_export(scope.clone());
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
                self.rt_ctx
                    .events
                    .emit_shell_alias(alias, vm_arc.lock().await.current_name());
                shells.insert(alias.clone(), vm_arc);
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
                    let exit_span = block.span().at_end();
                    if let Some(qualifier) = block.qualifier() {
                        // Qualified: alias.shell { ... }
                        let alias = qualifier.name();
                        let shell_name = block.name().name();
                        let display = format!("{alias}.{shell_name}");
                        self.rt_ctx
                            .events
                            .emit_shell_switch(&display, Some(switch_span));
                        let dep = dep_shells.get(alias).ok_or_else(|| Failure::Runtime {
                            message: format!("unknown effect alias `{alias}`"),
                            span: None,
                            shell: None,
                        })?;
                        let vm_arc = dep.get(shell_name).ok_or_else(|| Failure::Runtime {
                            message: format!(
                                "effect alias `{alias}` does not expose shell `{shell_name}`"
                            ),
                            span: None,
                            shell: None,
                        })?;
                        let mut vm = vm_arc.lock().await;
                        self.rt_ctx
                            .events
                            .emit_shell_switch(vm.current_name(), Some(switch_span));
                        vm.exec_stmts(block.body()).await?;
                        vm.set_exit_span(exit_span);
                    } else {
                        // Unqualified: shell name { ... }
                        let name = block.name().name().to_string();
                        self.rt_ctx
                            .events
                            .emit_shell_switch(&name, Some(switch_span));
                        if !shells.contains_key(&name) {
                            let shell_state = ShellState::new(name.clone(), None);
                            let ctx = ExecutionContext::new(
                                scope.clone(),
                                shell_state,
                                self.rt_ctx.shell.default_timeout.clone(),
                                self.rt_ctx.env.clone(),
                            );
                            let vm = Vm::new(name.clone(), ctx, &self.rt_ctx).await?;
                            shells.insert(name.clone(), Arc::new(TokioMutex::new(vm)));
                        }
                        let vm_arc = shells.get(&name).expect("shell just inserted above");
                        let mut vm = vm_arc.lock().await;
                        let display_name = vm.current_name().to_string();
                        self.rt_ctx
                            .events
                            .emit_shell_switch(&display_name, Some(switch_span));
                        vm.exec_stmts(block.body()).await?;
                        vm.set_exit_span(exit_span);
                    }
                }
                IrEffectItem::Cleanup { block, .. } => {
                    cleanup_block = Some(block.clone());
                }
            }
        }

        // 7. Resolve expose declarations — mark which shells are exposed
        let mut exposed: HashSet<String> = HashSet::new();
        for expose in effect.exposes() {
            let exposed_name = expose.exposed_name().to_string();
            if let Some(qualifier) = expose.qualifier() {
                // Qualified: `expose alias.shell [as name]` — from dependency
                let dep = dep_shells.get(qualifier).ok_or_else(|| Failure::Runtime {
                    message: format!(
                        "effect `{}` expose references unknown alias `{}`",
                        effect.name().name(),
                        qualifier,
                    ),
                    span: None,
                    shell: None,
                })?;
                let vm_arc = dep.get(expose.shell()).ok_or_else(|| Failure::Runtime {
                    message: format!(
                        "effect `{}` expose references shell `{}` not exposed by `{}`",
                        effect.name().name(),
                        expose.shell(),
                        qualifier,
                    ),
                    span: None,
                    shell: None,
                })?;
                shells.insert(exposed_name.clone(), vm_arc.clone());
                exposed.insert(exposed_name);
            } else {
                // Simple: `expose shell [as name]` — local shell
                if !shells.contains_key(expose.shell()) {
                    return Err(Failure::Runtime {
                        message: format!(
                            "effect `{}` expose references unknown shell `{}`",
                            effect.name().name(),
                            expose.shell(),
                        ),
                        span: None,
                        shell: None,
                    });
                }
                if exposed_name != expose.shell() {
                    // Aliased expose: insert under the exposed name too
                    let vm_arc = shells.get(expose.shell()).unwrap().clone();
                    shells.insert(exposed_name.clone(), vm_arc);
                }
                exposed.insert(exposed_name);
            }
        }

        // If no expose declarations, fall back to exposing the first local shell
        // (backwards compatibility for effects without explicit expose)
        if exposed.is_empty()
            && !shells.is_empty()
            && let Some(first_name) = effect.body().iter().find_map(|item| {
                if let IrEffectItem::Shell { block, .. } = item {
                    Some(block.name().name().to_string())
                } else {
                    None
                }
            })
            && shells.contains_key(&first_name)
        {
            exposed.insert(first_name);
        }

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

        Ok(EffectHandle {
            scope,
            shells,
            exposed,
            dependencies: dep_keys,
            cleanup: cleanup_block,
        })
    }

    async fn eval_overlay(
        &self,
        start: &IrEffectStart,
        caller_vars: &VarScope,
        caller_env: &Arc<LayeredEnv>,
    ) -> Result<Env, Failure> {
        let mut overlay = Env::new();
        for entry in start.overlay() {
            let value = relux_ir::evaluator::eval_pure_expr(
                entry.value(),
                caller_vars,
                caller_env,
                &self.rt_ctx.tables.pure_fns,
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
    ) {
        let mut vars = scope.vars().lock().await;
        let value = if let Some(expr) = stmt.value() {
            relux_ir::evaluator::eval_pure_expr(
                expr,
                &vars,
                effect_env,
                &self.rt_ctx.tables.pure_fns,
            )
        } else {
            String::new()
        };
        vars.insert(stmt.name().name().to_string(), value);
    }

    fn run_cleanup<'a>(
        &'a self,
        key: &'a EffectInstanceKey,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Warning>> + Send + 'a>> {
        Box::pin(async move { self.run_cleanup_inner(key).await })
    }

    async fn run_cleanup_inner(&self, key: &EffectInstanceKey) -> Vec<Warning> {
        let slot = self.registry.slot(key);
        let mut guard = slot.lock().await;
        let mut warnings = Vec::new();

        match &mut *guard {
            EffectSlot::Ready { refcount, handle } => {
                *refcount -= 1;

                if *refcount == 0 {
                    let effect_name = handle.scope.name().to_string();

                    self.rt_ctx
                        .events
                        .emit_effect_teardown("", &effect_name, None);

                    // 1. Shut down all VMs (exposed and non-exposed, deduplicated)
                    let mut seen = HashSet::new();
                    for vm_arc in handle.shells.values() {
                        let ptr = Arc::as_ptr(vm_arc) as usize;
                        if seen.insert(ptr) {
                            vm_arc.lock().await.shutdown().await;
                        }
                    }

                    // 2. Run cleanup block in fresh shell (best-effort)
                    if let Some(cleanup_block) = &handle.cleanup {
                        let cleanup_span = cleanup_block.span();
                        self.rt_ctx
                            .events
                            .emit_cleanup("__cleanup", Some(cleanup_span));
                        let cleanup_result =
                            self.run_cleanup_block(cleanup_block, &handle.scope).await;
                        if let Err(failure) = cleanup_result {
                            self.rt_ctx.events.emit_warning(
                                "__cleanup",
                                format!("effect {effect_name} cleanup failed"),
                                Some(cleanup_span),
                            );
                            warnings.push(Warning::CleanupFailed {
                                source: CleanupSource::Effect { name: effect_name },
                                failure,
                            });
                        }
                    }

                    let deps = handle.dependencies.clone();
                    *guard = EffectSlot::Empty;
                    drop(guard);

                    // 3. Recursively release dependencies
                    for dep in &deps {
                        warnings.extend(self.run_cleanup(dep).await);
                    }
                }
            }
            EffectSlot::Failed(_) => {
                // nothing to clean up
            }
            EffectSlot::Empty => {
                // Should not happen in normal use, but don't panic
            }
        }

        warnings
    }

    async fn run_cleanup_block(
        &self,
        cleanup_block: &IrCleanupBlock,
        scope: &Scope,
    ) -> Result<(), Failure> {
        let shell_state = ShellState::new("__cleanup".to_string(), None);
        let ctx = ExecutionContext::new(
            scope.clone(),
            shell_state,
            self.rt_ctx.shell.default_timeout.clone(),
            self.rt_ctx.env.clone(),
        );
        // Cleanup uses its own uncancellable token
        let mut cleanup_rt_ctx = self.rt_ctx.clone();
        cleanup_rt_ctx.cancel = CancellationToken::new();
        let mut vm = Vm::new("__cleanup".to_string(), ctx, &cleanup_rt_ctx).await?;
        vm.exec_stmts(cleanup_block.body()).await?;
        vm.shutdown().await;
        Ok(())
    }
}
