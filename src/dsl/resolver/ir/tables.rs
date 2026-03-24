use crate::diagnostics::{EffectId as IrEffectId, FnId as IrFnId, LoweringBail, ModulePath};
use crate::dsl::parser::ast::AstModule;
use crate::table::{FileId, FrozenTable, SharedTable, SourceFile};

use super::effect::IrEffect;
use super::func::{IrFn, IrPureFn};

pub type AstTable = FrozenTable<ModulePath, (FileId, AstModule)>;
pub type SourceTable = FrozenTable<FileId, SourceFile>;
pub type FnTable = SharedTable<IrFnId, Result<IrFn, LoweringBail>>;
pub type PureFnTable = SharedTable<IrFnId, Result<IrPureFn, LoweringBail>>;
pub type EffectTable = SharedTable<IrEffectId, Result<IrEffect, LoweringBail>>;
