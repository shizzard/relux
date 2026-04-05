use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::diagnostics::EffectId as DiagEffectId;
use crate::dsl::resolver::ir::IrCleanupBlock;
use crate::pure::Env;
use crate::runtime::report::result::Failure;
use crate::runtime::vm::Vm;
use crate::runtime::vm::context::Scope;

// ─── EffectInstanceKey ──────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectInstanceKey {
    pub effect_id: DiagEffectId,
    pub evaluated_overlay: String,
}

impl EffectInstanceKey {
    /// Build from effect ID and evaluated overlay values (runtime identity).
    pub fn from_evaluated(effect_id: DiagEffectId, evaluated_overlay: &Env) -> Self {
        let mut parts: Vec<String> = evaluated_overlay
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        parts.sort();
        Self {
            effect_id,
            evaluated_overlay: parts.join(","),
        }
    }
}

// ─── EffectHandle ───────────────────────────────────────────

pub struct EffectHandle {
    pub scope: Scope,
    /// All shells owned by this effect (both exposed and internal).
    pub shells: HashMap<String, Arc<TokioMutex<Vm>>>,
    /// Names of shells that are exposed to the caller.
    pub exposed: HashSet<String>,
    pub dependencies: Vec<EffectInstanceKey>,
    pub cleanup: Option<IrCleanupBlock>,
}

impl EffectHandle {
    /// Return only the shells that are exposed to the caller.
    pub fn exposed_shells(&self) -> HashMap<String, Arc<TokioMutex<Vm>>> {
        self.shells
            .iter()
            .filter(|(name, _)| self.exposed.contains(name.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

// ─── EffectSlot ─────────────────────────────────────────────

pub enum EffectSlot {
    Empty,
    Ready {
        refcount: usize,
        handle: EffectHandle,
    },
    Failed(Failure),
}

// ─── EffectRegistry ─────────────────────────────────────────

pub struct EffectRegistry {
    slots: std::sync::Mutex<HashMap<EffectInstanceKey, Arc<TokioMutex<EffectSlot>>>>,
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

    /// Return all keys that have been registered (any state).
    pub fn all_keys(&self) -> Vec<EffectInstanceKey> {
        self.slots
            .lock()
            .expect("slot map mutex poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(name: &str) -> EffectInstanceKey {
        EffectInstanceKey {
            effect_id: DiagEffectId {
                module: crate::diagnostics::ModulePath("test.relux".into()),
                name: crate::diagnostics::EffectName(name.to_string()),
            },
            evaluated_overlay: String::new(),
        }
    }

    fn test_key_with_overlay(name: &str, overlay: &str) -> EffectInstanceKey {
        EffectInstanceKey {
            effect_id: DiagEffectId {
                module: crate::diagnostics::ModulePath("test.relux".into()),
                name: crate::diagnostics::EffectName(name.to_string()),
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
    fn all_keys_empty_registry() {
        let reg = EffectRegistry::new();
        assert!(reg.all_keys().is_empty());
    }

    #[tokio::test]
    async fn all_keys_returns_accessed_slots() {
        let reg = EffectRegistry::new();
        let k1 = test_key("Db");
        let k2 = test_key("Redis");
        reg.slot(&k1);
        reg.slot(&k2);
        let mut keys = reg.all_keys();
        keys.sort_by(|a, b| a.effect_id.name.0.cmp(&b.effect_id.name.0));
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].effect_id.name.0, "Db");
        assert_eq!(keys[1].effect_id.name.0, "Redis");
    }

    #[tokio::test]
    async fn all_keys_includes_failed_slots() {
        // Regression: cleanup_all must see Failed slots too
        // (they are no-ops but must be reachable)
        let reg = EffectRegistry::new();
        let key = test_key("Broken");
        let slot = reg.slot(&key);
        *slot.lock().await = EffectSlot::Failed(crate::runtime::report::result::Failure::Runtime {
            message: "test failure".into(),
            span: None,
            shell: None,
        });
        let keys = reg.all_keys();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].effect_id.name.0, "Broken");
    }
}
