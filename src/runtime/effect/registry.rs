use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::diagnostics::EffectId as DiagEffectId;
use crate::dsl::resolver::ir::IrCleanupBlock;
use crate::dsl::resolver::ir::IrEffectNeed;
use crate::runtime::report::result::Failure;
use crate::runtime::vm::Vm;
use crate::runtime::vm::context::Scope;

// ─── EffectInstanceKey ──────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectInstanceKey {
    pub effect_id: DiagEffectId,
    pub canonical_overlay: String,
}

impl From<&IrEffectNeed> for EffectInstanceKey {
    fn from(need: &IrEffectNeed) -> Self {
        Self {
            effect_id: need.effect().clone(),
            canonical_overlay: need.canonical_overlay().to_string(),
        }
    }
}

// ─── EffectHandle ───────────────────────────────────────────

pub struct EffectHandle {
    pub scope: Scope,
    pub exported_vm: Arc<TokioMutex<Vm>>,
    pub dependencies: Vec<EffectInstanceKey>,
    pub cleanup: Option<IrCleanupBlock>,
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
            canonical_overlay: String::new(),
        }
    }

    fn test_key_with_overlay(name: &str, overlay: &str) -> EffectInstanceKey {
        EffectInstanceKey {
            effect_id: DiagEffectId {
                module: crate::diagnostics::ModulePath("test.relux".into()),
                name: crate::diagnostics::EffectName(name.to_string()),
            },
            canonical_overlay: overlay.to_string(),
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
}
