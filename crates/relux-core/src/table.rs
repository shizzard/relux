use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;

use elsa::sync::FrozenMap;

// ─── SharedTable ────────────────────────────────────────────

/// Mutable shared table — populated incrementally, potentially from multiple threads.
/// Write-once semantics: first insert wins, subsequent inserts for the same key are ignored.
pub struct SharedTable<K, V> {
    map: Arc<FrozenMap<K, Box<V>>>,
}

impl<K, V> SharedTable<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            map: Arc::new(FrozenMap::new()),
        }
    }

    pub fn insert(&self, key: K, value: V) {
        self.map.insert(key, Box::new(value));
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.len() == 0
    }

    pub fn as_vec(&self) -> Vec<(K, &V)>
    where
        K: Clone,
    {
        self.map
            .keys_cloned()
            .into_iter()
            .map(|k| {
                let v = self.map.get(&k).unwrap();
                (k, v)
            })
            .collect()
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
    line_offsets: Vec<usize>,
}

impl SourceFile {
    pub fn new(path: PathBuf, source: String) -> Self {
        let line_offsets = build_line_offsets(&source);
        Self {
            path,
            source,
            line_offsets,
        }
    }

    /// Returns the 1-based line number for a byte offset.
    pub fn line_at(&self, byte_offset: usize) -> usize {
        self.line_offsets.partition_point(|&off| off <= byte_offset)
    }
}

fn build_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

// ─── SourceTable ────────────────────────────────────────────

pub type SourceTable = SharedTable<FileId, SourceFile>;

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
        assert_eq!(t.get(&"a"), Some(&1));
    }

    #[test]
    fn shared_table_get_missing_returns_none() {
        let t: SharedTable<&str, i32> = SharedTable::new();
        assert_eq!(t.get(&"x"), None);
    }

    #[test]
    fn shared_table_first_insert_wins() {
        let t = SharedTable::new();
        t.insert("a", 1);
        t.insert("a", 2);
        // FrozenMap: first insert wins
        assert_eq!(t.get(&"a"), Some(&1));
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
        assert_eq!(t2.get(&"a"), Some(&1));
    }

    #[test]
    fn shared_table_original_sees_clone_inserts() {
        let t = SharedTable::new();
        let t2 = t.clone();
        t2.insert("b", 42);
        assert_eq!(t.get(&"b"), Some(&42));
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
            assert_eq!(t.get(&i), Some(&(i * 10)));
        }
    }

    #[test]
    fn shared_table_get_returns_ref() {
        let t = SharedTable::new();
        t.insert("a", vec![1, 2, 3]);
        let v = t.get(&"a").unwrap();
        assert_eq!(v, &vec![1, 2, 3]);
        // References are stable — get again returns same value
        let v2 = t.get(&"a").unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn shared_table_len() {
        let t = SharedTable::new();
        t.insert("a", 1);
        t.insert("b", 2);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn shared_table_len_empty() {
        let t: SharedTable<&str, i32> = SharedTable::new();
        assert_eq!(t.len(), 0);
        assert!(t.is_empty());
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

    // ── SourceFile ─────────────────────────────────────────

    #[test]
    fn line_at_single_line() {
        let sf = SourceFile::new(PathBuf::from("t.relux"), "hello".into());
        assert_eq!(sf.line_at(0), 1);
        assert_eq!(sf.line_at(4), 1);
    }

    #[test]
    fn line_at_multiple_lines() {
        // "ab\ncd\nef"
        //  0123 456 78
        let sf = SourceFile::new(PathBuf::from("t.relux"), "ab\ncd\nef".into());
        assert_eq!(sf.line_at(0), 1); // 'a'
        assert_eq!(sf.line_at(2), 1); // '\n'
        assert_eq!(sf.line_at(3), 2); // 'c'
        assert_eq!(sf.line_at(6), 3); // 'e'
    }

    #[test]
    fn line_at_empty_source() {
        let sf = SourceFile::new(PathBuf::from("t.relux"), String::new());
        assert_eq!(sf.line_at(0), 1);
    }
}
