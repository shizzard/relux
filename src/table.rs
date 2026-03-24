use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ─── SharedTable ────────────────────────────────────────────

/// Mutable shared table — populated incrementally, potentially from multiple threads.
pub struct SharedTable<K, V> {
    map: Arc<Mutex<HashMap<K, V>>>,
}

impl<K, V> SharedTable<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert(&self, key: K, value: V) {
        self.map.lock().unwrap().insert(key, value);
    }

    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.map.lock().unwrap().get(key).cloned()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.lock().unwrap().contains_key(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, V)>
    where
        K: Clone,
        V: Clone,
    {
        let map = self.map.lock().unwrap();
        map.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl<K, V> std::fmt::Debug for SharedTable<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedTable").finish_non_exhaustive()
    }
}

impl<K, V> Default for SharedTable<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Clone for SharedTable<K, V> {
    fn clone(&self) -> Self {
        Self {
            map: Arc::clone(&self.map),
        }
    }
}

impl<K, V> TryFrom<SharedTable<K, V>> for FrozenTable<K, V> {
    type Error = SharedTable<K, V>;

    fn try_from(shared: SharedTable<K, V>) -> Result<Self, Self::Error> {
        match Arc::try_unwrap(shared.map) {
            Ok(mutex) => Ok(FrozenTable {
                map: Arc::new(mutex.into_inner().unwrap()),
            }),
            Err(arc) => Err(SharedTable { map: arc }),
        }
    }
}

// ─── FrozenTable ────────────────────────────────────────────

/// Immutable shared table — frozen after population, shared for reads only.
#[derive(Debug)]
pub struct FrozenTable<K, V> {
    map: Arc<HashMap<K, V>>,
}

impl<K, V> FrozenTable<K, V>
where
    K: Eq + Hash,
{
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map.iter()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<K, V> Clone for FrozenTable<K, V> {
    fn clone(&self) -> Self {
        Self {
            map: Arc::clone(&self.map),
        }
    }
}

// ─── LocalTable ─────────────────────────────────────────────

/// Local name resolution table — maps local keys to global keys,
/// backed by a SharedTable for registry lookups.
pub struct LocalTable<K, GK, V> {
    locals: HashMap<K, GK>,
    registry: SharedTable<GK, V>,
}

impl<K, GK, V> LocalTable<K, GK, V>
where
    K: Eq + Hash,
    GK: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(registry: SharedTable<GK, V>) -> Self {
        Self {
            locals: HashMap::new(),
            registry,
        }
    }

    pub fn insert(&mut self, local_key: K, global_key: GK) {
        self.locals.insert(local_key, global_key);
    }

    pub fn get(&self, local_key: &K) -> Option<V> {
        let global_key = self.locals.get(local_key)?;
        self.registry.get(global_key)
    }

    /// Check if a local key has been mapped (regardless of whether
    /// the global key has a value in the registry yet).
    pub fn contains_local(&self, local_key: &K) -> bool {
        self.locals.contains_key(local_key)
    }

    /// Get the global key mapped to a local key, if any.
    pub fn get_global_key(&self, local_key: &K) -> Option<&GK> {
        self.locals.get(local_key)
    }
}

// ─── FileId ─────────────────────────────────────────────────

/// Absolute file path, used as the stable file identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId {
    path: PathBuf,
}

impl FileId {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

// ─── SourceFile ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    // ── SharedTable ─────────────────────────────────────────

    #[test]
    fn shared_table_insert_and_get() {
        let t = SharedTable::new();
        t.insert("a", 1);
        assert_eq!(t.get(&"a"), Some(1));
    }

    #[test]
    fn shared_table_get_missing_returns_none() {
        let t: SharedTable<&str, i32> = SharedTable::new();
        assert_eq!(t.get(&"x"), None);
    }

    #[test]
    fn shared_table_overwrite() {
        let t = SharedTable::new();
        t.insert("a", 1);
        t.insert("a", 2);
        assert_eq!(t.get(&"a"), Some(2));
    }

    #[test]
    fn shared_table_contains_true() {
        let t = SharedTable::new();
        t.insert("a", 1);
        assert!(t.contains(&"a"));
    }

    #[test]
    fn shared_table_contains_false() {
        let t: SharedTable<&str, i32> = SharedTable::new();
        assert!(!t.contains(&"a"));
    }

    #[test]
    fn shared_table_clone_shares_state() {
        let t = SharedTable::new();
        let t2 = t.clone();
        t.insert("a", 1);
        assert_eq!(t2.get(&"a"), Some(1));
    }

    #[test]
    fn shared_table_original_sees_clone_inserts() {
        let t = SharedTable::new();
        let t2 = t.clone();
        t2.insert("b", 42);
        assert_eq!(t.get(&"b"), Some(42));
    }

    #[test]
    fn shared_table_empty_new() {
        let t: SharedTable<String, String> = SharedTable::new();
        assert_eq!(t.get(&"anything".to_string()), None);
    }

    #[test]
    fn shared_table_multiple_keys() {
        let t = SharedTable::new();
        for i in 0..100 {
            t.insert(i, i * 10);
        }
        for i in 0..100 {
            assert_eq!(t.get(&i), Some(i * 10));
        }
    }

    #[test]
    fn shared_table_get_returns_clone() {
        let t = SharedTable::new();
        t.insert("a", vec![1, 2, 3]);
        let mut v = t.get(&"a").unwrap();
        v.push(4);
        assert_eq!(t.get(&"a").unwrap(), vec![1, 2, 3]);
    }

    // ── FrozenTable ─────────────────────────────────────────

    fn make_frozen(entries: Vec<(&str, i32)>) -> FrozenTable<String, i32> {
        let shared = SharedTable::new();
        for (k, v) in entries {
            shared.insert(k.to_string(), v);
        }
        FrozenTable::try_from(shared).unwrap()
    }

    #[test]
    fn frozen_table_get() {
        let t = make_frozen(vec![("a", 1), ("b", 2)]);
        assert_eq!(t.get(&"a".to_string()), Some(&1));
        assert_eq!(t.get(&"b".to_string()), Some(&2));
    }

    #[test]
    fn frozen_table_get_missing_returns_none() {
        let t = make_frozen(vec![("a", 1)]);
        assert_eq!(t.get(&"z".to_string()), None);
    }

    #[test]
    fn frozen_table_iter() {
        let t = make_frozen(vec![("a", 1), ("b", 2), ("c", 3)]);
        assert_eq!(t.iter().count(), 3);
    }

    #[test]
    fn frozen_table_len() {
        let t = make_frozen(vec![("a", 1), ("b", 2)]);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn frozen_table_len_empty() {
        let t = make_frozen(vec![]);
        assert_eq!(t.len(), 0);
        assert!(t.is_empty());
    }

    // ── TryFrom ─────────────────────────────────────────────

    #[test]
    fn try_from_shared_succeeds_when_unique() {
        let shared: SharedTable<String, i32> = SharedTable::new();
        shared.insert("a".to_string(), 1);
        let frozen = FrozenTable::try_from(shared);
        assert!(frozen.is_ok());
    }

    #[test]
    fn try_from_shared_fails_when_cloned() {
        let shared: SharedTable<String, i32> = SharedTable::new();
        let _clone = shared.clone();
        let result = FrozenTable::try_from(shared);
        assert!(result.is_err());
    }

    #[test]
    fn try_from_shared_succeeds_after_clone_dropped() {
        let shared: SharedTable<String, i32> = SharedTable::new();
        shared.insert("a".to_string(), 1);
        let clone = shared.clone();
        drop(clone);
        let frozen = FrozenTable::try_from(shared);
        assert!(frozen.is_ok());
        assert_eq!(frozen.unwrap().get(&"a".to_string()), Some(&1));
    }

    #[test]
    fn try_from_shared_empty_table() {
        let shared: SharedTable<String, i32> = SharedTable::new();
        let frozen = FrozenTable::try_from(shared).unwrap();
        assert!(frozen.is_empty());
    }

    #[test]
    fn try_from_preserves_all_entries() {
        let shared = SharedTable::new();
        for i in 0..50 {
            shared.insert(format!("k{i}"), i);
        }
        let frozen = FrozenTable::try_from(shared).unwrap();
        assert_eq!(frozen.len(), 50);
        for i in 0..50 {
            assert_eq!(frozen.get(&format!("k{i}")), Some(&i));
        }
    }

    // ── LocalTable ──────────────────────────────────────────

    #[test]
    fn local_table_insert_and_get() {
        let registry = SharedTable::new();
        registry.insert("global_a".to_string(), 42);
        let mut lt = LocalTable::new(registry);
        lt.insert("local_a".to_string(), "global_a".to_string());
        assert_eq!(lt.get(&"local_a".to_string()), Some(42));
    }

    #[test]
    fn local_table_get_missing_local_returns_none() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
        assert_eq!(lt.get(&"missing".to_string()), None);
    }

    #[test]
    fn local_table_get_missing_global_returns_none() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let mut lt = LocalTable::new(registry);
        lt.insert("local".to_string(), "not_in_registry".to_string());
        assert_eq!(lt.get(&"local".to_string()), None);
    }

    #[test]
    fn local_table_multiple_locals_same_global() {
        let registry = SharedTable::new();
        registry.insert("g".to_string(), 99);
        let mut lt = LocalTable::new(registry);
        lt.insert("a".to_string(), "g".to_string());
        lt.insert("b".to_string(), "g".to_string());
        assert_eq!(lt.get(&"a".to_string()), Some(99));
        assert_eq!(lt.get(&"b".to_string()), Some(99));
    }

    #[test]
    fn local_table_insert_overwrites() {
        let registry = SharedTable::new();
        registry.insert("g1".to_string(), 1);
        registry.insert("g2".to_string(), 2);
        let mut lt = LocalTable::new(registry);
        lt.insert("k".to_string(), "g1".to_string());
        assert_eq!(lt.get(&"k".to_string()), Some(1));
        lt.insert("k".to_string(), "g2".to_string());
        assert_eq!(lt.get(&"k".to_string()), Some(2));
    }

    #[test]
    fn local_table_registry_updated_after_insert() {
        let registry = SharedTable::new();
        let mut lt = LocalTable::new(registry.clone());
        lt.insert("k".to_string(), "g".to_string());
        assert_eq!(lt.get(&"k".to_string()), None);
        registry.insert("g".to_string(), 7);
        assert_eq!(lt.get(&"k".to_string()), Some(7));
    }

    #[test]
    fn local_table_empty() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
        assert_eq!(lt.get(&"anything".to_string()), None);
    }

    // ── FileId ──────────────────────────────────────────────

    #[test]
    fn file_id_equality() {
        let a = FileId::new(PathBuf::from("/a/b.relux"));
        let b = FileId::new(PathBuf::from("/a/b.relux"));
        assert_eq!(a, b);
    }

    #[test]
    fn file_id_inequality() {
        let a = FileId::new(PathBuf::from("/a/b.relux"));
        let b = FileId::new(PathBuf::from("/a/c.relux"));
        assert_ne!(a, b);
    }

    #[test]
    fn file_id_hash_consistency() {
        let a = FileId::new(PathBuf::from("/a/b.relux"));
        let b = FileId::new(PathBuf::from("/a/b.relux"));
        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn file_id_absolute_vs_relative() {
        let abs = FileId::new(PathBuf::from("/a/b"));
        let rel = FileId::new(PathBuf::from("a/b"));
        assert_ne!(abs, rel);
    }
}
