use std::collections::HashSet;
use std::sync::Arc;

use crate::pure::LayeredEnv;

// ─── ShallowEnv ─────────────────────────────────────────────

/// A set of known variable names (no values).
/// Used at resolve time to track which names are available
/// without evaluating any expressions.
#[derive(Debug, Clone)]
pub struct ShallowEnv(HashSet<String>);

impl ShallowEnv {
    pub fn new() -> Self {
        Self(HashSet::new())
    }

    pub fn from_layered(env: &LayeredEnv) -> Self {
        Self(env.iter().map(|(k, _)| k.to_string()).collect())
    }

    pub fn from_names(names: impl IntoIterator<Item = String>) -> Self {
        Self(names.into_iter().collect())
    }

    pub fn insert(&mut self, name: String) {
        self.0.insert(name);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}

// ─── ShallowLayeredEnv ──────────────────────────────────────

/// A layered set of known variable names that mirrors the runtime
/// `LayeredEnv` structure but tracks only name presence.
///
/// Used by the resolver to validate `expect` satisfiability:
/// at each `start` site, every expected var must be reachable
/// through the layer chain (overlay keys, let bindings, base env).
#[derive(Debug, Clone)]
pub struct ShallowLayeredEnv {
    own: ShallowEnv,
    parent: Option<Arc<ShallowLayeredEnv>>,
}

impl ShallowLayeredEnv {
    /// Root layer from the base process environment.
    pub fn root(env: &LayeredEnv) -> Self {
        Self {
            own: ShallowEnv::from_layered(env),
            parent: None,
        }
    }

    /// Child layer with a set of overlay/let-bound names.
    pub fn child(parent: Arc<Self>, names: impl IntoIterator<Item = String>) -> Self {
        Self {
            own: ShallowEnv::from_names(names),
            parent: Some(parent),
        }
    }

    /// Convenience: child layer with a single added name (for `let` bindings).
    pub fn with_name(parent: &Arc<Self>, name: String) -> Self {
        let mut own = ShallowEnv::new();
        own.insert(name);
        Self {
            own,
            parent: Some(Arc::clone(parent)),
        }
    }

    /// Check if a name is reachable anywhere in the layer chain.
    pub fn contains(&self, name: &str) -> bool {
        self.own.contains(name) || self.parent.as_ref().is_some_and(|p| p.contains(name))
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pure::Env;
    use std::collections::HashMap;

    fn make_env(keys: &[&str]) -> LayeredEnv {
        let map: HashMap<String, String> = keys
            .iter()
            .map(|k| (k.to_string(), String::new()))
            .collect();
        LayeredEnv::from(Env::from_map(map))
    }

    #[test]
    fn root_contains_env_keys() {
        let env = make_env(&["HOME", "PATH"]);
        let root = ShallowLayeredEnv::root(&env);
        assert!(root.contains("HOME"));
        assert!(root.contains("PATH"));
        assert!(!root.contains("MISSING"));
    }

    #[test]
    fn child_sees_own_and_parent() {
        let env = make_env(&["BASE"]);
        let root = Arc::new(ShallowLayeredEnv::root(&env));
        let child = ShallowLayeredEnv::child(root, ["OVERLAY".to_string()]);
        assert!(child.contains("BASE"));
        assert!(child.contains("OVERLAY"));
        assert!(!child.contains("MISSING"));
    }

    #[test]
    fn with_name_adds_single_binding() {
        let env = make_env(&["BASE"]);
        let root = Arc::new(ShallowLayeredEnv::root(&env));
        let extended = ShallowLayeredEnv::with_name(&root, "FOO".to_string());
        assert!(extended.contains("BASE"));
        assert!(extended.contains("FOO"));
        assert!(!extended.contains("BAR"));
    }

    #[test]
    fn three_level_chain() {
        let env = make_env(&["L0"]);
        let l0 = Arc::new(ShallowLayeredEnv::root(&env));
        let l1 = Arc::new(ShallowLayeredEnv::child(l0, ["L1".to_string()]));
        let l2 = ShallowLayeredEnv::child(l1, ["L2".to_string()]);
        assert!(l2.contains("L0"));
        assert!(l2.contains("L1"));
        assert!(l2.contains("L2"));
        assert!(!l2.contains("L3"));
    }
}
