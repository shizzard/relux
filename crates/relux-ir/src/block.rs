use relux_ast::AstCleanupBlock;
use relux_ast::AstShellBlock;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::ident::IrIdent;
use super::stmt::IrShellStmt;

#[derive(Debug, Clone)]
pub struct IrShellBlock {
    qualifier: Option<IrIdent>,
    name: IrIdent,
    body: Vec<IrShellStmt>,
    span: IrSpan,
}

impl IrShellBlock {
    pub fn new(
        qualifier: Option<IrIdent>,
        name: IrIdent,
        body: Vec<IrShellStmt>,
        span: IrSpan,
    ) -> Self {
        Self {
            qualifier,
            name,
            body,
            span,
        }
    }

    pub fn qualifier(&self) -> Option<&IrIdent> {
        self.qualifier.as_ref()
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn body(&self) -> &[IrShellStmt] {
        &self.body
    }
}

impl_ir_node_struct!(IrShellBlock);

#[derive(Debug, Clone)]
pub struct IrCleanupBlock {
    body: Vec<IrShellStmt>,
    span: IrSpan,
}

impl IrCleanupBlock {
    pub fn new(body: Vec<IrShellStmt>, span: IrSpan) -> Self {
        Self { body, span }
    }

    pub fn body(&self) -> &[IrShellStmt] {
        &self.body
    }
}

impl_ir_node_struct!(IrCleanupBlock);

impl IrNodeLowering for IrShellBlock {
    type Ast = AstShellBlock;
    fn lower(
        ast: &AstShellBlock,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let qualifier = ast
            .qualifier
            .as_ref()
            .map(|q| IrIdent::lower(&q.node, file, ctx))
            .transpose()?;
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let body = ast
            .stmts
            .iter()
            .map(|s| IrShellStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(IrShellBlock::new(
            qualifier,
            name,
            body,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrCleanupBlock {
    type Ast = AstCleanupBlock;
    fn lower(
        ast: &AstCleanupBlock,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let body = ast
            .stmts
            .iter()
            .map(|s| IrShellStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(IrCleanupBlock::new(
            body,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}
