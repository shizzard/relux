use std::time::Duration;

use crate::diagnostics::{IrSpan, LoweringBail};
use crate::dsl::parser::ast::AstTimeout;
use crate::table::FileId;

use super::{IrNode, IrNodeLowering, LoweringContext};

#[derive(Debug, Clone)]
pub enum IrTimeoutKind {
    Tolerance { span: IrSpan },
    Assertion { span: IrSpan },
}

impl_ir_node_enum!(IrTimeoutKind {
    Tolerance,
    Assertion
});

#[derive(Debug, Clone)]
pub struct IrTimeout {
    kind: IrTimeoutKind,
    duration: Duration,
    span: IrSpan,
}

impl IrTimeout {
    pub fn new(kind: IrTimeoutKind, duration: Duration, span: IrSpan) -> Self {
        Self {
            kind,
            duration,
            span,
        }
    }

    pub fn kind(&self) -> &IrTimeoutKind {
        &self.kind
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }
}

impl_ir_node_struct!(IrTimeout);

impl IrNodeLowering for IrTimeout {
    type Ast = AstTimeout;
    fn lower(
        ast: &AstTimeout,
        file: &FileId,
        _ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(match ast {
            AstTimeout::Tolerance { duration, span } => IrTimeout::new(
                IrTimeoutKind::Tolerance {
                    span: IrSpan::new(file.clone(), *span),
                },
                *duration,
                IrSpan::new(file.clone(), *span),
            ),
            AstTimeout::Assertion { duration, span } => IrTimeout::new(
                IrTimeoutKind::Assertion {
                    span: IrSpan::new(file.clone(), *span),
                },
                *duration,
                IrSpan::new(file.clone(), *span),
            ),
        })
    }
}
