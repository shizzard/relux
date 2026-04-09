use relux_ast::AstTestDef;
use relux_ast::AstTestItem;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::block::IrCleanupBlock;
use super::block::IrShellBlock;
use super::comment::IrComment;
use super::effect::IrEffectStart;
use super::stmt::IrPureLetStmt;

// ─── IrTestItem ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrTestItem {
    Comment { comment: IrComment, span: IrSpan },
    DocString { text: String, span: IrSpan },
    Start { start: IrEffectStart, span: IrSpan },
    Let { stmt: IrPureLetStmt, span: IrSpan },
    Shell { block: IrShellBlock, span: IrSpan },
    Cleanup { block: IrCleanupBlock, span: IrSpan },
}

impl_ir_node_enum!(IrTestItem {
    Comment,
    DocString,
    Start,
    Let,
    Shell,
    Cleanup
});

// ─── IrTest ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrTest {
    name: String,
    starts: Vec<IrEffectStart>,
    body: Vec<IrTestItem>,
    span: IrSpan,
}

impl IrTest {
    pub fn new(
        name: impl Into<String>,
        starts: Vec<IrEffectStart>,
        body: Vec<IrTestItem>,
        span: IrSpan,
    ) -> Self {
        Self {
            name: name.into(),
            starts,
            body,
            span,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn starts(&self) -> &[IrEffectStart] {
        &self.starts
    }

    pub fn body(&self) -> &[IrTestItem] {
        &self.body
    }
}

impl_ir_node_struct!(IrTest);

// ═══════════════════════════════════════════════════════════════
// IrNodeLowering implementations
// ═══════════════════════════════════════════════════════════════

impl IrNodeLowering for IrTestItem {
    type Ast = AstTestItem;
    fn lower(
        ast: &AstTestItem,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let s = |span: &relux_core::Span| IrSpan::new(file.clone(), *span);
        match ast {
            AstTestItem::Comment { span, .. } => {
                let comment = IrComment::lower(span, file, ctx)?;
                Ok(IrTestItem::Comment {
                    comment,
                    span: s(span),
                })
            }
            AstTestItem::DocString { text, span } => Ok(IrTestItem::DocString {
                text: text.clone(),
                span: s(span),
            }),
            AstTestItem::Start { decl, span } => {
                let start = IrEffectStart::lower(decl, file, ctx)?;
                Ok(IrTestItem::Start {
                    start,
                    span: s(span),
                })
            }
            AstTestItem::Let { stmt, span } => {
                let ir = IrPureLetStmt::lower(stmt, file, ctx)?;
                Ok(IrTestItem::Let {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstTestItem::Shell { block, span } => {
                let ir = IrShellBlock::lower(block, file, ctx)?;
                Ok(IrTestItem::Shell {
                    block: ir,
                    span: s(span),
                })
            }
            AstTestItem::Cleanup { block, span } => {
                let ir = IrCleanupBlock::lower(block, file, ctx)?;
                Ok(IrTestItem::Cleanup {
                    block: ir,
                    span: s(span),
                })
            }
        }
    }
}

impl IrNodeLowering for IrTest {
    type Ast = AstTestDef;
    /// Lower a test body. Assumes scope is already pushed on ctx.
    fn lower(
        ast: &AstTestDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let mut starts = Vec::new();
        let mut body_items = Vec::new();

        for spanned_item in &ast.body {
            let ir_item = IrTestItem::lower(&spanned_item.node, file, ctx)?;
            // Track let-bound names in the shallow env for expect checking
            if let IrTestItem::Let { ref stmt, .. } = ir_item
                && let Some(env) = ctx.shallow_env()
            {
                let updated =
                    std::sync::Arc::new(crate::shallow_env::ShallowLayeredEnv::with_name(
                        env,
                        stmt.name().name().to_string(),
                    ));
                ctx.set_shallow_env(updated);
            }
            if let IrTestItem::Start { ref start, .. } = ir_item {
                starts.push(start.clone());
            }
            body_items.push(ir_item);
        }

        let has_nonempty_shell = body_items.iter().any(
            |item| matches!(item, IrTestItem::Shell { block, .. } if !block.body().is_empty()),
        );
        if !has_nonempty_shell {
            return Err(LoweringBail::invalid(InvalidReport::empty_test_body(
                ast.name.node.clone(),
                IrSpan::new(file.clone(), ast.span),
            )));
        }

        Ok(IrTest::new(
            &ast.name.node,
            starts,
            body_items,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}
