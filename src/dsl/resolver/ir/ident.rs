use crate::core::table::FileId;
use crate::diagnostics::{IrSpan, LoweringBail};
use crate::dsl::parser::ast::AstIdent;

use super::{IrNode, IrNodeLowering, LoweringContext};

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

    #[test]
    fn ir_ident_name_and_span() {
        let ident = IrIdent::new("my_var", test_span());
        assert_eq!(ident.name(), "my_var");
        assert_eq!(ident.span().span(), &crate::Span::new(0, 10));
    }

    #[test]
    fn ir_ident_empty_name() {
        let ident = IrIdent::new("", test_span());
        assert_eq!(ident.name(), "");
    }

    // ─── Lowering tests (moved from lower.rs) ───────────────

    use crate::Span;
    use crate::dsl::parser::ast::AstIdent;
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_ident_name_and_span() {
        let file = crate::dsl::resolver::lower::test_helpers::test_file_id();
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        let ast_ident = AstIdent::new("foo", Span::new(5, 8));
        let ir = IrIdent::lower(&ast_ident, &file, &mut ctx).unwrap();
        assert_eq!(ir.name(), "foo");
        assert_eq!(ir.span().span(), &Span::new(5, 8));
    }

    #[test]
    fn lower_ident_preserves_span_file() {
        let file = FileId::new(PathBuf::from("/other/file.relux"));
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        let ast_ident = AstIdent::new("bar", Span::new(0, 3));
        let ir = IrIdent::lower(&ast_ident, &file, &mut ctx).unwrap();
        assert_eq!(ir.span().file(), &file);
    }
}
