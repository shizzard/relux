pub mod registry;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use crate::dsl::resolver::ir::{IrCleanupBlock, IrEffectItem, IrEffectNeed, IrPureLetStmt, Tables};
use crate::pure::{Env, VarScope};
use crate::runtime::effect::registry::{
    EffectHandle, EffectInstanceKey, EffectRegistry, EffectSlot,
};
use crate::runtime::observe::event_log::{EventCollector, LogEventKind};
use crate::runtime::observe::progress::{ProgressEvent, ProgressTx};
use crate::runtime::report::result::Failure;
use crate::runtime::vm::Vm;
use crate::runtime::vm::context::{ExecutionContext, Scope, ShellState};

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
    pub(crate) tables: Tables,
    pub(crate) env: Arc<Env>,
    pub(crate) shell_command: Arc<str>,
    pub(crate) shell_prompt: Arc<str>,
    pub(crate) default_timeout: crate::dsl::resolver::ir::IrTimeout,
    pub(crate) progress_tx: Option<ProgressTx>,
    pub(crate) log_dir: Arc<Path>,
    pub(crate) test_start: Instant,
    pub(crate) event_collector: Option<EventCollector>,
    pub(crate) cancel: CancellationToken,
}

impl EffectManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tables: Tables,
        env: Arc<Env>,
        shell_command: &str,
        shell_prompt: &str,
        default_timeout: crate::dsl::resolver::ir::IrTimeout,
        progress_tx: Option<ProgressTx>,
        log_dir: &Path,
        test_start: Instant,
        event_collector: Option<EventCollector>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            registry: Arc::new(EffectRegistry::new()),
            tables,
            env,
            shell_command: Arc::from(shell_command),
            shell_prompt: Arc::from(shell_prompt),
            default_timeout,
            progress_tx,
            log_dir: Arc::from(log_dir),
            test_start,
            event_collector,
            cancel,
        }
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

    fn emit_progress(&self, event: ProgressEvent) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(event);
        }
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
                    self.emit_progress(ProgressEvent::Error(failure.summary()));
                    *guard = EffectSlot::Failed(failure.clone());
                    Err(failure)
                }
            },
        }
    }

    async fn bootstrap_effect(&self, need: &IrEffectNeed) -> Result<EffectHandle, Failure> {
        let effect_name = need.effect().to_string();
        self.emit_progress(ProgressEvent::EffectSetup(effect_name.clone()));
        if let Some(ec) = &self.event_collector {
            ec.push(
                "",
                LogEventKind::EffectSetup {
                    effect: effect_name.clone(),
                },
            )
            .await;
        }

        let effect_result =
            self.tables
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
                if let Some(ec) = &self.event_collector {
                    let source = vm_arc.lock().await.current_name();
                    ec.push(
                        alias,
                        LogEventKind::ShellAlias {
                            name: alias.to_string(),
                            source,
                        },
                    )
                    .await;
                }
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
                    self.emit_progress(ProgressEvent::ShellSwitch(name.clone()));
                    if !shells.contains_key(&name) {
                        let shell_state =
                            ShellState::new(name.clone(), None, Some(env_overlay.clone()));
                        let ctx = ExecutionContext::new(
                            scope.clone(),
                            shell_state,
                            self.default_timeout.clone(),
                            self.env.clone(),
                        );
                        let vm = Vm::new(
                            name.clone(),
                            self.shell_prompt.to_string(),
                            self.shell_command.to_string(),
                            ctx,
                            self.tables.clone(),
                            self.progress_tx.clone(),
                            &self.log_dir,
                            self.test_start,
                            self.event_collector.clone(),
                            self.cancel.clone(),
                        )
                        .await?;
                        shells.insert(name.clone(), Arc::new(TokioMutex::new(vm)));
                    }
                    let vm_arc = shells.get(&name).expect("shell just inserted above");
                    let mut vm = vm_arc.lock().await;
                    let display_name = vm.current_name().to_string();
                    if let Some(ec) = &self.event_collector {
                        ec.push(
                            &display_name,
                            LogEventKind::ShellSwitch {
                                name: display_name.clone(),
                            },
                        )
                        .await;
                    }
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
                &self.env,
                &self.tables.pure_fns,
            );
            overlay.insert(entry.key().name().to_string(), value);
        }
        Ok(overlay)
    }

    async fn eval_effect_let(&self, stmt: &IrPureLetStmt, scope: &Scope, _env_overlay: &Arc<Env>) {
        let vars = VarScope::new();
        let value = if let Some(expr) = stmt.value() {
            crate::pure::evaluator::eval_pure_expr(expr, &vars, &self.env, &self.tables.pure_fns)
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

                    if let Some(ec) = &self.event_collector {
                        ec.push(
                            "",
                            LogEventKind::EffectTeardown {
                                effect: effect_name.clone(),
                            },
                        )
                        .await;
                    }

                    // 1. Shut down the exported VM
                    handle.exported_vm.lock().await.shutdown().await;

                    // 2. Run cleanup block in fresh shell (best-effort)
                    if let Some(cleanup_block) = &handle.cleanup {
                        self.emit_progress(ProgressEvent::Cleanup);
                        if let Some(ec) = &self.event_collector {
                            ec.push(
                                "__cleanup",
                                LogEventKind::Cleanup {
                                    shell: format!("{effect_name}.__cleanup"),
                                },
                            )
                            .await;
                        }
                        let cleanup_result =
                            self.run_cleanup_block(cleanup_block, &handle.scope).await;
                        if let Err(failure) = cleanup_result {
                            self.emit_progress(ProgressEvent::Warning(format!(
                                "effect {effect_name} cleanup failed"
                            )));
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
            self.default_timeout.clone(),
            self.env.clone(),
        );
        let mut vm = Vm::new(
            "__cleanup".to_string(),
            self.shell_prompt.to_string(),
            self.shell_command.to_string(),
            ctx,
            self.tables.clone(),
            None,
            &self.log_dir,
            self.test_start,
            self.event_collector.clone(),
            CancellationToken::new(), // intentionally uncancellable: cleanup must always complete
        )
        .await?;
        vm.exec_stmts(cleanup_block.body()).await?;
        vm.shutdown().await;
        Ok(())
    }
}
