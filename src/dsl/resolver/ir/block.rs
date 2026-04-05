use crate::core::table::FileId;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::dsl::parser::ast::AstCleanupBlock;
use crate::dsl::parser::ast::AstShellBlock;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use std::path::PathBuf;

    fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(0, 10))
    }

    fn test_ident(name: &str) -> IrIdent {
        IrIdent::new(name, test_span())
    }

    #[test]
    fn ir_shell_block() {
        let s = test_span();
        let block = IrShellBlock::new(
            None,
            test_ident("sh"),
            vec![IrShellStmt::BufferReset { span: s.clone() }],
            s,
        );
        assert_eq!(block.name().name(), "sh");
        assert_eq!(block.body().len(), 1);
    }

    #[test]
    fn ir_shell_block_empty_body() {
        let block = IrShellBlock::new(None, test_ident("sh"), vec![], test_span());
        assert!(block.body().is_empty());
    }

    #[test]
    fn ir_cleanup_block() {
        let s = test_span();
        let block = IrCleanupBlock::new(vec![IrShellStmt::BufferReset { span: s.clone() }], s);
        assert_eq!(block.body().len(), 1);
    }

    #[test]
    fn ir_cleanup_block_empty_body() {
        let block = IrCleanupBlock::new(vec![], test_span());
        assert!(block.body().is_empty());
    }
}
