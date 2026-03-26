use crate::core::table::FileId;
use crate::diagnostics::{IrSpan, LoweringBail};

use super::{IrNode, IrNodeLowering, LoweringContext};

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
    type Ast = crate::Span;
    fn lower(
        ast: &crate::Span,
        file: &FileId,
        _ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(IrComment::new(IrSpan::new(file.clone(), *ast)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use std::path::PathBuf;

    #[test]
    fn ir_comment_stub() {
        let file = FileId::new(PathBuf::from("test.relux"));
        let span = IrSpan::new(file, crate::Span::new(0, 10));
        let comment = IrComment::new(span.clone());
        assert_eq!(comment.span().file(), span.file());
    }
}
