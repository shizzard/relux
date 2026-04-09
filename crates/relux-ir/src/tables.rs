use std::collections::HashMap;
use std::hash::Hash;

use relux_ast::AstModule;
use relux_core::diagnostics::EffectId as IrEffectId;
use relux_core::diagnostics::EffectName;
use relux_core::diagnostics::FnId as IrFnId;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::ModulePath;
use relux_core::table::FileId;
use relux_core::table::SharedTable;
use relux_core::table::SourceTable;

use super::effect::IrEffect;
use super::func::IrFn;
use super::func::IrPureFn;

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
