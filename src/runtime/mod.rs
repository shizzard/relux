use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daggy::petgraph::algo::toposort;
use daggy::petgraph::visit::{EdgeRef, IntoEdgesDirected};
use tokio::sync::Mutex;

use crate::dsl::resolver::ir::{self, EffectInstance, InstanceId, Plan, SourceMap, Spanned};
use crate::runtime::result::{Failure, Outcome, TestResult};
use crate::runtime::vars::{Env, TestScope, VariableStack, interpolate_with_lookup};
use crate::runtime::vm::Vm;

pub mod result;
pub mod vars;
pub mod vm;

pub const DEFAULT_SHELL_PROMPT: &str = "relux> ";

pub type SharedVm = Arc<Mutex<Vm>>;

pub struct CodeServer {
    functions: Vec<ir::Function>,
    index: HashMap<(String, usize), usize>,
}

impl CodeServer {
    pub fn new(functions: Vec<ir::Function>) -> Self {
        let mut index = HashMap::new();
        for (i, f) in functions.iter().enumerate() {
            index.insert((f.name.node.clone(), f.params.len()), i);
        }
        Self { functions, index }
    }

    pub fn lookup(&self, name: &str, arity: usize) -> Option<usize> {
        self.index.get(&(name.to_string(), arity)).copied()
    }

    pub fn get(&self, fn_id: usize) -> Option<&ir::Function> {
        self.functions.get(fn_id)
    }
}

pub struct Runtime {
    env: Arc<Env>,
    source_map: SourceMap,
    default_timeout: Duration,
}

pub struct RunContext {
    pub run_id: String,
    pub artifacts_dir: PathBuf,
}

impl Runtime {
    pub fn new(source_map: SourceMap, run_context: RunContext) -> Self {
        let mut env = std::env::vars().collect::<HashMap<_, _>>();
        env.insert("__RELUX_RUN_ID".to_string(), run_context.run_id);
        env.insert(
            "__RELUX_RUN_ARTIFACTS".to_string(),
            run_context.artifacts_dir.display().to_string(),
        );
        env.insert(
            "__RELUX_SHELL_PROMPT".to_string(),
            DEFAULT_SHELL_PROMPT.to_string(),
        );
        Self {
            env: Arc::new(env),
            source_map,
            default_timeout: Duration::from_secs(10),
        }
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub async fn run(&self, plans: Vec<Plan>) -> Vec<TestResult> {
        let mut results = Vec::with_capacity(plans.len());
        for plan in plans {
            results.push(self.run_plan(plan).await);
        }
        results
    }

    async fn run_plan(&self, plan: Plan) -> TestResult {
        let start = Instant::now();
        let test_name = plan.test.name.node.clone();
        let code_server = Arc::new(CodeServer::new(plan.functions.clone()));
        let mut shell_logs = HashMap::new();

        let mut effect_exec = self
            .execute_effects(&plan, code_server.clone(), &mut shell_logs)
            .await;

        let outcome = match effect_exec.outcome.take() {
            Some(outcome) => outcome,
            None => {
                let test_scope = Arc::new(Mutex::new(TestScope::new()));
                for decl in &plan.test.vars {
                    let value = if let Some(expr) = &decl.node.value {
                        let vars = VariableStack::new(
                            test_scope.clone(),
                            HashMap::new(),
                            self.env.clone(),
                        );
                        self.eval_static_expr(expr, &vars).await.unwrap_or_default()
                    } else {
                        String::new()
                    };
                    test_scope
                        .lock()
                        .await
                        .insert(decl.node.name.node.clone(), value);
                }

                self.run_test_body(
                    &plan,
                    test_scope,
                    code_server,
                    &mut effect_exec.alias_shells,
                )
                .await
            }
        };

        self.run_test_cleanup(&plan, &effect_exec.alias_shells)
            .await;
        self.teardown_effects(&plan, &mut effect_exec).await;

        for (name, vm) in &effect_exec.alias_shells {
            let out = vm.lock().await.output_snapshot().await;
            shell_logs.insert(name.clone(), out);
        }

        TestResult {
            test_name,
            outcome,
            duration: start.elapsed(),
            shell_logs,
        }
    }

    async fn execute_effects(
        &self,
        plan: &Plan,
        code_server: Arc<CodeServer>,
        shell_logs: &mut HashMap<String, Vec<u8>>,
    ) -> EffectExecution {
        let mut effect_state = EffectExecution::default();
        let order = match toposort(&plan.effect_graph.dag, None) {
            Ok(v) => v,
            Err(e) => {
                effect_state.outcome = Some(Outcome::Fail(Failure::Runtime {
                    message: format!("effect graph has cycle at {:?}", e.node_id()),
                    span: Some(plan.test.span.clone()),
                    shell: None,
                }));
                return effect_state;
            }
        };

        for instance_id in order {
            let Some(instance) = plan.effect_graph.dag.node_weight(instance_id).cloned() else {
                continue;
            };
            let overlay = self.interpolate_overlay(&instance.overlay);
            let effect = &plan.effects[instance.effect];
            let effect_scope = Arc::new(Mutex::new(TestScope::new()));
            let mut shells: HashMap<String, SharedVm> = HashMap::new();

            for incoming in plan
                .effect_graph
                .dag
                .edges_directed(instance_id, daggy::petgraph::Direction::Incoming)
            {
                let dep_id = incoming.source();
                if let Some(dep) = effect_state.instances.get(&dep_id) {
                    shells.insert(
                        incoming.weight().alias.node.clone(),
                        dep.exported_vm.clone(),
                    );
                }
            }

            let mut setup_failed = None;
            for var in &effect.vars {
                let value = if let Some(expr) = &var.node.value {
                    let vars =
                        VariableStack::new(effect_scope.clone(), overlay.clone(), self.env.clone());
                    match self.eval_static_expr(expr, &vars).await {
                        Ok(v) => v,
                        Err(f) => {
                            setup_failed = Some(f);
                            break;
                        }
                    }
                } else {
                    String::new()
                };
                effect_scope
                    .lock()
                    .await
                    .insert(var.node.name.node.clone(), value);
            }

            if setup_failed.is_none() {
                for block in &effect.shells {
                    let shell_name = block.node.name.node.clone();
                    if !shells.contains_key(&shell_name) {
                        let vars = VariableStack::new(
                            effect_scope.clone(),
                            overlay.clone(),
                            self.env.clone(),
                        );
                        match Vm::new(
                            shell_name.clone(),
                            vars,
                            self.default_timeout,
                            code_server.clone(),
                        )
                        .await
                        {
                            Ok(vm) => {
                                shells.insert(shell_name.clone(), Arc::new(Mutex::new(vm)));
                            }
                            Err(f) => {
                                setup_failed = Some(f);
                                break;
                            }
                        }
                    }
                    if let Some(vm) = shells.get(&shell_name) {
                        let mut guard = vm.lock().await;
                        if let Err(f) = guard.exec_stmts(&block.node.stmts).await {
                            setup_failed = Some(f);
                            break;
                        }
                    }
                }
            }

            if let Some(failure) = setup_failed {
                let reason = format!("effect setup failed: {failure:?}");
                effect_state.outcome = Some(Outcome::Skipped(reason));
                for vm in shells.values() {
                    let out = vm.lock().await.output_snapshot().await;
                    shell_logs.insert(format!("effect:{}", effect.name.node), out);
                }
                effect_state.failures.push(failure);
                break;
            }

            let exported_name = effect.exported_shell.node.clone();
            let Some(exported_vm) = shells.get(&exported_name).cloned() else {
                effect_state.outcome = Some(Outcome::Fail(Failure::Runtime {
                    message: format!(
                        "effect `{}` exported shell `{}` not created",
                        effect.name.node, exported_name
                    ),
                    span: Some(effect.span.clone()),
                    shell: None,
                }));
                break;
            };

            effect_state.instances.insert(
                instance_id,
                EffectInstanceState {
                    info: instance,
                    all_shells: shells,
                    exported_vm,
                },
            );
        }

        for need in &plan.test.needs {
            if let Some(state) = effect_state.instances.get(&need.node.instance) {
                effect_state
                    .alias_shells
                    .insert(need.node.alias.node.clone(), state.exported_vm.clone());
            }
        }

        effect_state
    }

    async fn run_test_body(
        &self,
        plan: &Plan,
        test_scope: Arc<Mutex<TestScope>>,
        code_server: Arc<CodeServer>,
        aliases: &mut HashMap<String, SharedVm>,
    ) -> Outcome {
        let mut local_shells: HashMap<String, SharedVm> = HashMap::new();
        for block in &plan.test.shells {
            let shell_name = block.node.name.node.clone();
            let vm = if let Some(vm) = aliases.get(&shell_name).cloned() {
                vm
            } else if let Some(vm) = local_shells.get(&shell_name).cloned() {
                vm
            } else {
                let vars = VariableStack::new(test_scope.clone(), HashMap::new(), self.env.clone());
                let vm = match Vm::new(
                    shell_name.clone(),
                    vars,
                    self.default_timeout,
                    code_server.clone(),
                )
                .await
                {
                    Ok(vm) => Arc::new(Mutex::new(vm)),
                    Err(f) => return Outcome::Fail(f),
                };
                local_shells.insert(shell_name.clone(), vm.clone());
                vm
            };

            let mut guard = vm.lock().await;
            guard.reset_for_reuse(self.default_timeout).await;
            if let Err(f) = guard.exec_stmts(&block.node.stmts).await {
                return Outcome::Fail(f);
            }
        }
        Outcome::Pass
    }

    async fn run_test_cleanup(&self, plan: &Plan, aliases: &HashMap<String, SharedVm>) {
        if let Some(cleanup) = &plan.test.cleanup {
            let test_scope = Arc::new(Mutex::new(TestScope::new()));
            let vars = VariableStack::new(test_scope, HashMap::new(), self.env.clone());
            let code_server = Arc::new(CodeServer::new(plan.functions.clone()));
            if let Ok(mut vm) = Vm::new(
                "__test_cleanup".to_string(),
                vars,
                self.default_timeout,
                code_server,
            )
            .await
            {
                let converted = cleanup_to_shell_stmts(&cleanup.node.stmts, &cleanup.span);
                let _ = vm.exec_stmts(&converted).await;
                vm.shutdown().await;
            }
        }

        for vm in aliases.values() {
            vm.lock().await.reset_for_reuse(self.default_timeout).await;
        }
    }

    async fn teardown_effects(&self, plan: &Plan, state: &mut EffectExecution) {
        let mut order = match toposort(&plan.effect_graph.dag, None) {
            Ok(v) => v,
            Err(_) => return,
        };
        order.reverse();

        for id in order {
            let Some(instance_state) = state.instances.remove(&id) else {
                continue;
            };
            let effect = &plan.effects[instance_state.info.effect];
            if let Some(cleanup) = &effect.cleanup {
                let test_scope = Arc::new(Mutex::new(TestScope::new()));
                let overlay = self.interpolate_overlay(&instance_state.info.overlay);
                let vars = VariableStack::new(test_scope, overlay, self.env.clone());
                let code_server = Arc::new(CodeServer::new(Vec::new()));
                if let Ok(mut vm) = Vm::new(
                    "__effect_cleanup".to_string(),
                    vars,
                    self.default_timeout,
                    code_server,
                )
                .await
                {
                    let converted = cleanup_to_shell_stmts(&cleanup.node.stmts, &cleanup.span);
                    let _ = vm.exec_stmts(&converted).await;
                    vm.shutdown().await;
                }
            }
            for (_, vm) in instance_state.all_shells {
                if let Ok(mutex) = Arc::try_unwrap(vm) {
                    mutex.into_inner().shutdown().await;
                }
            }
        }
    }

    fn interpolate_overlay(&self, overlay: &[ir::OverlayEntry]) -> HashMap<String, String> {
        overlay
            .iter()
            .map(|entry| {
                let v =
                    interpolate_with_lookup(&entry.value.node, |name| self.env.get(name).cloned());
                (entry.key.node.clone(), v)
            })
            .collect()
    }

    async fn eval_static_expr(
        &self,
        expr: &Spanned<ir::Expr>,
        vars: &VariableStack,
    ) -> Result<String, Failure> {
        match &expr.node {
            ir::Expr::String(s) => Ok(crate::runtime::vars::interpolate(s, vars).await),
            ir::Expr::Var(name) => Ok(vars.lookup(name).await.unwrap_or_default()),
            _ => Err(Failure::Runtime {
                message: "unsupported expression in static context".to_string(),
                span: Some(expr.span.clone()),
                shell: None,
            }),
        }
    }
}

#[derive(Default)]
struct EffectExecution {
    instances: HashMap<InstanceId, EffectInstanceState>,
    alias_shells: HashMap<String, SharedVm>,
    failures: Vec<Failure>,
    outcome: Option<Outcome>,
}

struct EffectInstanceState {
    info: EffectInstance,
    all_shells: HashMap<String, SharedVm>,
    exported_vm: SharedVm,
}

fn cleanup_to_shell_stmts(
    stmts: &[Spanned<ir::CleanupStmt>],
    span: &ir::Span,
) -> Vec<Spanned<ir::ShellStmt>> {
    stmts
        .iter()
        .map(|stmt| {
            let node = match &stmt.node {
                ir::CleanupStmt::Send(s) => ir::ShellStmt::Expr(ir::Expr::Send(s.clone())),
                ir::CleanupStmt::SendRaw(s) => ir::ShellStmt::Expr(ir::Expr::SendRaw(s.clone())),
                ir::CleanupStmt::Let(v) => ir::ShellStmt::Let(ir::VarDecl {
                    name: v.name.clone(),
                    value: v
                        .value
                        .as_ref()
                        .map(|e| Spanned::new(e.node.clone(), e.span.clone())),
                }),
                ir::CleanupStmt::Assign(a) => ir::ShellStmt::Assign(ir::VarAssign {
                    name: a.name.clone(),
                    value: Spanned::new(a.value.node.clone(), a.value.span.clone()),
                }),
            };
            Spanned::new(node, stmt.span.clone())
        })
        .chain(std::iter::once(Spanned::new(
            ir::ShellStmt::Timeout(Duration::from_secs(10)),
            span.clone(),
        )))
        .collect()
}
