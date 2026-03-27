use std::collections::HashMap;

pub mod bifs;
pub mod evaluator;

// ─── VarScope ───────────────────────────────────────────────

/// A single variable scope — flat name→value mapping.
///
/// Used by the evaluator for per-call variable frames and
/// by the runtime's `ExecutionContext` for its frame variables.
#[derive(Debug, Default, Clone)]
pub struct VarScope {
    vars: HashMap<String, String>,
}

impl VarScope {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    pub fn from_map(vars: HashMap<String, String>) -> Self {
        Self { vars }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.vars.insert(key, value);
    }

    /// Assign a new value to an existing key. Returns `true` if the key
    /// existed (and was updated), `false` if the key was not found.
    pub fn assign(&mut self, key: &str, value: String) -> bool {
        if let Some(slot) = self.vars.get_mut(key) {
            *slot = value;
            true
        } else {
            false
        }
    }
}

// ─── Env ─────────────────────────────────────────────────────

/// Immutable snapshot of environment variables, captured once before
/// resolution. Shared between the resolver (marker evaluation) and
/// the runtime (variable fallback).
#[derive(Debug, Clone)]
pub struct Env {
    vars: HashMap<String, String>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    /// Create an empty environment.
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    /// Snapshot the current process environment.
    pub fn capture() -> Self {
        Self {
            vars: std::env::vars().collect(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.vars.insert(key, value);
    }

    pub fn from_map(vars: HashMap<String, String>) -> Self {
        Self { vars }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_scope_insert_and_get() {
        let mut s = VarScope::new();
        s.insert("x".into(), "hello".into());
        assert_eq!(s.get("x"), Some("hello"));
    }

    #[test]
    fn var_scope_get_missing_returns_none() {
        let s = VarScope::new();
        assert_eq!(s.get("nope"), None);
    }

    #[test]
    fn var_scope_insert_overwrites() {
        let mut s = VarScope::new();
        s.insert("x".into(), "a".into());
        s.insert("x".into(), "b".into());
        assert_eq!(s.get("x"), Some("b"));
    }

    #[test]
    fn var_scope_assign_existing_returns_true() {
        let mut s = VarScope::new();
        s.insert("x".into(), "old".into());
        assert!(s.assign("x", "new".into()));
        assert_eq!(s.get("x"), Some("new"));
    }

    #[test]
    fn var_scope_assign_missing_returns_false() {
        let mut s = VarScope::new();
        assert!(!s.assign("x", "val".into()));
    }

    #[test]
    fn var_scope_assign_missing_does_not_insert() {
        let mut s = VarScope::new();
        s.assign("x", "val".into());
        assert_eq!(s.get("x"), None);
    }

    #[test]
    fn var_scope_assign_empty_string() {
        let mut s = VarScope::new();
        s.insert("x".into(), "something".into());
        s.assign("x", String::new());
        assert_eq!(s.get("x"), Some(""));
    }

    #[test]
    fn var_scope_insert_empty_key() {
        let mut s = VarScope::new();
        s.insert(String::new(), "val".into());
        assert_eq!(s.get(""), Some("val"));
    }

    #[test]
    fn var_scope_insert_empty_value() {
        let mut s = VarScope::new();
        s.insert("k".into(), String::new());
        assert_eq!(s.get("k"), Some(""));
    }

    #[test]
    fn var_scope_default_is_empty() {
        let s = VarScope::default();
        assert_eq!(s.get("anything"), None);
    }

    #[test]
    fn var_scope_multiple_keys() {
        let mut s = VarScope::new();
        s.insert("a".into(), "1".into());
        s.insert("b".into(), "2".into());
        s.insert("c".into(), "3".into());
        assert_eq!(s.get("a"), Some("1"));
        assert_eq!(s.get("b"), Some("2"));
        assert_eq!(s.get("c"), Some("3"));
    }

    // ─── Env tests ───────────────────────────────────────────

    #[test]
    fn env_capture() {
        let env = Env::capture();
        assert!(env.get("PATH").is_some() || env.get("HOME").is_some());
    }

    #[test]
    fn env_get_existing() {
        let mut m = HashMap::new();
        m.insert("KEY".into(), "value".into());
        let env = Env::from_map(m);
        assert_eq!(env.get("KEY"), Some("value"));
    }

    #[test]
    fn env_get_missing() {
        let env = Env::from_map(HashMap::new());
        assert_eq!(env.get("NOPE"), None);
    }

    #[test]
    fn env_from_map() {
        let mut m = HashMap::new();
        m.insert("A".into(), "1".into());
        m.insert("B".into(), "2".into());
        let env = Env::from_map(m);
        assert_eq!(env.get("A"), Some("1"));
        assert_eq!(env.get("B"), Some("2"));
    }

    #[test]
    fn env_from_empty_map() {
        let env = Env::from_map(HashMap::new());
        assert_eq!(env.get("anything"), None);
    }

    #[test]
    fn env_get_empty_value() {
        let mut m = HashMap::new();
        m.insert("EMPTY".into(), String::new());
        let env = Env::from_map(m);
        assert_eq!(env.get("EMPTY"), Some(""));
    }

    #[test]
    fn env_clone() {
        let mut m = HashMap::new();
        m.insert("K".into(), "V".into());
        let env = Env::from_map(m);
        let cloned = env.clone();
        assert_eq!(cloned.get("K"), Some("V"));
    }

    #[test]
    fn env_insert() {
        let mut env = Env::from_map(HashMap::new());
        env.insert("NEW".into(), "val".into());
        assert_eq!(env.get("NEW"), Some("val"));
    }

    #[test]
    fn env_insert_overwrites() {
        let mut env = Env::from_map(HashMap::new());
        env.insert("K".into(), "old".into());
        env.insert("K".into(), "new".into());
        assert_eq!(env.get("K"), Some("new"));
    }
}
