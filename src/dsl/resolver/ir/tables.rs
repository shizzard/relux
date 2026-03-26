use std::collections::HashMap;
use std::hash::Hash;

use crate::core::table::{FileId, SharedTable, SourceFile};
use crate::diagnostics::{
    EffectId as IrEffectId, EffectName, FnId as IrFnId, IrSpan, LoweringBail, ModulePath,
};
use crate::dsl::parser::ast::AstModule;

use super::effect::IrEffect;
use super::func::{IrFn, IrPureFn};

// ─── Local Keys ─────────────────────────────────────────────

/// Local function key — used by both fn and pure fn local tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalFnKey {
    pub name: String,
    pub arity: usize,
}

impl LocalFnKey {
    pub fn new(name: impl Into<String>, arity: usize) -> Self {
        Self {
            name: name.into(),
            arity,
        }
    }
}

/// Local effect key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalEffectKey {
    pub name: EffectName,
}

impl LocalEffectKey {
    pub fn new(name: EffectName) -> Self {
        Self { name }
    }
}

pub type AstTable = SharedTable<ModulePath, (FileId, AstModule)>;
pub type SourceTable = SharedTable<FileId, SourceFile>;

pub type FnTable = SharedTable<IrFnId, Result<IrFn, LoweringBail>>;
pub type PureFnTable = SharedTable<IrFnId, Result<IrPureFn, LoweringBail>>;
pub type EffectTable = SharedTable<IrEffectId, Result<IrEffect, LoweringBail>>;

// ─── Tables ─────────────────────────────────────────────────

/// Shared (global) resolution tables — sources, functions, pure functions, effects.
#[derive(Debug, Clone)]
pub struct Tables {
    pub sources: SourceTable,
    pub fns: FnTable,
    pub pure_fns: PureFnTable,
    pub effects: EffectTable,
}

impl Tables {
    pub fn new() -> Self {
        Self {
            sources: SharedTable::new(),
            fns: SharedTable::new(),
            pure_fns: SharedTable::new(),
            effects: SharedTable::new(),
        }
    }
}

impl Default for Tables {
    fn default() -> Self {
        Self::new()
    }
}

// ─── LocalTable ─────────────────────────────────────────────

/// Local name resolution table — maps local keys to global keys with
/// origin spans, backed by a SharedTable for registry lookups.
pub struct LocalTable<K, GK, V> {
    locals: HashMap<K, (GK, IrSpan)>,
    registry: SharedTable<GK, V>,
}

impl<K, GK, V> LocalTable<K, GK, V>
where
    K: Eq + Hash,
    GK: Eq + Hash + Clone,
{
    pub fn new(registry: SharedTable<GK, V>) -> Self {
        Self {
            locals: HashMap::new(),
            registry,
        }
    }

    pub fn insert(&mut self, local_key: K, global_key: GK, span: IrSpan) {
        self.locals.insert(local_key, (global_key, span));
    }

    pub fn get(&self, local_key: &K) -> Option<&V> {
        let (global_key, _) = self.locals.get(local_key)?;
        self.registry.get(global_key)
    }

    /// Check if a local key has been mapped (regardless of whether
    /// the global key has a value in the registry yet).
    pub fn contains_local(&self, local_key: &K) -> bool {
        self.locals.contains_key(local_key)
    }

    /// Get the global key mapped to a local key, if any.
    pub fn get_global_key(&self, local_key: &K) -> Option<&GK> {
        self.locals.get(local_key).map(|(gk, _)| gk)
    }

    /// Get the origin span for a local key, if any.
    pub fn get_span(&self, local_key: &K) -> Option<&IrSpan> {
        self.locals.get(local_key).map(|(_, span)| span)
    }
}

// ─── Local Tables ───────────────────────────────────────────

pub type LocalFnTable = LocalTable<LocalFnKey, IrFnId, Result<IrFn, LoweringBail>>;
pub type LocalPureFnTable = LocalTable<LocalFnKey, IrFnId, Result<IrPureFn, LoweringBail>>;
pub type LocalEffectTable = LocalTable<LocalEffectKey, IrEffectId, Result<IrEffect, LoweringBail>>;

/// Per-scope local resolution tables for import processing.
pub struct LocalTables {
    pub fns: LocalFnTable,
    pub pure_fns: LocalPureFnTable,
    pub effects: LocalEffectTable,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;

    fn dummy_span() -> IrSpan {
        IrSpan::synthetic()
    }

    #[test]
    fn local_table_insert_and_get() {
        let registry = SharedTable::new();
        registry.insert("global_a".to_string(), 42);
        let mut lt = LocalTable::new(registry);
        lt.insert("local_a".to_string(), "global_a".to_string(), dummy_span());
        assert_eq!(lt.get(&"local_a".to_string()), Some(&42));
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
        lt.insert(
            "local".to_string(),
            "not_in_registry".to_string(),
            dummy_span(),
        );
        assert_eq!(lt.get(&"local".to_string()), None);
    }

    #[test]
    fn local_table_multiple_locals_same_global() {
        let registry = SharedTable::new();
        registry.insert("g".to_string(), 99);
        let mut lt = LocalTable::new(registry);
        lt.insert("a".to_string(), "g".to_string(), dummy_span());
        lt.insert("b".to_string(), "g".to_string(), dummy_span());
        assert_eq!(lt.get(&"a".to_string()), Some(&99));
        assert_eq!(lt.get(&"b".to_string()), Some(&99));
    }

    #[test]
    fn local_table_insert_overwrites() {
        let registry = SharedTable::new();
        registry.insert("g1".to_string(), 1);
        registry.insert("g2".to_string(), 2);
        let mut lt = LocalTable::new(registry);
        lt.insert("k".to_string(), "g1".to_string(), dummy_span());
        assert_eq!(lt.get(&"k".to_string()), Some(&1));
        lt.insert("k".to_string(), "g2".to_string(), dummy_span());
        assert_eq!(lt.get(&"k".to_string()), Some(&2));
    }

    #[test]
    fn local_table_registry_updated_after_insert() {
        let registry = SharedTable::new();
        let mut lt = LocalTable::new(registry.clone());
        lt.insert("k".to_string(), "g".to_string(), dummy_span());
        assert_eq!(lt.get(&"k".to_string()), None);
        registry.insert("g".to_string(), 7);
        assert_eq!(lt.get(&"k".to_string()), Some(&7));
    }

    #[test]
    fn local_table_empty() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
        assert_eq!(lt.get(&"anything".to_string()), None);
    }

    #[test]
    fn local_table_get_span() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let file = FileId::new(std::path::PathBuf::from("test.relux"));
        let span = IrSpan::new(file.clone(), crate::Span::new(10, 20));
        let mut lt = LocalTable::new(registry);
        lt.insert("k".to_string(), "g".to_string(), span);
        let got = lt.get_span(&"k".to_string()).unwrap();
        assert_eq!(got.file(), &file);
    }

    #[test]
    fn local_table_get_span_missing() {
        let registry: SharedTable<String, i32> = SharedTable::new();
        let lt: LocalTable<String, String, i32> = LocalTable::new(registry);
        assert!(lt.get_span(&"missing".to_string()).is_none());
    }
}
