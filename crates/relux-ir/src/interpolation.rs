use relux_ast::AstInterpolation;
use relux_ast::AstStringPart;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;

#[derive(Debug, Clone)]
pub struct IrInterpolation {
    parts: Vec<IrStringPart>,
    span: IrSpan,
}

impl IrInterpolation {
    pub fn new(parts: Vec<IrStringPart>, span: IrSpan) -> Self {
        Self { parts, span }
    }

    pub fn parts(&self) -> &[IrStringPart] {
        &self.parts
    }
}

impl_ir_node_struct!(IrInterpolation);

#[derive(Debug, Clone)]
pub enum IrStringPart {
    Literal { value: String, span: IrSpan },
    Var { name: String, span: IrSpan },
    CaptureRef { index: usize, span: IrSpan },
    EscapedDollar { span: IrSpan },
}

impl_ir_node_enum!(IrStringPart {
    Literal,
    Var,
    CaptureRef,
    EscapedDollar
});

impl IrNodeLowering for IrStringPart {
    type Ast = AstStringPart;
    fn lower(
        ast: &AstStringPart,
        file: &FileId,
        _ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(match ast {
            AstStringPart::Literal { value, span } => IrStringPart::Literal {
                value: value.clone(),
                span: IrSpan::new(file.clone(), *span),
            },
            AstStringPart::VarRef { name, span } => IrStringPart::Var {
                name: name.clone(),
                span: IrSpan::new(file.clone(), *span),
            },
            AstStringPart::CaptureRef { index, span } => IrStringPart::CaptureRef {
                index: *index,
                span: IrSpan::new(file.clone(), *span),
            },
            AstStringPart::EscapedDollar { span } => IrStringPart::EscapedDollar {
                span: IrSpan::new(file.clone(), *span),
            },
        })
    }
}

impl IrNodeLowering for IrInterpolation {
    type Ast = AstInterpolation;
    fn lower(
        ast: &AstInterpolation,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let parts = ast
            .parts
            .iter()
            .map(|p| IrStringPart::lower(p, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(IrInterpolation::new(
            parts,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}
