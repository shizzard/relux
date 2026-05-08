use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tokio::sync::Mutex;

use relux_core::pure::LayeredEnv;
use relux_core::pure::VarScope;
use relux_ir::IrTimeout;

use crate::observe::structured::SpanId;

// ─── FailPattern ────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum FailPattern {
    Regex(Regex),
    Literal(String),
}

// ─── Captures ───────────────────────────────────────────────

/// Regex capture storage. Indexed captures are stored as "0", "1", etc.
/// Named captures are stored by their group name.
#[derive(Debug, Default, Clone)]
pub struct Captures {
    map: HashMap<String, String>,
}

impl Captures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_indexed(&self, index: usize) -> Option<&str> {
        self.map.get(&index.to_string()).map(String::as_str)
    }

    pub fn get_named(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(String::as_str)
    }

    /// Look up by key (either numeric string for indexed, or name for named).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: String, value: String) {
        self.map.insert(key, value);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

// ─── Scope ──────────────────────────────────────────────────

#[derive(Clone)]
pub enum Scope {
    Test {
        name: String,
        vars: Arc<Mutex<VarScope>>,
        timeout: Option<IrTimeout>,
    },
    Effect {
        name: String,
        vars: Arc<Mutex<VarScope>>,
        _timeout: Option<IrTimeout>,
        env: Arc<LayeredEnv>,
    },
}

impl Scope {
    pub fn name(&self) -> &str {
        match self {
            Scope::Test { name, .. } | Scope::Effect { name, .. } => name,
        }
    }

    pub fn vars(&self) -> &Arc<Mutex<VarScope>> {
        match self {
            Scope::Test { vars, .. } | Scope::Effect { vars, .. } => vars,
        }
    }
}

// ─── ShellState ─────────────────────────────────────────────

pub struct ShellState {
    /// Local name of the shell within its current owning scope. At spawn
    /// time this is the shell's declaration name; each `reset_for_export`
    /// rewrites it to the key under which the source effect exposed the
    /// shell to the parent.
    pub name: String,

    /// User-supplied alias from the parent caller's `start <Effect> as
    /// <Alias>`. `None` when the caller did not alias, or when the shell
    /// is in its own (origin) scope and no parent has imported it yet.
    pub effect_alias: Option<String>,

    /// Original effect-type name of the parent effect that owns this
    /// shell from the current scope's POV. `None` symmetrically with
    /// `effect_alias` (no parent → no name).
    pub effect_name: Option<String>,

    pub vars: VarScope,
    pub captures: Captures,
    pub timeout: Option<IrTimeout>,
    pub fail_pattern: Option<FailPattern>,
}

impl ShellState {
    pub fn new(name: String) -> Self {
        Self {
            name,
            effect_alias: None,
            effect_name: None,
            vars: VarScope::new(),
            captures: Captures::new(),
            timeout: None,
            fail_pattern: None,
        }
    }
}

// ─── CallFrame ──────────────────────────────────────────────

pub struct CallFrame {
    pub name: String,
    pub vars: VarScope,
    pub captures: Captures,
    pub timeout: Option<IrTimeout>,
    pub fail_pattern: Option<FailPattern>,
}

// ─── ExecutionContext ────────────────────────────────────────

pub struct ExecutionContext {
    pub scope: Scope,
    pub shell: ShellState,
    call_stack: Vec<CallFrame>,
    span_stack: Vec<SpanId>,
    pub default_timeout: IrTimeout,
    pub env: Arc<LayeredEnv>,
}

impl ExecutionContext {
    pub fn new(
        scope: Scope,
        shell: ShellState,
        default_timeout: IrTimeout,
        env: Arc<LayeredEnv>,
        parent_span: SpanId,
    ) -> Self {
        Self {
            scope,
            shell,
            call_stack: Vec::new(),
            span_stack: vec![parent_span],
            default_timeout,
            env,
        }
    }

    /// The id of the innermost span currently active in this context. Used
    /// by every emission site so events reference the span they fired in.
    pub fn current_span(&self) -> SpanId {
        *self
            .span_stack
            .last()
            .expect("span_stack always has at least one entry")
    }

    pub fn push_span(&mut self, id: SpanId) {
        self.span_stack.push(id);
    }

    pub fn pop_span(&mut self) {
        // Never pop the bottom of the stack (the root passed to `new`).
        if self.span_stack.len() > 1 {
            self.span_stack.pop();
        }
    }

    /// Reset the span stack so emissions are parented on `span`. Used when a
    /// shell is reused across shell blocks: each block opens a fresh
    /// ShellBlock span and the VM's events should reference that one rather
    /// than the span from the shell's original construction.
    pub fn set_block_span(&mut self, span: SpanId) {
        self.span_stack = vec![span];
    }

    /// Look up a variable by name. Follows the lookup chain per RFC R005.
    pub async fn lookup(&self, key: &str) -> Option<String> {
        if let Some(frame) = self.call_stack.last() {
            // Inside a function call — hard barrier
            if let Some(v) = frame.vars.get(key) {
                return Some(v.to_string());
            }
            return self.env.get(key).map(str::to_string);
        }

        // Direct shell execution
        if let Some(v) = self.shell.vars.get(key) {
            return Some(v.to_string());
        }
        if let Some(v) = self.scope.vars().lock().await.get(key) {
            return Some(v.to_string());
        }
        // Effect scope env walks the layered chain (overlays → base)
        if let Scope::Effect { env, .. } = &self.scope
            && let Some(v) = env.get(key)
        {
            return Some(v.to_string());
        }
        self.env.get(key).map(str::to_string)
    }

    /// Look up a capture reference (e.g. ${1}).
    pub fn capture(&self, index: usize) -> Option<String> {
        let key = index.to_string();
        if let Some(frame) = self.call_stack.last() {
            return frame.captures.get(&key).map(str::to_string);
        }
        self.shell.captures.get(&key).map(str::to_string)
    }

    /// Insert a `let` variable into the current context.
    pub fn let_insert(&mut self, key: String, value: String) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.vars.insert(key, value);
        } else {
            self.shell.vars.insert(key, value);
        }
    }

    /// Assign to an existing variable. Returns true if found and updated.
    pub async fn assign(&mut self, key: &str, value: String) -> bool {
        if let Some(frame) = self.call_stack.last_mut() {
            return frame.vars.assign(key, value);
        }
        if self.shell.vars.assign(key, value.clone()) {
            return true;
        }
        self.scope.vars().lock().await.assign(key, value)
    }

    /// Push a function call frame.
    pub fn push_call(&mut self, name: String, args: Vec<(String, String)>) {
        let (timeout, fail_pattern) = if let Some(frame) = self.call_stack.last() {
            (frame.timeout.clone(), frame.fail_pattern.clone())
        } else {
            (self.shell.timeout.clone(), self.shell.fail_pattern.clone())
        };
        let mut vars = VarScope::new();
        for (k, v) in args {
            vars.insert(k, v);
        }
        self.call_stack.push(CallFrame {
            name,
            vars,
            captures: Captures::new(),
            timeout,
            fail_pattern,
        });
    }

    /// Pop the top function call frame.
    pub fn pop_call(&mut self) {
        self.call_stack.pop();
    }

    /// Get the effective timeout.
    pub fn timeout(&self) -> &IrTimeout {
        if let Some(frame) = self.call_stack.last()
            && let Some(ref t) = frame.timeout
        {
            return t;
        }
        if let Some(ref t) = self.shell.timeout {
            return t;
        }
        &self.default_timeout
    }

    /// Set the timeout on the current context.
    pub fn set_timeout(&mut self, t: IrTimeout) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.timeout = Some(t);
        } else {
            self.shell.timeout = Some(t);
        }
    }

    /// Get the current fail pattern.
    pub fn fail_pattern(&self) -> Option<&FailPattern> {
        if let Some(frame) = self.call_stack.last() {
            return frame.fail_pattern.as_ref();
        }
        self.shell.fail_pattern.as_ref()
    }

    /// Set the fail pattern on the current context.
    pub fn set_fail_pattern(&mut self, pattern: Option<FailPattern>) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.fail_pattern = pattern;
        } else {
            self.shell.fail_pattern = pattern;
        }
    }

    /// Current display name for logging. Reflects the *current scope's*
    /// view of the shell only — no chain accumulation across exports.
    ///
    /// Format:
    /// - `<name>` when no parent effect imported this shell (origin scope or
    ///   directly at test scope without aliasing through an effect).
    /// - `<Effect>.<name>` when the parent imported the source effect without
    ///   aliasing it.
    /// - `<Alias>(<Effect>).<name>` when the parent used `start Effect as Alias`.
    pub fn current_name(&self) -> String {
        match (&self.shell.effect_name, &self.shell.effect_alias) {
            (None, _) => self.shell.name.clone(),
            (Some(eff), None) => format!("{eff}.{}", self.shell.name),
            (Some(eff), Some(ali)) => format!("{ali}({eff}).{}", self.shell.name),
        }
    }

    /// Reset for shell export (effect → test/parent effect). Replaces the
    /// shell's view with how the new (parent) scope sees it: the parent's
    /// alias for the source effect, the source effect's original name, and
    /// the local key under which it was exposed.
    pub fn reset_for_export(
        &mut self,
        new_scope: Scope,
        parent_alias: Option<String>,
        parent_effect_name: Option<String>,
        shell_local_name: String,
    ) {
        self.shell.effect_alias = parent_alias;
        self.shell.effect_name = parent_effect_name;
        self.shell.name = shell_local_name;
        self.scope = new_scope;
        self.shell.vars = VarScope::new();
        self.shell.captures = Captures::new();
        // timeout, fail_pattern are preserved
    }

    /// Set captures on the current context.
    pub fn set_captures(&mut self, captures: Captures) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.captures = captures;
        } else {
            self.shell.captures = captures;
        }
    }

    /// Whether we're inside a function call.
    pub fn in_call(&self) -> bool {
        !self.call_stack.is_empty()
    }

    /// Snapshot user-visible variables at the current point of execution,
    /// for failure diagnostics. Excludes the layered process env (already in
    /// `events.json :: env.bootstrap`) and matcher captures. Sorted by key
    /// for stable JSON output.
    pub async fn snapshot_user_vars(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        if let Some(frame) = self.call_stack.last() {
            for (k, v) in frame.vars.iter() {
                out.push((k.to_string(), v.to_string()));
            }
        } else {
            for (k, v) in self.shell.vars.iter() {
                out.push((k.to_string(), v.to_string()));
            }
            let scope_vars = self.scope.vars().lock().await;
            for (k, v) in scope_vars.iter() {
                out.push((k.to_string(), v.to_string()));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        out
    }

    /// Build the environment variables map for spawning a shell process.
    /// For effects, the effect env already inherits the base env via LayeredEnv
    /// parent chain, so only the effect env is needed.
    pub fn process_env(&self) -> Vec<(String, String)> {
        let result: Vec<(String, String)> = match &self.scope {
            Scope::Effect { env, .. } => env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            Scope::Test { .. } => self
                .env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relux_core::pure::Env;
    use std::collections::HashMap;
    use std::time::Duration;

    fn test_env() -> Arc<LayeredEnv> {
        let mut m = HashMap::new();
        m.insert("PATH".into(), "/usr/bin".into());
        Arc::new(LayeredEnv::from(Env::from_map(m)))
    }

    fn test_scope(name: &str) -> Scope {
        Scope::Test {
            name: name.into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            timeout: None,
        }
    }

    fn test_shell(name: &str) -> ShellState {
        ShellState::new(name.into())
    }

    fn test_ctx() -> ExecutionContext {
        ExecutionContext::new(
            test_scope("my test"),
            test_shell("sh"),
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
            0,
        )
    }

    // ─── Lookup tests ────────────────────────────────────────

    #[tokio::test]
    async fn lookup_shell_var() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("x".into(), "hello".into());
        assert_eq!(ctx.lookup("x").await, Some("hello".into()));
    }

    #[tokio::test]
    async fn lookup_scope_var() {
        let ctx = test_ctx();
        ctx.scope
            .vars()
            .lock()
            .await
            .insert("g".into(), "global".into());
        assert_eq!(ctx.lookup("g").await, Some("global".into()));
    }

    #[tokio::test]
    async fn lookup_env_fallback() {
        let ctx = test_ctx();
        assert_eq!(ctx.lookup("PATH").await, Some("/usr/bin".into()));
    }

    #[tokio::test]
    async fn lookup_missing() {
        let ctx = test_ctx();
        assert_eq!(ctx.lookup("NONEXISTENT").await, None);
    }

    #[tokio::test]
    async fn lookup_shell_shadows_scope() {
        let mut ctx = test_ctx();
        ctx.scope
            .vars()
            .lock()
            .await
            .insert("x".into(), "scope".into());
        ctx.shell.vars.insert("x".into(), "shell".into());
        assert_eq!(ctx.lookup("x").await, Some("shell".into()));
    }

    // ─── Call stack barrier ──────────────────────────────────

    #[tokio::test]
    async fn call_frame_barrier() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("outer".into(), "val".into());
        ctx.push_call("fn".into(), vec![("arg".into(), "argval".into())]);
        // Can see arg
        assert_eq!(ctx.lookup("arg").await, Some("argval".into()));
        // Cannot see outer shell vars
        assert_eq!(ctx.lookup("outer").await, None);
        // Can see env
        assert_eq!(ctx.lookup("PATH").await, Some("/usr/bin".into()));
        ctx.pop_call();
        // After pop, can see outer again
        assert_eq!(ctx.lookup("outer").await, Some("val".into()));
    }

    #[tokio::test]
    async fn nested_calls_stack() {
        let mut ctx = test_ctx();
        ctx.push_call("f1".into(), vec![("a".into(), "1".into())]);
        ctx.push_call("f2".into(), vec![("b".into(), "2".into())]);
        assert_eq!(ctx.lookup("b").await, Some("2".into()));
        assert_eq!(ctx.lookup("a").await, None); // barrier
        ctx.pop_call();
        assert_eq!(ctx.lookup("a").await, Some("1".into()));
        ctx.pop_call();
    }

    // ─── Let insert ──────────────────────────────────────────

    #[tokio::test]
    async fn let_insert_in_shell() {
        let mut ctx = test_ctx();
        ctx.let_insert("x".into(), "v".into());
        assert_eq!(ctx.lookup("x").await, Some("v".into()));
    }

    #[tokio::test]
    async fn let_insert_in_call() {
        let mut ctx = test_ctx();
        ctx.push_call("fn".into(), vec![]);
        ctx.let_insert("local".into(), "val".into());
        assert_eq!(ctx.lookup("local").await, Some("val".into()));
        ctx.pop_call();
        assert_eq!(ctx.lookup("local").await, None);
    }

    // ─── Assign ──────────────────────────────────────────────

    #[tokio::test]
    async fn assign_in_shell() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("x".into(), "old".into());
        assert!(ctx.assign("x", "new".into()).await);
        assert_eq!(ctx.lookup("x").await, Some("new".into()));
    }

    #[tokio::test]
    async fn assign_missing_returns_false() {
        let mut ctx = test_ctx();
        assert!(!ctx.assign("nope", "val".into()).await);
    }

    #[tokio::test]
    async fn assign_falls_through_to_scope() {
        let mut ctx = test_ctx();
        ctx.scope
            .vars()
            .lock()
            .await
            .insert("g".into(), "old".into());
        assert!(ctx.assign("g", "new".into()).await);
        assert_eq!(ctx.scope.vars().lock().await.get("g"), Some("new"));
    }

    // ─── Timeout ─────────────────────────────────────────────

    #[test]
    fn timeout_default_fallback() {
        let ctx = test_ctx();
        assert_eq!(ctx.timeout().raw_duration(), Duration::from_secs(5));
    }

    #[test]
    fn timeout_shell_overrides_default() {
        let mut ctx = test_ctx();
        ctx.shell.timeout = Some(IrTimeout::tolerance(Duration::from_secs(10)));
        assert_eq!(ctx.timeout().raw_duration(), Duration::from_secs(10));
    }

    #[test]
    fn timeout_call_frame_overrides_shell() {
        let mut ctx = test_ctx();
        ctx.shell.timeout = Some(IrTimeout::tolerance(Duration::from_secs(10)));
        ctx.push_call("fn".into(), vec![]);
        ctx.set_timeout(IrTimeout::tolerance(Duration::from_secs(1)));
        assert_eq!(ctx.timeout().raw_duration(), Duration::from_secs(1));
        ctx.pop_call();
        assert_eq!(ctx.timeout().raw_duration(), Duration::from_secs(10));
    }

    // ─── Fail pattern ────────────────────────────────────────

    #[test]
    fn fail_pattern_default_none() {
        let ctx = test_ctx();
        assert!(ctx.fail_pattern().is_none());
    }

    #[test]
    fn fail_pattern_set_and_get() {
        let mut ctx = test_ctx();
        ctx.set_fail_pattern(Some(FailPattern::Literal("ERR".into())));
        assert!(ctx.fail_pattern().is_some());
    }

    #[test]
    fn fail_pattern_call_frame_isolated() {
        let mut ctx = test_ctx();
        ctx.set_fail_pattern(Some(FailPattern::Literal("shell".into())));
        ctx.push_call("fn".into(), vec![]);
        // Call inherits shell's fail pattern
        assert!(ctx.fail_pattern().is_some());
        ctx.set_fail_pattern(None);
        assert!(ctx.fail_pattern().is_none());
        ctx.pop_call();
        // Shell still has its pattern
        assert!(ctx.fail_pattern().is_some());
    }

    // ─── Name resolution ─────────────────────────────────────

    #[test]
    fn current_name_bare() {
        let ctx = test_ctx();
        assert_eq!(ctx.current_name(), "sh");
    }

    #[test]
    fn current_name_effect_no_alias() {
        let mut ctx = test_ctx();
        ctx.shell.effect_name = Some("Setup".into());
        ctx.shell.name = "psql".into();
        assert_eq!(ctx.current_name(), "Setup.psql");
    }

    #[test]
    fn current_name_effect_with_alias() {
        let mut ctx = test_ctx();
        ctx.shell.effect_name = Some("Setup".into());
        ctx.shell.effect_alias = Some("Db".into());
        ctx.shell.name = "psql".into();
        assert_eq!(ctx.current_name(), "Db(Setup).psql");
    }

    #[test]
    fn current_name_replaced_by_export_chain() {
        // Each export step replaces the view; nothing accumulates.
        let mut ctx = test_ctx();
        ctx.shell.name = "inner".into();
        // First export: Inner → Outer (Outer's `start Inner as Dep`).
        ctx.reset_for_export(
            Scope::Effect {
                name: "Outer".into(),
                vars: Arc::new(Mutex::new(VarScope::new())),
                _timeout: None,
                env: Arc::new(LayeredEnv::root(Env::new())),
            },
            Some("Dep".into()),
            Some("Inner".into()),
            "inner".into(),
        );
        assert_eq!(ctx.current_name(), "Dep(Inner).inner");
        // Second export: Outer → test (test's `start Outer as O`, exposed-as `wrapped`).
        ctx.reset_for_export(
            test_scope("my test"),
            Some("O".into()),
            Some("Outer".into()),
            "wrapped".into(),
        );
        assert_eq!(ctx.current_name(), "O(Outer).wrapped");
    }

    // ─── Captures ────────────────────────────────────────────

    #[test]
    fn capture_in_shell() {
        let mut ctx = test_ctx();
        let mut caps = Captures::new();
        caps.set("0".into(), "whole".into());
        caps.set("1".into(), "first".into());
        ctx.set_captures(caps);
        assert_eq!(ctx.capture(0), Some("whole".into()));
        assert_eq!(ctx.capture(1), Some("first".into()));
        assert_eq!(ctx.capture(2), None);
    }

    #[test]
    fn capture_in_call_frame() {
        let mut ctx = test_ctx();
        let mut shell_caps = Captures::new();
        shell_caps.set("1".into(), "shell".into());
        ctx.set_captures(shell_caps);

        ctx.push_call("fn".into(), vec![]);
        let mut fn_caps = Captures::new();
        fn_caps.set("1".into(), "fn".into());
        ctx.set_captures(fn_caps);
        assert_eq!(ctx.capture(1), Some("fn".into()));
        ctx.pop_call();
        assert_eq!(ctx.capture(1), Some("shell".into()));
    }

    // ─── Reset for export ────────────────────────────────────

    #[tokio::test]
    async fn reset_for_export_clears_vars_and_captures() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("x".into(), "v".into());
        let mut caps = Captures::new();
        caps.set("1".into(), "c".into());
        ctx.set_captures(caps);
        ctx.shell.timeout = Some(IrTimeout::tolerance(Duration::from_secs(99)));

        let new_scope = test_scope("new test");
        ctx.reset_for_export(new_scope, None, None, "sh".into());

        assert_eq!(ctx.lookup("x").await, None);
        assert_eq!(ctx.capture(1), None);
        assert_eq!(ctx.scope.name(), "new test");
        // timeout preserved
        assert_eq!(
            ctx.shell.timeout.as_ref().unwrap().raw_duration(),
            Duration::from_secs(99)
        );
    }

    // ─── Effect scope with overlay ───────────────────────────

    #[tokio::test]
    async fn effect_scope_overlay_lookup() {
        let mut overlay_map = HashMap::new();
        overlay_map.insert("PORT".into(), "5432".into());

        let scope = Scope::Effect {
            name: "Db".into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            _timeout: None,
            env: Arc::new(LayeredEnv::root(Env::from_map(overlay_map))),
        };
        let shell = ShellState::new("db".into());
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
            0,
        );
        assert_eq!(ctx.lookup("PORT").await, Some("5432".into()));
    }

    // ─── LayeredEnv chain bugs ─────────────────────────────

    #[tokio::test]
    async fn effect_scope_lookup_walks_parent_layers() {
        // Parent layer has BASE_PORT, child overlay has LABEL.
        // lookup("BASE_PORT") should walk the chain and find it.
        let mut base = Env::new();
        base.insert("BASE_PORT".into(), "5432".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut overlay = Env::new();
        overlay.insert("LABEL".into(), "child".into());
        let child_env = Arc::new(LayeredEnv::child(root, overlay));

        let scope = Scope::Effect {
            name: "Child".into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            _timeout: None,
            env: child_env,
        };
        let shell = ShellState::new("s".into());
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
            0,
        );
        // lookup walks the chain — this works correctly
        assert_eq!(ctx.lookup("BASE_PORT").await, Some("5432".into()));
        assert_eq!(ctx.lookup("LABEL").await, Some("child".into()));
    }

    #[test]
    fn process_env_includes_parent_layer_variables() {
        // Regression test: process_env() must include variables from parent
        // LayeredEnv layers, not just the immediate layer.
        let mut base = Env::new();
        base.insert("BASE_PORT".into(), "5432".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut overlay = Env::new();
        overlay.insert("LABEL".into(), "child".into());
        let child_env = Arc::new(LayeredEnv::child(root, overlay));

        let scope = Scope::Effect {
            name: "Child".into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            _timeout: None,
            env: child_env,
        };
        let shell = ShellState::new("s".into());
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
            0,
        );
        let penv: HashMap<String, String> = ctx.process_env().into_iter().collect();
        // Child's own overlay should be present
        assert_eq!(penv.get("LABEL"), Some(&"child".to_string()));
        // Parent layer variable should also be present in the PTY env
        assert_eq!(
            penv.get("BASE_PORT"),
            Some(&"5432".to_string()),
            "process_env must include variables from parent LayeredEnv layers"
        );
    }

    // ─── snapshot_user_vars ─────────────────────────────────

    #[tokio::test]
    async fn snapshot_user_vars_in_shell_scope() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("a".into(), "1".into());
        ctx.scope.vars().lock().await.insert("b".into(), "2".into());
        let snap = ctx.snapshot_user_vars().await;
        assert_eq!(
            snap,
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }

    #[tokio::test]
    async fn snapshot_user_vars_in_call_frame_only() {
        let mut ctx = test_ctx();
        ctx.shell.vars.insert("outer".into(), "v".into());
        ctx.push_call("fn".into(), vec![("arg".into(), "av".into())]);
        ctx.let_insert("local".into(), "lv".into());
        let snap = ctx.snapshot_user_vars().await;
        // Only the innermost call frame's vars are visible (matches lookup barrier).
        assert_eq!(
            snap,
            vec![("arg".into(), "av".into()), ("local".into(), "lv".into()),]
        );
    }

    #[tokio::test]
    async fn snapshot_user_vars_excludes_env() {
        let ctx = test_ctx();
        let snap = ctx.snapshot_user_vars().await;
        // PATH is in the layered env, not in scope/shell vars — must be excluded.
        assert!(snap.iter().all(|(k, _)| k != "PATH"));
    }

    // ─── Captures unit tests ────────────────────────────────

    #[test]
    fn captures_new_is_empty() {
        let c = Captures::new();
        assert_eq!(c.get_indexed(0), None);
        assert_eq!(c.get_named("foo"), None);
    }

    #[test]
    fn captures_set_and_get_indexed() {
        let mut c = Captures::new();
        c.set("0".into(), "whole".into());
        c.set("1".into(), "first".into());
        assert_eq!(c.get_indexed(0), Some("whole"));
        assert_eq!(c.get_indexed(1), Some("first"));
        assert_eq!(c.get_indexed(2), None);
    }

    #[test]
    fn captures_set_and_get_named() {
        let mut c = Captures::new();
        c.set("host".into(), "localhost".into());
        assert_eq!(c.get_named("host"), Some("localhost"));
        assert_eq!(c.get_named("port"), None);
    }

    #[test]
    fn captures_get_generic() {
        let mut c = Captures::new();
        c.set("1".into(), "idx".into());
        c.set("name".into(), "named".into());
        assert_eq!(c.get("1"), Some("idx"));
        assert_eq!(c.get("name"), Some("named"));
    }

    #[test]
    fn captures_clear() {
        let mut c = Captures::new();
        c.set("1".into(), "val".into());
        c.clear();
        assert_eq!(c.get("1"), None);
    }

    #[test]
    fn captures_clone() {
        let mut c = Captures::new();
        c.set("1".into(), "val".into());
        let cloned = c.clone();
        assert_eq!(cloned.get("1"), Some("val"));
    }
}
