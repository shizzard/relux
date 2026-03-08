use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use daggy::petgraph::algo::toposort;
use daggy::petgraph::visit::{EdgeRef, IntoEdgesDirected};
use tokio::sync::Mutex;

use crate::dsl::resolver::ir::{self, EffectInstance, InstanceId, Plan, SourceMap, Spanned};
use crate::runtime::event_log::{EventCollector, LogEventKind};
use crate::runtime::progress::{ProgressEvent, ProgressTx};
use crate::runtime::result::{Failure, Outcome, TestResult};
use crate::runtime::vars::{Env, ScopeStack, TestScope, interpolate_with_lookup};
use crate::runtime::vm::Vm;

pub mod bifs;
pub mod event_log;
pub mod html;
pub mod progress;
pub mod result;
pub mod shell_log;
pub mod vars;
pub mod vm;

use crate::config;

pub type SharedVm = Arc<Mutex<Vm>>;

pub enum Callable {
    UserDefined(usize),
    Builtin(Box<dyn bifs::Bif>),
}

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

    pub fn lookup(&self, name: &str, arity: usize) -> Option<Callable> {
        if let Some(&id) = self.index.get(&(name.to_string(), arity)) {
            Some(Callable::UserDefined(id))
        } else {
            bifs::lookup(name, arity).map(Callable::Builtin)
        }
    }

    pub fn get(&self, fn_id: usize) -> Option<&ir::Function> {
        self.functions.get(fn_id)
    }
}

fn evaluate_conditions(
    conditions: &[Spanned<ir::Condition>],
    env: &Env,
) -> Option<String> {
    for cond in conditions {
        let raw = env.get(&cond.node.var).cloned().unwrap_or_default();

        let result_value = match &cond.node.test {
            None => raw.clone(),
            Some(ir::CondTest::Eq(expected)) => {
                if raw == *expected {
                    raw.clone()
                } else {
                    String::new()
                }
            }
            Some(ir::CondTest::Regex(pat)) => match regex::Regex::new(pat) {
                Ok(re) => re
                    .find(&raw)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                Err(_) => String::new(),
            },
        };

        let truthy = !result_value.is_empty();

        let should_act = match cond.node.modifier {
            ir::CondModifier::If => truthy,
            ir::CondModifier::Unless => !truthy,
        };

        match cond.node.kind {
            ir::CondKind::Skip => {
                if should_act {
                    let reason = if raw.is_empty() {
                        format!("skip: {} is not set", cond.node.var)
                    } else {
                        format!("skip: {} = {:?}", cond.node.var, raw)
                    };
                    return Some(reason);
                }
            }
            ir::CondKind::Run => {
                if !should_act {
                    let reason = if raw.is_empty() {
                        format!("run condition not met: {} is not set", cond.node.var)
                    } else {
                        format!("run condition not met: {} = {:?}", cond.node.var, raw)
                    };
                    return Some(reason);
                }
            }
            ir::CondKind::Flaky => {
                if should_act {
                    let reason = if raw.is_empty() {
                        format!("flaky: {} is not set", cond.node.var)
                    } else {
                        format!("flaky: {} = {:?}", cond.node.var, raw)
                    };
                    return Some(reason);
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStrategy {
    All,
    FailFast,
}

pub struct Runtime {
    env: Arc<Env>,
    source_map: SourceMap,
    run_dir: PathBuf,
    project_root: PathBuf,
    default_timeout: Duration,
    shell_command: String,
    shell_prompt: String,
    case_timeout: Option<Duration>,
    suite_timeout: Option<Duration>,
    strategy: RunStrategy,
}

pub struct RunContext {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub project_root: PathBuf,
    pub shell_command: String,
    pub shell_prompt: String,
    pub default_timeout: Duration,
    pub case_timeout: Option<Duration>,
    pub suite_timeout: Option<Duration>,
    pub strategy: RunStrategy,
}

impl Runtime {
    pub fn new(source_map: SourceMap, run_context: RunContext) -> Self {
        let mut env = std::env::vars().collect::<HashMap<_, _>>();
        env.insert("__RELUX_RUN_ID".to_string(), run_context.run_id.clone());
        env.insert(
            "__RELUX_RUN_ARTIFACTS".to_string(),
            run_context.artifacts_dir.display().to_string(),
        );
        env.insert(
            "__RELUX_SHELL_PROMPT".to_string(),
            run_context.shell_prompt.clone(),
        );
        Self {
            env: Arc::new(env),
            source_map,
            run_dir: run_context.run_dir,
            project_root: run_context.project_root,
            default_timeout: run_context.default_timeout,
            shell_command: run_context.shell_command,
            shell_prompt: run_context.shell_prompt,
            case_timeout: run_context.case_timeout,
            suite_timeout: run_context.suite_timeout,
            strategy: run_context.strategy,
        }
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    pub async fn run(&self, plans: Vec<Plan>) -> Vec<TestResult> {
        let run_fut = self.run_inner(plans);
        match self.suite_timeout {
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

    async fn run_inner(&self, plans: Vec<Plan>) -> Vec<TestResult> {
        let mut results = Vec::with_capacity(plans.len());
        for plan in plans {
            let result = self.run_plan(plan).await;
            let failed = matches!(result.outcome, Outcome::Fail(_));
            results.push(result);
            if failed && self.strategy == RunStrategy::FailFast {
                break;
            }
        }
        results
    }

    async fn run_plan(&self, plan: Plan) -> TestResult {
        match self.case_timeout {
            Some(timeout) => match tokio::time::timeout(timeout, self.run_plan_inner(plan)).await {
                Ok(result) => result,
                Err(_) => TestResult {
                    test_name: "(unknown)".to_string(),
                    outcome: Outcome::Fail(Failure::Runtime {
                        message: format!("case timeout ({timeout:?}) exceeded"),
                        span: None,
                        shell: None,
                    }),
                    duration: Duration::ZERO,
                    shell_logs: HashMap::new(),
                    progress: String::new(),
                    log_dir: None,
                },
            },
            None => self.run_plan_inner(plan).await,
        }
    }

    async fn run_plan_inner(&self, plan: Plan) -> TestResult {
        let test_start = Instant::now();
        let test_name = plan.test.name.node.clone();
        let code_server = Arc::new(CodeServer::new(plan.functions.clone()));
        let mut shell_logs = HashMap::new();

        let log_dir = self.compute_test_log_dir(&plan);
        let _ = std::fs::create_dir_all(&log_dir);
        let event_collector = EventCollector::new(test_start);

        eprint!("test \"{test_name}\": ");
        let (progress_tx, progress_rx) = progress::channel();
        let printer_handle = progress::spawn_printer(progress_rx);

        if let Some(reason) =
            evaluate_conditions(&plan.test.conditions, &self.env)
        {
            drop(progress_tx);
            let progress_string = printer_handle.await.unwrap_or_default();
            let events = event_collector.take().await;
            crate::runtime::html::generate_html_logs(
                &log_dir,
                &test_name,
                &events,
                &self.run_dir,
            );
            return TestResult {
                test_name,
                outcome: Outcome::Skipped(reason),
                duration: test_start.elapsed(),
                shell_logs,
                progress: progress_string,
                log_dir: Some(log_dir),
            };
        }

        let mut effect_exec = self
            .execute_effects(
                &plan,
                code_server.clone(),
                &mut shell_logs,
                progress_tx.clone(),
                &log_dir,
                test_start,
                &event_collector,
            )
            .await;

        let outcome = match effect_exec.outcome.take() {
            Some(outcome) => outcome,
            None => {
                let test_scope = Arc::new(Mutex::new(TestScope::new()));
                for decl in &plan.test.vars {
                    let value = if let Some(expr) = &decl.node.value {
                        let vars = ScopeStack::new(
                            test_scope.clone(),
                            HashMap::new(),
                            self.env.clone(),
                            self.default_timeout,
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
                    progress_tx.clone(),
                    &log_dir,
                    test_start,
                    &event_collector,
                )
                .await
            }
        };

        self.run_test_cleanup(
            &plan,
            &effect_exec.alias_shells,
            progress_tx.clone(),
            &log_dir,
            test_start,
            &event_collector,
        )
        .await;
        // Drop alias references before teardown so Arc::try_unwrap
        // can succeed when shutting down effect shells.
        effect_exec.alias_shells.clear();
        self.teardown_effects(
            &plan,
            &mut effect_exec,
            progress_tx.clone(),
            &log_dir,
            test_start,
            &event_collector,
        )
        .await;

        drop(progress_tx);
        let progress_string = printer_handle.await.unwrap_or_default();

        for (name, vm) in &effect_exec.alias_shells {
            let out = vm.lock().await.output_snapshot().await;
            shell_logs.insert(name.clone(), out);
        }

        let events = event_collector.take().await;
        crate::runtime::html::generate_html_logs(&log_dir, &test_name, &events, &self.run_dir);

        TestResult {
            test_name,
            outcome,
            duration: test_start.elapsed(),
            shell_logs,
            progress: progress_string,
            log_dir: Some(log_dir),
        }
    }

    fn compute_test_log_dir(&self, plan: &Plan) -> PathBuf {
        let source_path = &self.source_map.files[plan.test.span.file].path;
        let relative = source_path
            .strip_prefix(&self.project_root)
            .unwrap_or(source_path);
        self.run_dir
            .join("logs")
            .join(relative.with_extension(""))
            .join(slugify(&plan.test.name.node))
    }

    async fn execute_effects(
        &self,
        plan: &Plan,
        code_server: Arc<CodeServer>,
        shell_logs: &mut HashMap<String, Vec<u8>>,
        progress_tx: ProgressTx,
        log_dir: &Path,
        test_start: Instant,
        event_collector: &EventCollector,
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

        let scope_prefixes = compute_scope_prefixes(plan);

        for instance_id in order {
            let Some(instance) = plan.effect_graph.dag.node_weight(instance_id).cloned() else {
                continue;
            };
            let overlay = self.interpolate_overlay(&instance.overlay);
            let effect = &plan.effects[instance.effect];
            let _ = progress_tx.send(ProgressEvent::EffectSetup(effect.name.node.clone()));
            event_collector.push("", LogEventKind::EffectSetup { effect: effect.name.node.clone() }).await;

            if let Some(reason) =
                evaluate_conditions(&effect.conditions, &self.env)
            {
                let reason = format!("effect {} skipped: {reason}", effect.name.node);
                effect_state.outcome = Some(Outcome::Skipped(reason));
                return effect_state;
            }

            let effect_scope = Arc::new(Mutex::new(TestScope::new()));
            let mut shells: HashMap<String, SharedVm> = HashMap::new();
            let scope_prefix = scope_prefixes
                .get(&instance_id)
                .cloned()
                .unwrap_or_else(|| effect.name.node.clone());

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
                        ScopeStack::new(effect_scope.clone(), overlay.clone(), self.env.clone(), self.default_timeout);
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
                    let scoped_name = format!("{scope_prefix}.{shell_name}");
                    if !shells.contains_key(&shell_name) {
                        let scope = ScopeStack::new(
                            effect_scope.clone(),
                            overlay.clone(),
                            self.env.clone(),
                            self.default_timeout,
                        );
                        match Vm::new(
                            scoped_name,
                            self.shell_prompt.clone(),
                            self.shell_command.clone(),
                            scope,
                            code_server.clone(),
                            Some(progress_tx.clone()),
                            log_dir,
                            test_start,
                            Some(event_collector.clone()),
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
                    scope_prefix,
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
        progress_tx: ProgressTx,
        log_dir: &Path,
        test_start: Instant,
        event_collector: &EventCollector,
    ) -> Outcome {
        let mut local_shells: HashMap<String, SharedVm> = HashMap::new();
        for block in &plan.test.shells {
            let shell_name = block.node.name.node.clone();
            let _ = progress_tx.send(ProgressEvent::ShellSwitch(shell_name.clone()));
            event_collector.push("", LogEventKind::ShellSwitch { name: shell_name.clone() }).await;
            let vm = if let Some(vm) = aliases.get(&shell_name).cloned() {
                vm
            } else if let Some(vm) = local_shells.get(&shell_name).cloned() {
                vm
            } else {
                let scope = ScopeStack::new(test_scope.clone(), HashMap::new(), self.env.clone(), self.default_timeout);
                let vm = match Vm::new(
                    shell_name.clone(),
                    self.shell_prompt.clone(),
                    self.shell_command.clone(),
                    scope,
                    code_server.clone(),
                    Some(progress_tx.clone()),
                    log_dir,
                    test_start,
                    Some(event_collector.clone()),
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

    async fn run_test_cleanup(
        &self,
        plan: &Plan,
        aliases: &HashMap<String, SharedVm>,
        progress_tx: ProgressTx,
        log_dir: &Path,
        test_start: Instant,
        event_collector: &EventCollector,
    ) {
        if let Some(cleanup) = &plan.test.cleanup {
            let _ = progress_tx.send(ProgressEvent::Cleanup);
            let test_scope = Arc::new(Mutex::new(TestScope::new()));
            let scope = ScopeStack::new(test_scope, HashMap::new(), self.env.clone(), self.default_timeout);
            let code_server = Arc::new(CodeServer::new(plan.functions.clone()));
            event_collector.push("", LogEventKind::Cleanup { shell: "__cleanup".to_string() }).await;
            if let Ok(mut vm) = Vm::new(
                "__cleanup".to_string(),
                self.shell_prompt.clone(),
                self.shell_command.clone(),
                scope,
                code_server,
                Some(progress_tx),
                log_dir,
                test_start,
                Some(event_collector.clone()),
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

    async fn teardown_effects(
        &self,
        plan: &Plan,
        state: &mut EffectExecution,
        progress_tx: ProgressTx,
        log_dir: &Path,
        test_start: Instant,
        event_collector: &EventCollector,
    ) {
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
                let _ = progress_tx.send(ProgressEvent::Cleanup);
                let cleanup_name = format!("{}.__cleanup", instance_state.scope_prefix);
                event_collector.push("", LogEventKind::EffectTeardown { effect: effect.name.node.clone() }).await;
                event_collector.push("", LogEventKind::Cleanup { shell: cleanup_name.clone() }).await;
                let test_scope = Arc::new(Mutex::new(TestScope::new()));
                let overlay = self.interpolate_overlay(&instance_state.info.overlay);
                let scope = ScopeStack::new(test_scope, overlay, self.env.clone(), self.default_timeout);
                let code_server = Arc::new(CodeServer::new(Vec::new()));
                if let Ok(mut vm) = Vm::new(
                    cleanup_name,
                    self.shell_prompt.clone(),
                    self.shell_command.clone(),
                    scope,
                    code_server,
                    Some(progress_tx.clone()),
                    log_dir,
                    test_start,
                    Some(event_collector.clone()),
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
        scope: &ScopeStack,
    ) -> Result<String, Failure> {
        match &expr.node {
            ir::Expr::String(s) => Ok(crate::runtime::vars::interpolate(s, scope).await),
            ir::Expr::Var(name) => Ok(scope.lookup(name).await.unwrap_or_default()),
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
    scope_prefix: String,
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
            ir::ShellStmt::Timeout(config::DEFAULT_TIMEOUT),
            span.clone(),
        )))
        .collect()
}

fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Builds a map from InstanceId to a hierarchical scope prefix string.
/// Top-level effects (referenced by test needs) get `{effect_name}.{alias}`.
/// Nested dependencies get `{parent_prefix}.{effect_name}.{dep_alias}`.
fn compute_scope_prefixes(plan: &Plan) -> HashMap<InstanceId, String> {
    let mut prefixes: HashMap<InstanceId, String> = HashMap::new();

    for need in &plan.test.needs {
        let instance_id = need.node.instance;
        if let Some(instance) = plan.effect_graph.dag.node_weight(instance_id) {
            let effect = &plan.effects[instance.effect];
            let prefix = format!("{}.{}", effect.name.node, need.node.alias.node);
            prefixes.insert(instance_id, prefix);
        }
    }

    if let Ok(order) = toposort(&plan.effect_graph.dag, None) {
        let reversed: Vec<_> = order.into_iter().rev().collect();
        for id in reversed {
            if prefixes.contains_key(&id) {
                continue;
            }
            if let Some(instance) = plan.effect_graph.dag.node_weight(id) {
                let effect = &plan.effects[instance.effect];
                let mut parent_prefix = None;
                for edge in plan
                    .effect_graph
                    .dag
                    .edges_directed(id, daggy::petgraph::Direction::Outgoing)
                {
                    let target = edge.target();
                    if let Some(p) = prefixes.get(&target) {
                        parent_prefix =
                            Some(format!("{}.{}.{}", p, effect.name.node, edge.weight().alias.node));
                        break;
                    }
                }
                let prefix =
                    parent_prefix.unwrap_or_else(|| format!("{}.{}", effect.name.node, id.index()));
                prefixes.insert(id, prefix);
            }
        }
    }

    prefixes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::resolver::ir::{self, Span};

    fn make_cond(
        kind: ir::CondKind,
        modifier: ir::CondModifier,
        var: &str,
        test: Option<ir::CondTest>,
    ) -> Spanned<ir::Condition> {
        Spanned::new(
            ir::Condition {
                kind,
                modifier,
                var: var.to_string(),
                test,
            },
            Span::new(0, 0..0),
        )
    }

    #[test]
    fn skip_unless_unset_var() {
        let conds = vec![make_cond(
            ir::CondKind::Skip,
            ir::CondModifier::Unless,
            "MISSING",
            None,
        )];
        let env = Env::new();
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
        assert!(result.unwrap().contains("skip"));
    }

    #[test]
    fn skip_unless_set_var() {
        let conds = vec![make_cond(
            ir::CondKind::Skip,
            ir::CondModifier::Unless,
            "CI",
            None,
        )];
        let mut env = Env::new();
        env.insert("CI".into(), "true".into());
        assert!(evaluate_conditions(&conds, &env).is_none());
    }

    #[test]
    fn skip_if_set_var() {
        let conds = vec![make_cond(
            ir::CondKind::Skip,
            ir::CondModifier::If,
            "CI",
            None,
        )];
        let mut env = Env::new();
        env.insert("CI".into(), "1".into());
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
        assert!(result.unwrap().contains("skip"));
    }

    #[test]
    fn run_if_matching_literal() {
        let conds = vec![make_cond(
            ir::CondKind::Run,
            ir::CondModifier::If,
            "OS",
            Some(ir::CondTest::Eq("linux".into())),
        )];
        let mut env = Env::new();
        env.insert("OS".into(), "linux".into());
        assert!(evaluate_conditions(&conds, &env).is_none());
    }

    #[test]
    fn run_if_not_matching_literal() {
        let conds = vec![make_cond(
            ir::CondKind::Run,
            ir::CondModifier::If,
            "OS",
            Some(ir::CondTest::Eq("linux".into())),
        )];
        let mut env = Env::new();
        env.insert("OS".into(), "macos".into());
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
        assert!(result.unwrap().contains("run condition not met"));
    }

    #[test]
    fn skip_unless_regex_match() {
        let conds = vec![make_cond(
            ir::CondKind::Skip,
            ir::CondModifier::Unless,
            "ARCH",
            Some(ir::CondTest::Regex("^(x86_64|aarch64)$".into())),
        )];
        let mut env = Env::new();
        env.insert("ARCH".into(), "x86_64".into());
        assert!(evaluate_conditions(&conds, &env).is_none());
    }

    #[test]
    fn skip_unless_regex_no_match() {
        let conds = vec![make_cond(
            ir::CondKind::Skip,
            ir::CondModifier::Unless,
            "ARCH",
            Some(ir::CondTest::Regex("^(x86_64|aarch64)$".into())),
        )];
        let mut env = Env::new();
        env.insert("ARCH".into(), "riscv".into());
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
    }

    #[test]
    fn multiple_conditions_all_pass() {
        let conds = vec![
            make_cond(ir::CondKind::Skip, ir::CondModifier::Unless, "CI", None),
            make_cond(
                ir::CondKind::Run,
                ir::CondModifier::If,
                "OS",
                Some(ir::CondTest::Eq("linux".into())),
            ),
        ];
        let mut env = Env::new();
        env.insert("CI".into(), "1".into());
        env.insert("OS".into(), "linux".into());
        assert!(evaluate_conditions(&conds, &env).is_none());
    }

    #[test]
    fn multiple_conditions_second_fails() {
        let conds = vec![
            make_cond(ir::CondKind::Skip, ir::CondModifier::Unless, "CI", None),
            make_cond(
                ir::CondKind::Run,
                ir::CondModifier::If,
                "OS",
                Some(ir::CondTest::Eq("linux".into())),
            ),
        ];
        let mut env = Env::new();
        env.insert("CI".into(), "1".into());
        env.insert("OS".into(), "macos".into());
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
    }

    #[test]
    fn flaky_if_set() {
        let conds = vec![make_cond(
            ir::CondKind::Flaky,
            ir::CondModifier::If,
            "CI",
            None,
        )];
        let mut env = Env::new();
        env.insert("CI".into(), "1".into());
        let result = evaluate_conditions(&conds, &env);
        assert!(result.is_some());
        assert!(result.unwrap().contains("flaky"));
    }

    #[test]
    fn empty_conditions_pass() {
        let conds: Vec<Spanned<ir::Condition>> = vec![];
        let env = Env::new();
        assert!(evaluate_conditions(&conds, &env).is_none());
    }
}
