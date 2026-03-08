use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::dsl::resolver::ir::{Expr, Spanned, StringExpr, StringPart, VarAssign, VarDecl};

pub type Env = HashMap<String, String>;

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
    timeout: Duration,
    /// Function call boundary — prevents `assign()` from walking into the caller's scope.
    is_function_scope: bool,
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
        default_timeout: Duration,
    ) -> Self {
        Self {
            frames: vec![Frame {
                vars: HashMap::new(),
                timeout: default_timeout,
                is_function_scope: false,
            }],
            captures: HashMap::new(),
            test_scope,
            overlay,
            env,
        }
    }

    pub fn push_frame(&mut self) {
        let timeout = self.timeout();
        self.frames.push(Frame {
            vars: HashMap::new(),
            timeout,
            is_function_scope: false,
        });
    }

    pub fn push_function_frame(&mut self) {
        let timeout = self.timeout();
        self.frames.push(Frame {
            vars: HashMap::new(),
            timeout,
            is_function_scope: true,
        });
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            let _ = self.frames.pop();
        }
    }

    pub fn timeout(&self) -> Duration {
        self.frames.last().unwrap().timeout
    }

    pub fn set_timeout(&mut self, d: Duration) {
        self.frames.last_mut().unwrap().timeout = d;
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
            if frame.is_function_scope {
                break;
            }
        }
        self.test_scope.lock().await.assign(key, value)
    }

    pub fn set_captures(&mut self, captures: HashMap<String, String>) {
        self.captures = captures;
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

    fn make_scope(timeout: Duration) -> ScopeStack {
        ScopeStack::new(
            Arc::new(Mutex::new(TestScope::new())),
            HashMap::new(),
            Arc::new(HashMap::new()),
            timeout,
        )
    }

    #[test]
    fn default_timeout() {
        let scope = make_scope(Duration::from_secs(10));
        assert_eq!(scope.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn set_timeout_changes_current_frame() {
        let mut scope = make_scope(Duration::from_secs(10));
        scope.set_timeout(Duration::from_secs(30));
        assert_eq!(scope.timeout(), Duration::from_secs(30));
    }

    #[test]
    fn push_frame_inherits_timeout() {
        let mut scope = make_scope(Duration::from_secs(10));
        scope.set_timeout(Duration::from_secs(5));
        scope.push_frame();
        assert_eq!(scope.timeout(), Duration::from_secs(5));
    }

    #[test]
    fn pop_frame_restores_timeout() {
        let mut scope = make_scope(Duration::from_secs(10));
        scope.push_frame();
        scope.set_timeout(Duration::from_secs(99));
        assert_eq!(scope.timeout(), Duration::from_secs(99));
        scope.pop_frame();
        assert_eq!(scope.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn nested_frames_restore_correctly() {
        let mut scope = make_scope(Duration::from_secs(10));

        scope.push_frame();
        scope.set_timeout(Duration::from_secs(20));

        scope.push_frame();
        scope.set_timeout(Duration::from_secs(30));
        assert_eq!(scope.timeout(), Duration::from_secs(30));

        scope.pop_frame();
        assert_eq!(scope.timeout(), Duration::from_secs(20));

        scope.pop_frame();
        assert_eq!(scope.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn pop_frame_on_root_is_noop() {
        let mut scope = make_scope(Duration::from_secs(10));
        scope.set_timeout(Duration::from_secs(5));
        scope.pop_frame();
        assert_eq!(scope.timeout(), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn vars_scoped_independently_from_timeout() {
        let mut scope = make_scope(Duration::from_secs(10));
        scope.let_insert("x".into(), "outer".into());

        scope.push_frame();
        scope.set_timeout(Duration::from_secs(99));
        scope.let_insert("x".into(), "inner".into());
        assert_eq!(scope.lookup("x").await.unwrap(), "inner");
        assert_eq!(scope.timeout(), Duration::from_secs(99));

        scope.pop_frame();
        assert_eq!(scope.lookup("x").await.unwrap(), "outer");
        assert_eq!(scope.timeout(), Duration::from_secs(10));
    }
}
