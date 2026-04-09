use relux_ast::AstIdent;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;

#[derive(Debug, Clone)]
pub struct IrIdent {
    name: String,
    span: IrSpan,
}

impl IrIdent {
    pub fn new(name: impl Into<String>, span: IrSpan) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl_ir_node_struct!(IrIdent);

impl IrNodeLowering for IrIdent {
    type Ast = AstIdent;
    fn lower(
        ast: &AstIdent,
        file: &FileId,
        _ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(IrIdent::new(&ast.name, IrSpan::new(file.clone(), ast.span)))
    }
}
