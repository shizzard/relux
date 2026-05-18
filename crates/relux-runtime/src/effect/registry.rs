use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::Notify;

use crate::observe::structured::SpanId;
use crate::report::result::ExecError;
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

    /// Stable mnemonic computed from the dedup identity. Same key →
    /// same marker; two acquires of the same effect-instance (one
    /// bootstrap + N dedup'd reuses) all share this string.
    pub fn marker(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        std::hash::Hash::hash(self, &mut hasher);
        relux_core::diagnostics::format_mnemonic(hasher.finish())
    }
}

// ─── ShellInstanceKey ───────────────────────────────────────

/// Stable identity for a shell, regardless of how the shell is
/// renamed by `reset_for_export` later in its lifetime. The marker
/// hashed from this key is stored on the VM at spawn time and
/// threaded through every shell-bearing event and buffer event.
///
/// `Effect`: shells owned by an effect instance (`shell foo { ... }`
/// inside an effect body), including the synthetic `__cleanup`
/// shell each effect cleanup opens. Identity composes the dedup
/// `EffectInstanceKey` with the shell's local name.
///
/// `Test`: shells owned directly by the test span (`shell foo { ... }`
/// at test scope, plus the synthetic test-level `__cleanup`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShellInstanceKey {
    Effect {
        effect: EffectInstanceKey,
        shell_name: String,
    },
    Test {
        shell_name: String,
    },
}

impl ShellInstanceKey {
    /// Stable mnemonic computed from the identity. Same key → same
    /// marker across runs.
    pub fn marker(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        std::hash::Hash::hash(self, &mut hasher);
        relux_core::diagnostics::format_mnemonic(hasher.finish())
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
    /// Guards held for each acquired dependency. Dropping a guard via
    /// `release_and_teardown` decrements the dep's refcount; the last
    /// holder triggers the dep's cleanup body.
    pub dep_guards: Vec<EffectGuard>,
    pub cleanup: Option<IrCleanupBlock>,
    /// The `EffectSetup` span this handle represents. Threaded into the
    /// `EffectCleanup` span at teardown so the viewer can resolve a
    /// cleanup shell's scope back to the owning effect — cleanups
    /// themselves are now parented directly under the test span, so this
    /// is the only link from cleanup back to the originating setup.
    pub setup_span: SpanId,
    /// Dedup key for this effect instance. Needed at cleanup time to
    /// derive a `ShellInstanceKey::Effect` for the synthetic
    /// `__cleanup` shell. `marker` below is `key.marker()`,
    /// pre-computed at construction.
    pub key: EffectInstanceKey,
    /// Identity marker mirrored from the dedup key. Threaded into every
    /// `EffectCleanup` span this handle drives at teardown so partner
    /// lookup by marker works without re-deriving from the key.
    pub marker: String,
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
    /// Bootstrap is in flight on another task. Acquirers that hit
    /// this state clone the `Notify`, drop the slot lock, and await
    /// `notified()`; the bootstrapping task transitions the slot to
    /// `Ready` or `Failed` and calls `notify_waiters()`.
    Loading(Arc<Notify>),
    Ready {
        refcount: usize,
        handle: Box<EffectHandle>,
    },
    Failed(ExecError),
}

// EffectGuard

/// What `EffectGuard::release` returns.
///
/// - `LastHolder` — refcount went to zero. Caller takes ownership of
///   the handle and runs the cleanup body.
/// - `Deferred` — refcount stayed positive. Caller emits a
///   zero-duration deferred `EffectCleanup` span using the supplied
///   metadata; the actual cleanup span (and body) will be opened by a
///   later releaser.
/// - `Drift` — slot wasn't `Ready` (only reachable on a bug: a guard
///   was released against a slot it doesn't belong to). A
///   `debug_assert!` fires inside `release`; release builds short-circuit.
pub enum ReleaseOutcome {
    LastHolder {
        handle: Box<EffectHandle>,
    },
    Deferred {
        effect: String,
        alias: Option<String>,
        setup_span: SpanId,
        marker: String,
    },
    Drift,
}

/// Outstanding handle on one acquired refcount of an `EffectSlot`.
///
/// Constructed only by `EffectManager::acquire` via `EffectGuard::new`
/// (one guard per successful acquire, including dedup hits). Consumed
/// by `release`, which atomically decrements the slot's refcount under
/// the slot mutex and returns the `EffectHandle` to the releaser whose
/// decrement hit zero (i.e. exactly once per fully-acquired slot).
///
/// Not `Clone`, not `Copy`, with a private `slot` field: the
/// type-level non-cloneability plus the crate-internal-only
/// constructor keep refcount and outstanding-guard count in lockstep.
pub struct EffectGuard {
    slot: Arc<TokioMutex<EffectSlot>>,
}

impl EffectGuard {
    /// Crate-internal constructor. Caller MUST have just incremented
    /// (or initialized to 1) the refcount on the slot this guard
    /// points at; otherwise the refcount and outstanding-guard count
    /// will drift.
    pub(crate) fn new(slot: Arc<TokioMutex<EffectSlot>>) -> Self {
        Self { slot }
    }

    /// Atomic decrement-and-take under the slot mutex.
    ///
    /// Returns `LastHolder { handle }` exactly once per slot (the call
    /// that drove `refcount` from 1 to 0) and `Deferred { ... }` from
    /// every other release on the same slot, carrying the metadata the
    /// caller needs to emit a zero-duration deferred-cleanup span.
    /// The handle is moved out of the slot; the slot becomes
    /// `EffectSlot::Empty`. Callers run the returned handle's cleanup
    /// body *after* this method returns, so the slot mutex is not held
    /// during cleanup.
    pub async fn release(self) -> ReleaseOutcome {
        let mut guard = self.slot.lock().await;
        match &mut *guard {
            EffectSlot::Ready { refcount, handle } => {
                *refcount -= 1;
                if *refcount == 0 {
                    let taken = std::mem::replace(&mut *guard, EffectSlot::Empty);
                    match taken {
                        EffectSlot::Ready { handle, .. } => ReleaseOutcome::LastHolder { handle },
                        _ => unreachable!("matched Ready above"),
                    }
                } else {
                    ReleaseOutcome::Deferred {
                        effect: handle.scope.name().to_string(),
                        alias: handle.alias.clone(),
                        setup_span: handle.setup_span,
                        marker: handle.marker.clone(),
                    }
                }
            }
            EffectSlot::Empty | EffectSlot::Loading(_) | EffectSlot::Failed(_) => {
                debug_assert!(
                    false,
                    r#"EffectGuard::release on non-Ready slot indicates a refcount/outstanding-guard drift; every guard must point at a Ready slot until it is consumed"#,
                );
                ReleaseOutcome::Drift
            }
        }
    }
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

    fn stub_handle() -> EffectHandle {
        use crate::vm::context::Scope;
        use relux_core::pure::VarScope;
        EffectHandle {
            scope: Scope::Test {
                name: "stub".into(),
                vars: Arc::new(TokioMutex::new(VarScope::new())),
                timeout: None,
            },
            shells: HashMap::new(),
            exposed: HashSet::new(),
            exposed_vars: HashMap::new(),
            dep_guards: Vec::new(),
            cleanup: None,
            setup_span: 0u64,
            key: test_key("stub"),
            marker: "stub-marker-0000".into(),
            alias: None,
        }
    }

    fn ready_slot(refcount: usize) -> Arc<TokioMutex<EffectSlot>> {
        Arc::new(TokioMutex::new(EffectSlot::Ready {
            refcount,
            handle: Box::new(stub_handle()),
        }))
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

    #[tokio::test]
    async fn release_deferred_carries_metadata_and_decrements() {
        let slot = ready_slot(2);
        let g = EffectGuard::new(slot.clone());
        match g.release().await {
            ReleaseOutcome::Deferred { marker, .. } => {
                assert_eq!(marker, "stub-marker-0000");
            }
            _ => panic!("non-last release should produce Deferred"),
        }
        let guard = slot.lock().await;
        match &*guard {
            EffectSlot::Ready { refcount, .. } => assert_eq!(*refcount, 1),
            _ => panic!("slot should remain Ready with refcount 1"),
        }
    }

    #[tokio::test]
    async fn release_last_holder_returns_handle_and_empties_slot() {
        let slot = ready_slot(1);
        let g = EffectGuard::new(slot.clone());
        match g.release().await {
            ReleaseOutcome::LastHolder { handle } => {
                assert_eq!(handle.marker, "stub-marker-0000");
            }
            _ => panic!("last release should produce LastHolder"),
        }
        let guard = slot.lock().await;
        assert!(matches!(*guard, EffectSlot::Empty), "slot should be Empty");
    }

    #[tokio::test]
    async fn concurrent_releases_serialize_via_slot_mutex() {
        let slot = ready_slot(2);
        let g1 = EffectGuard::new(slot.clone());
        let g2 = EffectGuard::new(slot.clone());
        let (a, b) = tokio::join!(g1.release(), g2.release());
        let mut last_holder = 0usize;
        let mut deferred = 0usize;
        for outcome in [a, b] {
            match outcome {
                ReleaseOutcome::LastHolder { .. } => last_holder += 1,
                ReleaseOutcome::Deferred { .. } => deferred += 1,
                ReleaseOutcome::Drift => panic!("unexpected Drift"),
            }
        }
        assert_eq!(last_holder, 1, "exactly one releaser is the last holder");
        assert_eq!(deferred, 1, "exactly one releaser is deferred");
        let guard = slot.lock().await;
        assert!(matches!(*guard, EffectSlot::Empty));
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
    fn marker_is_stable_for_same_key() {
        let k = test_key("FX");
        assert_eq!(k.marker(), k.marker());
    }

    #[test]
    fn marker_differs_for_different_overlay() {
        let a = test_key_with_overlay("FX", "alpha");
        let b = test_key_with_overlay("FX", "beta");
        assert_ne!(a.marker(), b.marker());
    }

    #[test]
    fn marker_matches_mnemonic_format() {
        let k = test_key("FX");
        let m = k.marker();
        let parts: Vec<&str> = m.split('-').collect();
        assert_eq!(parts.len(), 3, "marker {m:?} should be adj-noun-NNNN");
        assert!(parts[0].chars().all(|c| c.is_ascii_lowercase()));
        assert!(parts[1].chars().all(|c| c.is_ascii_lowercase()));
        assert_eq!(parts[2].len(), 4);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn shell_key_effect_marker_is_stable() {
        let key = ShellInstanceKey::Effect {
            effect: test_key("Db"),
            shell_name: "redis".into(),
        };
        assert_eq!(key.marker(), key.marker());
    }

    #[test]
    fn shell_key_test_marker_is_stable() {
        let key = ShellInstanceKey::Test {
            shell_name: "default".into(),
        };
        assert_eq!(key.marker(), key.marker());
    }

    #[test]
    fn shell_key_marker_matches_mnemonic_format() {
        let key = ShellInstanceKey::Test {
            shell_name: "default".into(),
        };
        let m = key.marker();
        let parts: Vec<&str> = m.split('-').collect();
        assert_eq!(parts.len(), 3, "marker {m:?} should be adj-noun-NNNN");
        assert_eq!(parts[2].len(), 4);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn shell_key_effect_vs_test_dont_collide_for_same_name() {
        let effect_key = ShellInstanceKey::Effect {
            effect: test_key("Db"),
            shell_name: "default".into(),
        };
        let test_key_shell = ShellInstanceKey::Test {
            shell_name: "default".into(),
        };
        assert_ne!(effect_key.marker(), test_key_shell.marker());
    }

    #[test]
    fn shell_key_different_shell_names_produce_distinct_markers() {
        let a = ShellInstanceKey::Effect {
            effect: test_key("Db"),
            shell_name: "redis".into(),
        };
        let b = ShellInstanceKey::Effect {
            effect: test_key("Db"),
            shell_name: "postgres".into(),
        };
        assert_ne!(a.marker(), b.marker());
    }

    #[test]
    fn shell_key_cleanup_distinct_per_effect_instance() {
        let a = ShellInstanceKey::Effect {
            effect: test_key("Db"),
            shell_name: "__cleanup".into(),
        };
        let b = ShellInstanceKey::Effect {
            effect: test_key("Redis"),
            shell_name: "__cleanup".into(),
        };
        assert_ne!(a.marker(), b.marker());
    }
}
