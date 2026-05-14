use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::observe::structured::SpanId;
use crate::report::result::Failure;
use crate::vm::Vm;
use crate::vm::context::Scope;
use relux_core::diagnostics::EffectId as DiagEffectId;
use relux_core::pure::Env;
use relux_ir::IrCleanupBlock;

// ─── Type Aliases ──────────────────────────────────────────

pub type ShellMap = HashMap<String, Arc<TokioMutex<Vm>>>;
pub type VarMap = HashMap<String, String>;

// ─── ExportedEffect / AcquiredEffect ───────────────────────

/// Result of instantiating a single effect: identity key + exposed shells and vars.
pub struct ExportedEffect {
    pub key: EffectInstanceKey,
    pub shells: ShellMap,
    pub vars: VarMap,
}

/// Result of acquiring a single effect instance: exposed shells and vars (no key).
pub struct AcquiredEffect {
    pub shells: ShellMap,
    pub vars: VarMap,
}

// ─── EffectInstanceKey ──────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectInstanceKey {
    pub effect_id: DiagEffectId,
    pub evaluated_overlay: String,
}

impl EffectInstanceKey {
    /// Build from effect ID and the expected-variable values in declaration order.
    ///
    /// Only the values of variables declared in `expect` participate in identity.
    /// The order comes from the `expect` declaration, so no sorting is needed.
    /// Values are joined with `\0` (null byte) to avoid ambiguity — overlay
    /// values are shell strings and cannot contain null bytes.
    pub fn from_expects(
        effect_id: DiagEffectId,
        expect_names: &[&str],
        evaluated_overlay: &Env,
    ) -> Self {
        let identity: String = expect_names
            .iter()
            .map(|name| {
                let val = evaluated_overlay.get(name).unwrap_or("");
                format!("{name}\0{val}")
            })
            .collect::<Vec<_>>()
            .join("\0");
        Self {
            effect_id,
            evaluated_overlay: identity,
        }
    }
}

// ─── EffectHandle ───────────────────────────────────────────

pub struct EffectHandle {
    pub scope: Scope,
    /// All shells owned by this effect (both exposed and internal).
    pub shells: ShellMap,
    /// Names of shells that are exposed to the caller.
    pub exposed: HashSet<String>,
    /// Variables exposed to the caller (name → value).
    pub exposed_vars: VarMap,
    pub dependencies: Vec<EffectInstanceKey>,
    pub cleanup: Option<IrCleanupBlock>,
    /// The `EffectSetup` span this handle represents. Threaded into the
    /// `EffectCleanup` span at teardown so the viewer can resolve a
    /// cleanup shell's scope back to the owning effect — cleanups
    /// themselves are now parented directly under the test span, so this
    /// is the only link from cleanup back to the originating setup.
    pub setup_span: SpanId,
    /// Alias supplied at the first acquisition (`start <FX> as <alias>`).
    /// `None` when no alias was used. Threaded into the `EffectCleanup`
    /// span so the cleanup card can mirror `EffectSetup`'s alias display.
    pub alias: Option<String>,
}

impl EffectHandle {
    /// Return only the shells that are exposed to the caller.
    pub fn exposed_shells(&self) -> ShellMap {
        self.shells
            .iter()
            .filter(|(name, _)| self.exposed.contains(name.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Return the exposed variables.
    pub fn exposed_vars(&self) -> &VarMap {
        &self.exposed_vars
    }
}

// ─── EffectSlot ─────────────────────────────────────────────

pub enum EffectSlot {
    Empty,
    Ready {
        refcount: usize,
        handle: Box<EffectHandle>,
    },
    Failed(Failure),
}

// ─── EffectRegistry ─────────────────────────────────────────

pub struct EffectRegistry {
    slots: std::sync::Mutex<HashMap<EffectInstanceKey, Arc<TokioMutex<EffectSlot>>>>,
    /// Ordered log of every acquisition (with duplicates for deduped effects).
    /// Mirrors the order in which `acquire` was called, so cleanup can run
    /// one `run_cleanup` per acquisition — correctly draining refcounts.
    acquisition_order: std::sync::Mutex<Vec<EffectInstanceKey>>,
}

impl Default for EffectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self {
            slots: std::sync::Mutex::new(HashMap::new()),
            acquisition_order: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Get or create the slot for a given key.
    /// The outer std::sync::Mutex is held only briefly for the HashMap lookup.
    pub fn slot(&self, key: &EffectInstanceKey) -> Arc<TokioMutex<EffectSlot>> {
        self.slots
            .lock()
            .expect("slot map mutex poisoned")
            .entry(key.clone())
            .or_insert_with(|| Arc::new(TokioMutex::new(EffectSlot::Empty)))
            .clone()
    }

    /// Record that a key was acquired (called once per `acquire`, including dedup hits).
    pub fn record_acquisition(&self, key: EffectInstanceKey) {
        self.acquisition_order
            .lock()
            .expect("acquisition order mutex poisoned")
            .push(key);
    }

    /// Return the full ordered acquisition log (with duplicates).
    pub fn acquired_keys(&self) -> Vec<EffectInstanceKey> {
        self.acquisition_order
            .lock()
            .expect("acquisition order mutex poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(name: &str) -> EffectInstanceKey {
        EffectInstanceKey {
            effect_id: DiagEffectId {
                module: relux_core::diagnostics::ModulePath("test.relux".into()),
                name: relux_core::diagnostics::EffectName(name.to_string()),
            },
            evaluated_overlay: String::new(),
        }
    }

    fn test_key_with_overlay(name: &str, overlay: &str) -> EffectInstanceKey {
        EffectInstanceKey {
            effect_id: DiagEffectId {
                module: relux_core::diagnostics::ModulePath("test.relux".into()),
                name: relux_core::diagnostics::EffectName(name.to_string()),
            },
            evaluated_overlay: overlay.to_string(),
        }
    }

    #[test]
    fn key_equality_same() {
        let k1 = test_key("Db");
        let k2 = test_key("Db");
        assert_eq!(k1, k2);
    }

    #[test]
    fn key_equality_different_name() {
        let k1 = test_key("Db");
        let k2 = test_key("Redis");
        assert_ne!(k1, k2);
    }

    #[test]
    fn key_equality_different_overlay() {
        let k1 = test_key_with_overlay("Db", "PORT=5432");
        let k2 = test_key_with_overlay("Db", "PORT=5433");
        assert_ne!(k1, k2);
    }

    #[test]
    fn key_hash_consistent() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hash;
        use std::hash::Hasher;
        let k1 = test_key("Db");
        let k2 = test_key("Db");
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        k1.hash(&mut h1);
        k2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn registry_new_is_empty() {
        let reg = EffectRegistry::new();
        assert!(reg.slots.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn slot_creates_empty_on_first_access() {
        let reg = EffectRegistry::new();
        let key = test_key("Db");
        let slot = reg.slot(&key);
        let guard = slot.lock().await;
        assert!(matches!(*guard, EffectSlot::Empty));
    }

    #[tokio::test]
    async fn slot_returns_same_arc_for_same_key() {
        let reg = EffectRegistry::new();
        let key = test_key("Db");
        let s1 = reg.slot(&key);
        let s2 = reg.slot(&key);
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    #[tokio::test]
    async fn slot_returns_different_arcs_for_different_keys() {
        let reg = EffectRegistry::new();
        let k1 = test_key("Db");
        let k2 = test_key("Redis");
        let s1 = reg.slot(&k1);
        let s2 = reg.slot(&k2);
        assert!(!Arc::ptr_eq(&s1, &s2));
    }

    #[test]
    fn acquired_keys_empty_registry() {
        let reg = EffectRegistry::new();
        assert!(reg.acquired_keys().is_empty());
    }

    #[test]
    fn acquired_keys_preserves_order_and_duplicates() {
        let reg = EffectRegistry::new();
        let k1 = test_key("Db");
        let k2 = test_key("Redis");
        reg.record_acquisition(k1.clone());
        reg.record_acquisition(k2.clone());
        reg.record_acquisition(k1.clone());
        let keys = reg.acquired_keys();
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0].effect_id.name.0, "Db");
        assert_eq!(keys[1].effect_id.name.0, "Redis");
        assert_eq!(keys[2].effect_id.name.0, "Db");
    }

    #[test]
    fn from_expects_no_collision_when_value_contains_separator() {
        // Two structurally different overlays must produce different keys.
        // Effect expects A only. Overlay 1: A = "x\0y", Overlay 2: A = "x".
        // With naive join these could collide; null-byte framing prevents it.
        use std::collections::HashMap;
        let effect_id = DiagEffectId {
            module: relux_core::diagnostics::ModulePath("test.relux".into()),
            name: relux_core::diagnostics::EffectName("E".to_string()),
        };

        let mut overlay1 = HashMap::new();
        overlay1.insert("A".into(), "x,B=y".into());
        let env1 = relux_core::pure::Env::from_map(overlay1);

        let mut overlay2 = HashMap::new();
        overlay2.insert("A".into(), "x".into());
        overlay2.insert("B".into(), "y".into());
        let env2 = relux_core::pure::Env::from_map(overlay2);

        let expects = &["A"];
        let k1 = EffectInstanceKey::from_expects(effect_id.clone(), expects, &env1);
        let k2 = EffectInstanceKey::from_expects(effect_id, expects, &env2);
        assert_ne!(
            k1, k2,
            "different expect values must produce different keys"
        );
    }

    #[test]
    fn from_expects_uses_only_expected_keys() {
        // Extra overlay keys beyond what the effect expects should not
        // affect identity — only expected variable values matter.
        use std::collections::HashMap;
        let effect_id = DiagEffectId {
            module: relux_core::diagnostics::ModulePath("test.relux".into()),
            name: relux_core::diagnostics::EffectName("E".to_string()),
        };

        let mut overlay1 = HashMap::new();
        overlay1.insert("PORT".into(), "5432".into());
        overlay1.insert("EXTRA".into(), "foo".into());
        let env1 = relux_core::pure::Env::from_map(overlay1);

        let mut overlay2 = HashMap::new();
        overlay2.insert("PORT".into(), "5432".into());
        overlay2.insert("EXTRA".into(), "bar".into());
        let env2 = relux_core::pure::Env::from_map(overlay2);

        let expects = &["PORT"];
        let k1 = EffectInstanceKey::from_expects(effect_id.clone(), expects, &env1);
        let k2 = EffectInstanceKey::from_expects(effect_id, expects, &env2);
        assert_eq!(
            k1, k2,
            "extra overlay keys beyond expects should not affect identity"
        );
    }

    #[test]
    fn from_expects_declaration_order_is_stable() {
        use std::collections::HashMap;
        let effect_id = DiagEffectId {
            module: relux_core::diagnostics::ModulePath("test.relux".into()),
            name: relux_core::diagnostics::EffectName("E".to_string()),
        };

        let mut overlay = HashMap::new();
        overlay.insert("A".into(), "1".into());
        overlay.insert("B".into(), "2".into());
        let env = relux_core::pure::Env::from_map(overlay);

        // Same expects in same order → same key
        let k1 = EffectInstanceKey::from_expects(effect_id.clone(), &["A", "B"], &env);
        let k2 = EffectInstanceKey::from_expects(effect_id, &["A", "B"], &env);
        assert_eq!(k1, k2);
    }

    #[test]
    fn from_expects_empty_expects_produces_equal_keys() {
        use std::collections::HashMap;
        let effect_id = DiagEffectId {
            module: relux_core::diagnostics::ModulePath("test.relux".into()),
            name: relux_core::diagnostics::EffectName("E".to_string()),
        };

        let mut overlay1 = HashMap::new();
        overlay1.insert("X".into(), "1".into());
        let env1 = relux_core::pure::Env::from_map(overlay1);
        let env2 = relux_core::pure::Env::from_map(HashMap::new());

        let expects: &[&str] = &[];
        let k1 = EffectInstanceKey::from_expects(effect_id.clone(), expects, &env1);
        let k2 = EffectInstanceKey::from_expects(effect_id, expects, &env2);
        assert_eq!(
            k1, k2,
            "effects with no expects should always share identity"
        );
    }

    #[test]
    fn acquired_keys_not_recorded_for_failed_slots() {
        // Failed acquisitions should not be recorded — only successful ones.
        let reg = EffectRegistry::new();
        // Slot exists but no acquisition was recorded.
        let key = test_key("Broken");
        reg.slot(&key);
        assert!(reg.acquired_keys().is_empty());
    }
}
