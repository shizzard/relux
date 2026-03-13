use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tokio::sync::Mutex;

use crate::dsl::resolver::ir::{Expr, Spanned, StringExpr, StringPart, Timeout, VarAssign, VarDecl};

pub type Env = HashMap<String, String>;

/// Saved state from before a function call, used to restore after the call.
#[derive(Debug)]
pub struct FunctionSave {
    frames: Vec<Frame>,
    captures: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub enum FailPattern {
    Regex(Regex),
    Literal(String),
}

#[derive(Debug, Default)]
pub struct TestScope {
    values: HashMap<String, String>,
}

impl TestScope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.values.insert(key, value);
    }

    pub fn assign(&mut self, key: &str, value: String) -> bool {
        if let Some(slot) = self.values.get_mut(key) {
            *slot = value;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
struct Frame {
    vars: HashMap<String, String>,
    timeout: Timeout,
    fail_pattern: Option<FailPattern>,
}

#[derive(Debug)]
pub struct ScopeStack {
    frames: Vec<Frame>,
    pub captures: HashMap<String, String>,
    test_scope: Arc<Mutex<TestScope>>,
    overlay: HashMap<String, String>,
    env: Arc<Env>,
}

impl ScopeStack {
    pub fn new(
        test_scope: Arc<Mutex<TestScope>>,
        overlay: HashMap<String, String>,
        env: Arc<Env>,
        default_timeout: Timeout,
    ) -> Self {
        Self {
            frames: vec![Frame {
                vars: HashMap::new(),
                timeout: default_timeout,
                fail_pattern: None,
            }],
            captures: HashMap::new(),
            test_scope,
            overlay,
            env,
        }
    }

    pub fn push_frame(&mut self) {
        let timeout = self.timeout().clone();
        let fail_pattern = self.fail_pattern().cloned();
        self.frames.push(Frame {
            vars: HashMap::new(),
            timeout,
            fail_pattern,
        });
    }

    /// Enter a function call: save current frames/captures and replace with a
    /// single isolated frame containing only the function's parameters.
    /// Returns the saved state to be passed to `exit_function()`.
    pub fn enter_function(&mut self, params: HashMap<String, String>) -> FunctionSave {
        let timeout = self.timeout().clone();
        let fail_pattern = self.fail_pattern().cloned();
        let saved_frames = std::mem::replace(
            &mut self.frames,
            vec![Frame {
                vars: params,
                timeout,
                fail_pattern,
            }],
        );
        let saved_captures = std::mem::take(&mut self.captures);
        FunctionSave {
            frames: saved_frames,
            captures: saved_captures,
        }
    }

    /// Exit a function call: restore the saved frames/captures.
    /// Timeout and fail pattern changes inside the function are discarded.
    pub fn exit_function(&mut self, save: FunctionSave) {
        self.frames = save.frames;
        self.captures = save.captures;
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            let _ = self.frames.pop();
        }
    }

    pub fn timeout(&self) -> &Timeout {
        &self.frames.last().unwrap().timeout
    }

    pub fn set_timeout(&mut self, t: Timeout) {
        self.frames.last_mut().unwrap().timeout = t;
    }

    pub fn fail_pattern(&self) -> Option<&FailPattern> {
        self.frames.last().unwrap().fail_pattern.as_ref()
    }

    pub fn set_fail_pattern(&mut self, pattern: Option<FailPattern>) {
        self.frames.last_mut().unwrap().fail_pattern = pattern;
    }

    pub async fn lookup(&self, key: &str) -> Option<String> {
        if let Some(v) = self.captures.get(key) {
            return Some(v.clone());
        }

        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.vars.get(key) {
                return Some(v.clone());
            }
        }

        if let Some(v) = self.test_scope.lock().await.get(key).map(str::to_string) {
            return Some(v);
        }

        if let Some(v) = self.overlay.get(key) {
            return Some(v.clone());
        }

        self.env.get(key).cloned()
    }

    pub fn let_insert(&mut self, key: String, value: String) {
        if let Some(frame) = self.frames.last_mut() {
            frame.vars.insert(key, value);
        }
    }

    pub async fn assign(&mut self, key: &str, value: String) -> bool {
        for frame in self.frames.iter_mut().rev() {
            if let Some(slot) = frame.vars.get_mut(key) {
                *slot = value;
                return true;
            }
        }
        self.test_scope.lock().await.assign(key, value)
    }

    pub fn set_captures(&mut self, captures: HashMap<String, String>) {
        self.captures = captures;
    }

    /// Returns the combined env + overlay for pure function contexts.
    pub fn env(&self) -> Arc<Env> {
        self.env.clone()
    }

    pub fn process_env(&self) -> HashMap<String, String> {
        let mut out = (*self.env).clone();
        out.extend(self.overlay.clone());
        out
    }

    pub async fn exec_var_decl<F, Fut>(&mut self, decl: &VarDecl, eval_expr: F) -> String
    where
        F: Fn(&Spanned<Expr>) -> Fut,
        Fut: std::future::Future<Output = String>,
    {
        let value = match &decl.value {
            Some(expr) => eval_expr(expr).await,
            None => String::new(),
        };
        self.let_insert(decl.name.node.clone(), value.clone());
        value
    }

    pub async fn exec_var_assign<F, Fut>(&mut self, assign: &VarAssign, eval_expr: F) -> String
    where
        F: Fn(&Spanned<Expr>) -> Fut,
        Fut: std::future::Future<Output = String>,
    {
        let value = eval_expr(&assign.value).await;
        let _ = self.assign(&assign.name.node, value.clone()).await;
        value
    }
}

pub async fn interpolate(expr: &StringExpr, vars: &ScopeStack) -> String {
    let mut out = String::new();
    for part in &expr.parts {
        match &part.node {
            StringPart::Literal(s) => out.push_str(s),
            StringPart::Interp(name) => {
                if let Some(v) = vars.lookup(name).await {
                    out.push_str(&v);
                }
            }
            StringPart::EscapedDollar => out.push('$'),
        }
    }
    out
}

pub fn interpolate_with_lookup(
    expr: &StringExpr,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> String {
    let mut out = String::new();
    for part in &expr.parts {
        match &part.node {
            StringPart::Literal(s) => out.push_str(s),
            StringPart::Interp(name) => {
                if let Some(v) = lookup(name) {
                    out.push_str(&v);
                }
            }
            StringPart::EscapedDollar => out.push('$'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tol(secs: u64) -> Timeout {
        Timeout::Tolerance { duration: Duration::from_secs(secs), multiplier: 1.0 }
    }

    fn make_scope(timeout: Timeout) -> ScopeStack {
        ScopeStack::new(
            Arc::new(Mutex::new(TestScope::new())),
            HashMap::new(),
            Arc::new(HashMap::new()),
            timeout,
        )
    }

    #[test]
    fn default_timeout() {
        let scope = make_scope(tol(10));
        assert_eq!(*scope.timeout(), tol(10));
    }

    #[test]
    fn set_timeout_changes_current_frame() {
        let mut scope = make_scope(tol(10));
        scope.set_timeout(tol(30));
        assert_eq!(*scope.timeout(), tol(30));
    }

    #[test]
    fn push_frame_inherits_timeout() {
        let mut scope = make_scope(tol(10));
        scope.set_timeout(tol(5));
        scope.push_frame();
        assert_eq!(*scope.timeout(), tol(5));
    }

    #[test]
    fn pop_frame_restores_timeout() {
        let mut scope = make_scope(tol(10));
        scope.push_frame();
        scope.set_timeout(tol(99));
        assert_eq!(*scope.timeout(), tol(99));
        scope.pop_frame();
        assert_eq!(*scope.timeout(), tol(10));
    }

    #[test]
    fn nested_frames_restore_correctly() {
        let mut scope = make_scope(tol(10));

        scope.push_frame();
        scope.set_timeout(tol(20));

        scope.push_frame();
        scope.set_timeout(tol(30));
        assert_eq!(*scope.timeout(), tol(30));

        scope.pop_frame();
        assert_eq!(*scope.timeout(), tol(20));

        scope.pop_frame();
        assert_eq!(*scope.timeout(), tol(10));
    }

    #[test]
    fn pop_frame_on_root_is_noop() {
        let mut scope = make_scope(tol(10));
        scope.set_timeout(tol(5));
        scope.pop_frame();
        assert_eq!(*scope.timeout(), tol(5));
    }

    #[tokio::test]
    async fn vars_scoped_independently_from_timeout() {
        let mut scope = make_scope(tol(10));
        scope.let_insert("x".into(), "outer".into());

        scope.push_frame();
        scope.set_timeout(tol(99));
        scope.let_insert("x".into(), "inner".into());
        assert_eq!(scope.lookup("x").await.unwrap(), "inner");
        assert_eq!(*scope.timeout(), tol(99));

        scope.pop_frame();
        assert_eq!(scope.lookup("x").await.unwrap(), "outer");
        assert_eq!(*scope.timeout(), tol(10));
    }
}
