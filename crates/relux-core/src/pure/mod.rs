use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::Arc;

pub mod bifs;

// --- VarScope --------------------------------------------

/// A single variable scope - flat name->value mapping.
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

    /// Assign a new value to an existing key. Returns `Some(prev)` with the
    /// prior value if the key existed (and was updated), `None` if the key
    /// was not found.
    pub fn assign(&mut self, key: &str, value: String) -> Option<String> {
        let slot = self.vars.get_mut(key)?;
        Some(std::mem::replace(slot, value))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

// --- Env -------------------------------------------------

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

// --- LayeredEnvSource ------------------------------------

/// Provenance of a single `LayeredEnv` layer: where its values came from.
/// One tag per layer; because a lookup returns the first hit walking the
/// chain, the winning value's source is always recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayeredEnvSource {
    /// Process environment captured at startup (lowest precedence).
    Base,
    /// Values resolved from one `.env` file.
    DotEnv(PathBuf),
    /// `__RELUX_*` run-level internals.
    ReluxInternal,
    /// Effect-contributed overlay, tagged with the effect instance mnemonic.
    EffectOverlay(String),
    /// Test-contributed overlay: `let` bindings and `__RELUX_TEST_*`.
    Test,
}

// --- LayeredEnv ------------------------------------------

/// Layered environment with recursive parent chain.
///
/// Each layer holds a small overlay (`own`) and points to a parent
/// `LayeredEnv`. The root layer wraps the base process environment
/// with no parent. Lookups walk the chain: own -> parent -> grandparent -> ...
///
/// No cloning of the base env - each layer is `Arc`-shared.
#[derive(Debug, Clone)]
pub struct LayeredEnv {
    own: Env,
    source: LayeredEnvSource,
    parent: Option<Arc<LayeredEnv>>,
    /// Cached cumulative hash of this layer folded with the parent chain.
    /// Computed once at construction (layers are immutable), so the top
    /// layer's value is the whole-stack identity, O(1) to read.
    cumulative_hash: u64,
}

impl LayeredEnv {
    /// Hash one layer: its source tag plus its `(key, value)` pairs folded
    /// in sorted-key order (a `HashMap`'s iteration order is nondeterministic,
    /// so sorting is required for a reproducible hash).
    fn layer_hash(source: &LayeredEnvSource, own: &Env) -> u64 {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let mut pairs: Vec<(&str, &str)> = own.iter().collect();
        pairs.sort_unstable_by_key(|(k, _)| *k);
        for (k, v) in pairs {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Create the root layer from the base process environment (source `Base`).
    pub fn root(base: Env) -> Self {
        Self::root_with_source(base, LayeredEnvSource::Base)
    }

    /// Create the root layer with an explicit source.
    pub fn root_with_source(base: Env, source: LayeredEnvSource) -> Self {
        let cumulative_hash = Self::layer_hash(&source, &base);
        Self {
            own: base,
            source,
            parent: None,
            cumulative_hash,
        }
    }

    /// Create a child overlay on top of this env (source `Test` by default,
    /// the common case; effect overlays use `child_with_source`).
    pub fn child(parent: Arc<LayeredEnv>, overlay: Env) -> Self {
        Self::child_with_source(parent, overlay, LayeredEnvSource::Test)
    }

    /// Create a child overlay with an explicit source.
    pub fn child_with_source(
        parent: Arc<LayeredEnv>,
        overlay: Env,
        source: LayeredEnvSource,
    ) -> Self {
        let own_hash = Self::layer_hash(&source, &overlay);
        let mut hasher = DefaultHasher::new();
        own_hash.hash(&mut hasher);
        parent.cumulative_hash.hash(&mut hasher);
        let cumulative_hash = hasher.finish();
        Self {
            own: overlay,
            source,
            parent: Some(parent),
            cumulative_hash,
        }
    }

    /// This layer's provenance.
    pub fn source(&self) -> &LayeredEnvSource {
        &self.source
    }

    /// The whole-stack identity hash: deterministic, insertion-order
    /// independent, sensitive to every layer's source and values.
    pub fn stack_hash(&self) -> u64 {
        self.cumulative_hash
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

    /// Iterate all entries across all layers, tagging each with the source of
    /// the layer that owns the winning value. Closest layer wins on duplicates,
    /// so the tag is the winning value's provenance. Walk order matches `iter()`.
    pub fn iter_with_source(&self) -> impl Iterator<Item = (&str, &str, &LayeredEnvSource)> {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut current = Some(self);
        while let Some(layer) = current {
            for (k, v) in layer.own.iter() {
                if seen.insert(k) {
                    entries.push((k, v, &layer.source));
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

// --- LayeredEnvBuilder -----------------------------------

/// Mutable staging type for building one `LayeredEnv` layer incrementally
/// while resolving names against it. Used by `.env` parsing: `${VAR}` resolves
/// via `get()` (this layer's `own`, then the parent chain), and each parsed
/// line is recorded via `insert()`. `build()` seals into an immutable
/// `LayeredEnv` with its cumulative hash computed once - so `LayeredEnv` itself
/// never needs a post-construction mutator (which would stale the cached hash).
#[derive(Debug, Clone)]
pub struct LayeredEnvBuilder {
    own: Env,
    source: LayeredEnvSource,
    parent: Arc<LayeredEnv>,
}

impl LayeredEnvBuilder {
    /// Start a new layer over `parent` with the given `source`.
    pub fn new(parent: Arc<LayeredEnv>, source: LayeredEnvSource) -> Self {
        Self {
            own: Env::new(),
            source,
            parent,
        }
    }

    /// Resolve a name against this layer's own values first, then the parent
    /// chain. Returns `None` if unbound anywhere.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.own.get(key).or_else(|| self.parent.get(key))
    }

    /// Record a value into the layer under construction.
    pub fn insert(&mut self, key: String, value: String) {
        self.own.insert(key, value);
    }

    /// Seal into an immutable layer, computing the cumulative hash once.
    pub fn build(self) -> LayeredEnv {
        LayeredEnv::child_with_source(self.parent, self.own, self.source)
    }
}

// --- Tests -----------------------------------------------

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
    fn var_scope_assign_existing_returns_previous() {
        let mut s = VarScope::new();
        s.insert("x".into(), "old".into());
        assert_eq!(s.assign("x", "new".into()), Some("old".into()));
        assert_eq!(s.get("x"), Some("new"));
    }

    #[test]
    fn var_scope_assign_missing_returns_none() {
        let mut s = VarScope::new();
        assert_eq!(s.assign("x", "val".into()), None);
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

    // --- Env tests ---------------------------------------

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

    // --- LayeredEnv tests --------------------------------

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

    // --- From<Env> ---------------------------------------

    #[test]
    fn from_env_creates_root() {
        let mut env = Env::new();
        env.insert("K".into(), "V".into());
        let layered: LayeredEnv = env.into();
        assert_eq!(layered.get("K"), Some("V"));
    }

    // --- iter() tests ------------------------------------

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

    #[test]
    fn iter_with_source_tags_winning_layer() {
        let mut base = Env::new();
        base.insert("HOME".into(), "/h".into());
        base.insert("SHARED".into(), "base".into());
        let root = Arc::new(LayeredEnv::root(base)); // source Base

        let mut ov = Env::new();
        ov.insert("PORT".into(), "5432".into());
        ov.insert("SHARED".into(), "dot".into());
        let child =
            LayeredEnv::child_with_source(root, ov, LayeredEnvSource::DotEnv("/p/.env".into()));

        let got: HashMap<&str, (&str, LayeredEnvSource)> = child
            .iter_with_source()
            .map(|(k, v, s)| (k, (v, s.clone())))
            .collect();

        assert_eq!(got["HOME"], ("/h", LayeredEnvSource::Base));
        assert_eq!(
            got["PORT"],
            ("5432", LayeredEnvSource::DotEnv("/p/.env".into()))
        );
        // Closest layer wins: SHARED resolves to the DotEnv layer's value + source.
        assert_eq!(
            got["SHARED"],
            ("dot", LayeredEnvSource::DotEnv("/p/.env".into()))
        );
    }

    // --- LayeredEnvSource + stack hash -----------------------

    #[test]
    fn layered_root_source_defaults_to_base() {
        let env = LayeredEnv::root(Env::new());
        assert_eq!(env.source(), &LayeredEnvSource::Base);
    }

    #[test]
    fn layered_child_source_defaults_to_test() {
        let parent = Arc::new(LayeredEnv::root(Env::new()));
        let child = LayeredEnv::child(parent, Env::new());
        assert_eq!(child.source(), &LayeredEnvSource::Test);
    }

    #[test]
    fn child_with_source_records_effect_overlay() {
        let parent = Arc::new(LayeredEnv::root(Env::new()));
        let child = LayeredEnv::child_with_source(
            parent,
            Env::new(),
            LayeredEnvSource::EffectOverlay("brave-yak-0001".into()),
        );
        assert_eq!(
            child.source(),
            &LayeredEnvSource::EffectOverlay("brave-yak-0001".into())
        );
    }

    #[test]
    fn stack_hash_independent_of_insertion_order() {
        let mut a = Env::new();
        a.insert("X".into(), "1".into());
        a.insert("Y".into(), "2".into());
        let mut b = Env::new();
        b.insert("Y".into(), "2".into());
        b.insert("X".into(), "1".into());
        assert_eq!(
            LayeredEnv::root(a).stack_hash(),
            LayeredEnv::root(b).stack_hash()
        );
    }

    #[test]
    fn stack_hash_distinguishes_source() {
        let mut a = Env::new();
        a.insert("X".into(), "1".into());
        let mut b = Env::new();
        b.insert("X".into(), "1".into());
        let base = LayeredEnv::root_with_source(a, LayeredEnvSource::Base);
        let test = LayeredEnv::root_with_source(b, LayeredEnvSource::Test);
        assert_ne!(base.stack_hash(), test.stack_hash());
    }

    #[test]
    fn stack_hash_distinguishes_child_source() {
        let parent = Arc::new(LayeredEnv::root(Env::new()));
        let mut o1 = Env::new();
        o1.insert("A".into(), "x".into());
        let mut o2 = Env::new();
        o2.insert("A".into(), "x".into());
        let as_test = LayeredEnv::child_with_source(parent.clone(), o1, LayeredEnvSource::Test);
        let as_effect = LayeredEnv::child_with_source(
            parent.clone(),
            o2,
            LayeredEnvSource::EffectOverlay("db".into()),
        );
        assert_ne!(as_test.stack_hash(), as_effect.stack_hash());
    }

    #[test]
    fn sibling_children_over_same_parent_share_stack_hash() {
        let parent = Arc::new(LayeredEnv::root(Env::new()));
        let mut o1 = Env::new();
        o1.insert("A".into(), "x".into());
        let mut o2 = Env::new();
        o2.insert("A".into(), "x".into());
        let c1 = LayeredEnv::child(parent.clone(), o1);
        let c2 = LayeredEnv::child(parent.clone(), o2);
        assert_eq!(c1.stack_hash(), c2.stack_hash());
        assert_ne!(c1.stack_hash(), parent.stack_hash());
    }

    #[test]
    fn child_hash_reflects_parent() {
        let ov = || {
            let mut e = Env::new();
            e.insert("K".into(), "v".into());
            e
        };
        let mut p1 = Env::new();
        p1.insert("P".into(), "1".into());
        let mut p2 = Env::new();
        p2.insert("P".into(), "2".into());
        let c1 = LayeredEnv::child(Arc::new(LayeredEnv::root(p1)), ov());
        let c2 = LayeredEnv::child(Arc::new(LayeredEnv::root(p2)), ov());
        assert_ne!(c1.stack_hash(), c2.stack_hash());
    }

    #[test]
    fn stack_hash_stable_across_reconstruction() {
        let build = || {
            let mut base = Env::new();
            base.insert("HOME".into(), "/h".into());
            let root = Arc::new(LayeredEnv::root(base));
            let mut ov = Env::new();
            ov.insert("K".into(), "v".into());
            LayeredEnv::child(root, ov).stack_hash()
        };
        assert_eq!(build(), build());
    }

    // --- LayeredEnvBuilder -----------------------------------

    #[test]
    fn builder_get_prefers_own_then_parent() {
        let mut parent_env = Env::new();
        parent_env.insert("A".into(), "parent-a".into());
        parent_env.insert("B".into(), "parent-b".into());
        let parent = Arc::new(LayeredEnv::root(parent_env));

        let mut b = LayeredEnvBuilder::new(parent, LayeredEnvSource::DotEnv("x/.env".into()));
        b.insert("A".into(), "own-a".into());
        assert_eq!(b.get("A"), Some("own-a")); // own shadows parent
        assert_eq!(b.get("B"), Some("parent-b")); // falls through to parent
        assert_eq!(b.get("MISSING"), None);
    }

    #[test]
    fn builder_build_seals_layer_with_source_and_hash() {
        let parent = Arc::new(LayeredEnv::root(Env::new()));
        let mut b =
            LayeredEnvBuilder::new(parent.clone(), LayeredEnvSource::DotEnv("x/.env".into()));
        b.insert("K".into(), "v".into());
        let layer = b.build();
        assert_eq!(layer.source(), &LayeredEnvSource::DotEnv("x/.env".into()));
        assert_eq!(layer.get("K"), Some("v"));
        // Sealed layer's hash equals a directly-constructed equivalent layer.
        let mut own = Env::new();
        own.insert("K".into(), "v".into());
        let direct =
            LayeredEnv::child_with_source(parent, own, LayeredEnvSource::DotEnv("x/.env".into()));
        assert_eq!(layer.stack_hash(), direct.stack_hash());
    }
}
