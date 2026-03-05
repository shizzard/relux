use std::collections::HashMap;
use std::sync::Arc;

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
pub struct VariableStack {
    frames: Vec<HashMap<String, String>>,
    pub captures: HashMap<String, String>,
    test_scope: Arc<Mutex<TestScope>>,
    overlay: HashMap<String, String>,
    env: Arc<Env>,
}

impl VariableStack {
    pub fn new(
        test_scope: Arc<Mutex<TestScope>>,
        overlay: HashMap<String, String>,
        env: Arc<Env>,
    ) -> Self {
        Self {
            frames: vec![HashMap::new()],
            captures: HashMap::new(),
            test_scope,
            overlay,
            env,
        }
    }

    pub fn push_frame(&mut self) {
        self.frames.push(HashMap::new());
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            let _ = self.frames.pop();
        }
    }

    pub async fn lookup(&self, key: &str) -> Option<String> {
        if let Some(v) = self.captures.get(key) {
            return Some(v.clone());
        }

        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get(key) {
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
            frame.insert(key, value);
        }
    }

    pub async fn assign(&mut self, key: &str, value: String) -> bool {
        for frame in self.frames.iter_mut().rev() {
            if let Some(slot) = frame.get_mut(key) {
                *slot = value;
                return true;
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

pub async fn interpolate(expr: &StringExpr, vars: &VariableStack) -> String {
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
