use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;

/// Stub IR node for comments. No fields beyond span — comments are
/// passed through to the runtime, which can decide what (if anything)
/// to do with them.
#[derive(Debug, Clone)]
pub struct IrComment {
    span: IrSpan,
}

impl IrComment {
    pub fn new(span: IrSpan) -> Self {
        Self { span }
    }
}

impl_ir_node_struct!(IrComment);

impl IrNodeLowering for IrComment {
    type Ast = relux_core::Span;
    fn lower(
        ast: &relux_core::Span,
        file: &FileId,
        _ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(IrComment::new(IrSpan::new(file.clone(), *ast)))
    }
}
