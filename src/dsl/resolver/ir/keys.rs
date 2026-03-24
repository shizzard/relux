use crate::diagnostics::EffectName;

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
