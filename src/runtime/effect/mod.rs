pub mod registry;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use crate::dsl::resolver::ir::IrCleanupBlock;
use crate::dsl::resolver::ir::IrEffectItem;
use crate::dsl::resolver::ir::IrEffectNeed;
use crate::dsl::resolver::ir::IrPureLetStmt;
use crate::pure::Env;
use crate::pure::VarScope;
use crate::runtime::RuntimeContext;
use crate::runtime::effect::registry::EffectHandle;
use crate::runtime::effect::registry::EffectInstanceKey;
use crate::runtime::effect::registry::EffectRegistry;
use crate::runtime::effect::registry::EffectSlot;
use crate::runtime::report::result::Failure;
use crate::runtime::vm::Vm;
use crate::runtime::vm::context::ExecutionContext;
use crate::runtime::vm::context::Scope;
use crate::runtime::vm::context::ShellState;

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

    /// Acquire all needs. Each need recursively acquires its own
    /// dependencies before bootstrapping itself.
    #[allow(clippy::type_complexity)]
    pub fn instantiate<'a>(
        &'a self,
        needs: &'a [IrEffectNeed],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<Arc<TokioMutex<Vm>>>, Failure>> + Send + 'a,
        >,
    > {
        Box::pin(async move {
            let mut vms = Vec::with_capacity(needs.len());
            for need in needs {
                let key = EffectInstanceKey::from(need);
                vms.push(self.acquire(&key, need).await?);
            }
            Ok(vms)
        })
    }

    /// Release all needs. Refcount-based — last releaser runs
    /// cleanup and recursively releases dependencies.
    pub async fn cleanup(&self, needs: &[IrEffectNeed]) -> Vec<Warning> {
        let mut warnings = Vec::new();
        for need in needs {
            let key = EffectInstanceKey::from(need);
            warnings.extend(self.run_cleanup(&key).await);
        }
        warnings
    }

    async fn acquire(
        &self,
        key: &EffectInstanceKey,
        need: &IrEffectNeed,
    ) -> Result<Arc<TokioMutex<Vm>>, Failure> {
        let slot = self.registry.slot(key);
        let mut guard = slot.lock().await;

        match &mut *guard {
            EffectSlot::Ready { refcount, handle } => {
                *refcount += 1;
                Ok(handle.exported_vm.clone())
            }
            EffectSlot::Failed(failure) => Err(failure.clone()),
            EffectSlot::Empty => match self.bootstrap_effect(need).await {
                Ok(handle) => {
                    let vm_arc = handle.exported_vm.clone();
                    *guard = EffectSlot::Ready {
                        refcount: 1,
                        handle,
                    };
                    Ok(vm_arc)
                }
                Err(failure) => {
                    self.rt_ctx.events.emit_error("", failure.summary());
                    *guard = EffectSlot::Failed(failure.clone());
                    Err(failure)
                }
            },
        }
    }

    async fn bootstrap_effect(&self, need: &IrEffectNeed) -> Result<EffectHandle, Failure> {
        let effect_name = need.effect().to_string();
        self.rt_ctx.events.emit_effect_setup("", &effect_name);

        let effect_result = self
            .rt_ctx
            .tables
            .effects
            .get(need.effect())
            .ok_or_else(|| Failure::Runtime {
                message: format!("effect {:?} not found in table", need.effect()),
                span: None,
                shell: None,
            })?;
        let effect = effect_result.as_ref().map_err(|e| Failure::Runtime {
            message: format!("effect resolution failed: {e:?}"),
            span: None,
            shell: None,
        })?;

        // 1. Recursively instantiate sub-dependencies
        let exported_deps = self.instantiate(effect.needs()).await?;

        // 2. Build shell map from dependency exported shells
        let mut shells: HashMap<String, Arc<TokioMutex<Vm>>> = HashMap::new();
        for (sub_need, vm_arc) in effect.needs().iter().zip(exported_deps) {
            if let Some(alias) = sub_need.alias() {
                let source = vm_arc.lock().await.current_name();
                self.rt_ctx.events.emit_shell_alias(alias, source);
                shells.insert(alias.to_string(), vm_arc);
            }
        }

        // 3. Evaluate overlay → build env_overlay
        let env_overlay = self.eval_overlay(need).await?;
        let env_overlay = Arc::new(env_overlay);

        // 4. Create effect scope
        let scope = Scope::Effect {
            name: effect.name().name().to_string(),
            vars: Arc::new(TokioMutex::new(VarScope::new())),
            _timeout: None,
            env_overlay: env_overlay.clone(),
        };

        // 4b. Reset imported VMs
        let mut reset_seen = HashSet::new();
        for vm_arc in shells.values() {
            let ptr = Arc::as_ptr(vm_arc) as usize;
            if reset_seen.insert(ptr) {
                vm_arc.lock().await.reset_for_export(scope.clone());
            }
        }

        // 5. Walk IrEffectItems
        let mut cleanup_block = None;
        for item in effect.body() {
            match item {
                IrEffectItem::Comment { .. } | IrEffectItem::Need { .. } => continue,
                IrEffectItem::Let { stmt, .. } => {
                    self.eval_effect_let(stmt, &scope, &env_overlay).await;
                }
                IrEffectItem::Shell { block, .. } => {
                    let name = block.name().name().to_string();
                    self.rt_ctx.events.emit_shell_switch(&name);
                    if !shells.contains_key(&name) {
                        let shell_state =
                            ShellState::new(name.clone(), None, Some(env_overlay.clone()));
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
                    self.rt_ctx.events.emit_shell_switch(&display_name);
                    vm.exec_stmts(block.body()).await?;
                }
                IrEffectItem::Cleanup { block, .. } => {
                    cleanup_block = Some(block.clone());
                }
            }
        }

        // 6. Extract exported shell
        let exported_name = effect.exported_shell().name().to_string();
        let exported_vm = shells
            .remove(&exported_name)
            .ok_or_else(|| Failure::Runtime {
                message: format!(
                    "effect `{}` exported shell `{}` not created",
                    effect.name().name(),
                    exported_name
                ),
                span: None,
                shell: None,
            })?;

        // 7. Terminate non-exported shells (deduplicate by Arc pointer)
        let mut seen = HashSet::new();
        for (_, vm_arc) in shells.drain() {
            let ptr = Arc::as_ptr(&vm_arc) as usize;
            if seen.insert(ptr) {
                vm_arc.lock().await.shutdown().await;
            }
        }

        Ok(EffectHandle {
            scope,
            exported_vm,
            dependencies: effect.needs().iter().map(EffectInstanceKey::from).collect(),
            cleanup: cleanup_block,
        })
    }

    async fn eval_overlay(&self, need: &IrEffectNeed) -> Result<Env, Failure> {
        let mut overlay = Env::new();
        let vars = VarScope::new();
        for entry in need.overlay() {
            let value = crate::pure::evaluator::eval_pure_expr(
                entry.value(),
                &vars,
                &self.rt_ctx.env,
                &self.rt_ctx.tables.pure_fns,
            );
            overlay.insert(entry.key().name().to_string(), value);
        }
        Ok(overlay)
    }

    async fn eval_effect_let(&self, stmt: &IrPureLetStmt, scope: &Scope, _env_overlay: &Arc<Env>) {
        let vars = VarScope::new();
        let value = if let Some(expr) = stmt.value() {
            crate::pure::evaluator::eval_pure_expr(
                expr,
                &vars,
                &self.rt_ctx.env,
                &self.rt_ctx.tables.pure_fns,
            )
        } else {
            String::new()
        };
        scope
            .vars()
            .lock()
            .await
            .insert(stmt.name().name().to_string(), value);
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

                    self.rt_ctx.events.emit_effect_teardown("", &effect_name);

                    // 1. Shut down the exported VM
                    handle.exported_vm.lock().await.shutdown().await;

                    // 2. Run cleanup block in fresh shell (best-effort)
                    if let Some(cleanup_block) = &handle.cleanup {
                        self.rt_ctx.events.emit_cleanup("__cleanup");
                        let cleanup_result =
                            self.run_cleanup_block(cleanup_block, &handle.scope).await;
                        if let Err(failure) = cleanup_result {
                            self.rt_ctx.events.emit_warning(
                                "__cleanup",
                                format!("effect {effect_name} cleanup failed"),
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
        let shell_state = ShellState::new("__cleanup".to_string(), None, None);
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
