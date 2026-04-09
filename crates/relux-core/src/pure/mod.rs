use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

pub mod bifs;

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

// ─── LayeredEnv ─────────────────────────────────────────────

/// Layered environment with recursive parent chain.
///
/// Each layer holds a small overlay (`own`) and points to a parent
/// `LayeredEnv`. The root layer wraps the base process environment
/// with no parent. Lookups walk the chain: own → parent → grandparent → ...
///
/// No cloning of the base env — each layer is `Arc`-shared.
#[derive(Debug, Clone)]
pub struct LayeredEnv {
    own: Env,
    parent: Option<Arc<LayeredEnv>>,
}

impl LayeredEnv {
    /// Create the root layer from the base process environment.
    pub fn root(base: Env) -> Self {
        Self {
            own: base,
            parent: None,
        }
    }

    /// Create a child layer with the given overlay on top of this env.
    pub fn child(parent: Arc<LayeredEnv>, overlay: Env) -> Self {
        Self {
            own: overlay,
            parent: Some(parent),
        }
    }

    /// Look up a variable, walking the chain until found.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.own
            .get(key)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get(key)))
    }

    /// Iterate all entries across all layers. Closest layer wins on duplicates.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut current = Some(self);
        while let Some(layer) = current {
            for (k, v) in layer.own.iter() {
                if seen.insert(k) {
                    entries.push((k, v));
                }
            }
            current = layer.parent.as_deref();
        }
        entries.into_iter()
    }
}

impl From<Env> for LayeredEnv {
    fn from(env: Env) -> Self {
        Self::root(env)
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

    // ─── LayeredEnv tests ────────────────────────────────────

    #[test]
    fn layered_root_lookup() {
        let mut base = Env::new();
        base.insert("PATH".into(), "/usr/bin".into());
        let root = LayeredEnv::root(base);
        assert_eq!(root.get("PATH"), Some("/usr/bin"));
        assert_eq!(root.get("NOPE"), None);
    }

    #[test]
    fn layered_child_overrides_parent() {
        let mut base = Env::new();
        base.insert("PORT".into(), "3000".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut overlay = Env::new();
        overlay.insert("PORT".into(), "5432".into());
        let child = LayeredEnv::child(root, overlay);

        assert_eq!(child.get("PORT"), Some("5432"));
    }

    #[test]
    fn layered_child_inherits_parent() {
        let mut base = Env::new();
        base.insert("PATH".into(), "/usr/bin".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut overlay = Env::new();
        overlay.insert("PORT".into(), "5432".into());
        let child = LayeredEnv::child(root, overlay);

        // Child sees its own entry
        assert_eq!(child.get("PORT"), Some("5432"));
        // Child inherits parent entry
        assert_eq!(child.get("PATH"), Some("/usr/bin"));
    }

    #[test]
    fn layered_three_levels() {
        let mut base = Env::new();
        base.insert("BASE".into(), "root".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut mid_overlay = Env::new();
        mid_overlay.insert("MID".into(), "middle".into());
        let mid = Arc::new(LayeredEnv::child(root, mid_overlay));

        let mut top_overlay = Env::new();
        top_overlay.insert("TOP".into(), "leaf".into());
        let top = LayeredEnv::child(mid, top_overlay);

        assert_eq!(top.get("TOP"), Some("leaf"));
        assert_eq!(top.get("MID"), Some("middle"));
        assert_eq!(top.get("BASE"), Some("root"));
        assert_eq!(top.get("NOPE"), None);
    }

    #[test]
    fn layered_deeper_override() {
        let mut base = Env::new();
        base.insert("X".into(), "base".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut mid_overlay = Env::new();
        mid_overlay.insert("X".into(), "mid".into());
        let mid = Arc::new(LayeredEnv::child(root, mid_overlay));

        let mut top_overlay = Env::new();
        top_overlay.insert("X".into(), "top".into());
        let top = LayeredEnv::child(mid, top_overlay);

        // Nearest layer wins
        assert_eq!(top.get("X"), Some("top"));
    }

    // ─── From<Env> ──────────────────────────────────────────

    #[test]
    fn from_env_creates_root() {
        let mut env = Env::new();
        env.insert("K".into(), "V".into());
        let layered: LayeredEnv = env.into();
        assert_eq!(layered.get("K"), Some("V"));
    }

    // ─── iter() tests ───────────────────────────────────────

    #[test]
    fn iter_single_layer() {
        let mut base = Env::new();
        base.insert("A".into(), "1".into());
        base.insert("B".into(), "2".into());
        let root = LayeredEnv::root(base);
        let entries: HashMap<&str, &str> = root.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries["A"], "1");
        assert_eq!(entries["B"], "2");
    }

    #[test]
    fn iter_two_layers_closest_wins() {
        let mut base = Env::new();
        base.insert("X".into(), "base".into());
        base.insert("Y".into(), "base".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut overlay = Env::new();
        overlay.insert("X".into(), "child".into());
        let child = LayeredEnv::child(root, overlay);

        let entries: HashMap<&str, &str> = child.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries["X"], "child");
        assert_eq!(entries["Y"], "base");
    }

    #[test]
    fn iter_three_layers() {
        let mut base = Env::new();
        base.insert("A".into(), "root".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mut mid = Env::new();
        mid.insert("B".into(), "mid".into());
        let mid = Arc::new(LayeredEnv::child(root, mid));

        let mut top = Env::new();
        top.insert("C".into(), "top".into());
        let top = LayeredEnv::child(mid, top);

        let entries: HashMap<&str, &str> = top.iter().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries["A"], "root");
        assert_eq!(entries["B"], "mid");
        assert_eq!(entries["C"], "top");
    }

    #[test]
    fn iter_empty_layers_skipped() {
        let base = Env::new();
        let root = Arc::new(LayeredEnv::root(base));
        let child = LayeredEnv::child(root, Env::new());
        assert_eq!(child.iter().count(), 0);
    }

    #[test]
    fn iter_deep_override() {
        let mut base = Env::new();
        base.insert("X".into(), "root".into());
        let root = Arc::new(LayeredEnv::root(base));

        let mid = Arc::new(LayeredEnv::child(root, Env::new()));

        let mut top = Env::new();
        top.insert("X".into(), "top".into());
        let top = LayeredEnv::child(mid, top);

        let entries: HashMap<&str, &str> = top.iter().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries["X"], "top");
    }
}
