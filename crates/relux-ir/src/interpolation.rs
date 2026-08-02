use relux_ast::AstInterpolation;
use relux_ast::AstStringPart;
use relux_core::diagnostics::InvalidReport;
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

    /// Lower an interpolation used in a pure context - a pure-match pattern or
    /// a pure string expression. Rejects `${Alias.field}` (`QualifiedVarRef`)
    /// parts, which read effect-scoped state and are never pure. This is a
    /// compile-time purity check: without it a qualified var would lower clean
    /// and then resolve silently at eval time - `render_interpolation` looks it
    /// up by flat `qualifier.name` key and defaults to `""` - masking the
    /// purity violation instead of surfacing it as a lowering error.
    pub fn lower_pure(
        ast: &AstInterpolation,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        for part in &ast.parts {
            if let AstStringPart::QualifiedVarRef { span, .. } = part {
                return Err(LoweringBail::invalid(InvalidReport::purity_violation(
                    IrSpan::new(file.clone(), *span),
                )));
            }
        }
        Self::lower(ast, file, ctx)
    }
}

impl_ir_node_struct!(IrInterpolation);

#[derive(Debug, Clone)]
pub enum IrStringPart {
    Literal {
        value: String,
        span: IrSpan,
    },
    Var {
        name: String,
        span: IrSpan,
    },
    QualifiedVar {
        qualifier: String,
        name: String,
        span: IrSpan,
    },
    CaptureRef {
        index: usize,
        span: IrSpan,
    },
    EscapedDollar {
        span: IrSpan,
    },
}

impl_ir_node_enum!(IrStringPart {
    Literal,
    Var,
    QualifiedVar,
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
            AstStringPart::QualifiedVarRef {
                qualifier,
                name,
                span,
            } => IrStringPart::QualifiedVar {
                qualifier: qualifier.clone(),
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
