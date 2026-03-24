use crate::diagnostics::{CycleReport, InvalidReport, LoweringBail};
use crate::table::FileId;

pub use crate::dsl::resolver::lower::LoweringContext;

/// Trait for lowering AST nodes into IR nodes with optional caching
/// and cycle detection. Default implementations provide no-op behavior
/// for non-cacheable types — only `lower` must be implemented.
// TODO: box LoweringBail to reduce Result size
#[allow(clippy::result_large_err)]
pub trait IrNodeLowering: Sized + Clone {
    type Ast;

    /// Return `None` for non-cacheable types (default).
    /// Return `Some(Some(result))` if already resolved.
    /// Return `Some(None)` if cacheable but not yet visited.
    fn cached(
        _ast: &Self::Ast,
        _ctx: &LoweringContext,
    ) -> Option<Option<Result<Self, LoweringBail>>> {
        None
    }

    fn cache(_ast: &Self::Ast, _result: Result<Self, LoweringBail>, _ctx: &mut LoweringContext) {}

    fn check_cycle(_ast: &Self::Ast, _ctx: &LoweringContext) -> Option<CycleReport> {
        None
    }

    fn push_in_progress(_ast: &Self::Ast, _ctx: &mut LoweringContext) {}

    fn pop_in_progress(_ctx: &mut LoweringContext) {}

    /// AST → IR lowering for a single node.
    fn lower(
        ast: &Self::Ast,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail>;

    /// Orchestrates caching, cycle detection, and lowering.
    fn from_ast(
        ast: &Self::Ast,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        match Self::cached(ast, ctx) {
            None => Self::lower(ast, file, ctx),
            Some(Some(result)) => result,
            Some(None) => {
                if let Some(cycle) = Self::check_cycle(ast, ctx) {
                    let bail = LoweringBail::Invalid(InvalidReport::Cycle(cycle));
                    Self::cache(ast, Err(bail.clone()), ctx);
                    return Err(bail);
                }
                Self::push_in_progress(ast, ctx);
                let result = Self::lower(ast, file, ctx);
                Self::pop_in_progress(ctx);
                Self::cache(ast, result.clone(), ctx);
                result
            }
        }
    }
}
