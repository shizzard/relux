use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tokio::sync::Mutex;

use crate::dsl::resolver::ir::IrTimeout;
use crate::pure::LayeredEnv;
use crate::pure::VarScope;

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
    pub name: String,
    pub alias: Option<String>,
    /// Accumulated name path from effect export chain.
    /// Each export pushes `"EffectName.shell_name"` onto this prefix.
    pub name_prefix: Vec<String>,
    pub vars: VarScope,
    pub captures: Captures,
    pub timeout: Option<IrTimeout>,
    pub fail_pattern: Option<FailPattern>,
}

impl ShellState {
    pub fn new(name: String, alias: Option<String>) -> Self {
        Self {
            name,
            alias,
            name_prefix: Vec::new(),
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
    pub default_timeout: IrTimeout,
    pub env: Arc<LayeredEnv>,
}

impl ExecutionContext {
    pub fn new(
        scope: Scope,
        shell: ShellState,
        default_timeout: IrTimeout,
        env: Arc<LayeredEnv>,
    ) -> Self {
        Self {
            scope,
            shell,
            call_stack: Vec::new(),
            default_timeout,
            env,
        }
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

    /// Current display name for logging.
    /// Builds the full qualified name from the effect export chain:
    /// e.g. `SetupDb.db.Db.db.mydb` for a 2-level effect chain with alias `mydb`.
    pub fn current_name(&self) -> String {
        let tail = self.shell.alias.as_deref().unwrap_or(&self.shell.name);
        if self.shell.name_prefix.is_empty() {
            tail.to_string()
        } else {
            format!("{}.{}", self.shell.name_prefix.join("."), tail)
        }
    }

    /// Reset for shell export (effect → test/parent effect).
    /// Accumulates the current scope+shell name into the name prefix chain.
    pub fn reset_for_export(&mut self, new_scope: Scope) {
        // Push "EffectName.shell_name" onto the prefix before switching scope
        let segment = format!("{}.{}", self.scope.name(), self.shell.name);
        self.shell.name_prefix.push(segment);
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
    use crate::pure::Env;
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
        ShellState::new(name.into(), None)
    }

    fn test_ctx() -> ExecutionContext {
        ExecutionContext::new(
            test_scope("my test"),
            test_shell("sh"),
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
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
    fn current_name_shell() {
        let ctx = test_ctx();
        assert_eq!(ctx.current_name(), "sh");
    }

    #[test]
    fn current_name_alias() {
        let mut ctx = test_ctx();
        ctx.shell.alias = Some("mydb".into());
        assert_eq!(ctx.current_name(), "mydb");
    }

    #[test]
    fn current_name_with_prefix() {
        let mut ctx = test_ctx();
        ctx.shell.name_prefix = vec!["SetupDb.db".into(), "Db.db".into()];
        ctx.shell.alias = Some("mydb".into());
        assert_eq!(ctx.current_name(), "SetupDb.db.Db.db.mydb");
    }

    #[test]
    fn current_name_with_prefix_no_alias() {
        let mut ctx = test_ctx();
        ctx.shell.name_prefix = vec!["Db.db".into()];
        assert_eq!(ctx.current_name(), "Db.db.sh");
    }

    #[test]
    fn current_name_accumulated_via_export() {
        let mut ctx = test_ctx();
        ctx.scope = Scope::Effect {
            name: "Db".into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            _timeout: None,
            env: Arc::new(LayeredEnv::root(Env::new())),
        };
        ctx.shell.name = "db".into();
        // First export: Db.db → SetupDb
        ctx.reset_for_export(Scope::Effect {
            name: "SetupDb".into(),
            vars: Arc::new(Mutex::new(VarScope::new())),
            _timeout: None,
            env: Arc::new(LayeredEnv::root(Env::new())),
        });
        assert_eq!(ctx.shell.name_prefix, vec!["Db.db"]);
        // Second export: SetupDb.db → test
        ctx.reset_for_export(test_scope("my test"));
        assert_eq!(ctx.shell.name_prefix, vec!["Db.db", "SetupDb.db"]);
        ctx.shell.alias = Some("mydb".into());
        assert_eq!(ctx.current_name(), "Db.db.SetupDb.db.mydb");
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
        ctx.reset_for_export(new_scope);

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
        let shell = ShellState::new("db".into(), None);
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
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
        let shell = ShellState::new("s".into(), None);
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
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
        let shell = ShellState::new("s".into(), None);
        let ctx = ExecutionContext::new(
            scope,
            shell,
            IrTimeout::tolerance(Duration::from_secs(5)),
            test_env(),
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
