use relux_ast::AstFnDef;
use relux_ast::AstPureFnDef;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNodeLowering;
use super::LoweringContext;
use super::ident::IrIdent;
use super::stmt::IrPureStmt;
use super::stmt::IrShellStmt;

/// IrFn is an enum because builtins have no AST source.
/// IrNode is NOT implemented — Builtin has no span.
#[derive(Debug, Clone)]
pub enum IrFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrShellStmt>,
        marker_recordings: Vec<crate::marker::MarkerRecording>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}

/// IrPureFn is an enum because builtins have no AST source.
/// IrNode is NOT implemented — Builtin has no span.
#[derive(Debug, Clone)]
pub enum IrPureFn {
    UserDefined {
        name: IrIdent,
        params: Vec<IrIdent>,
        body: Vec<IrPureStmt>,
        marker_recordings: Vec<crate::marker::MarkerRecording>,
        span: IrSpan,
    },
    Builtin {
        name: String,
        arity: usize,
    },
}

impl IrNodeLowering for IrFn {
    type Ast = AstFnDef;
    fn lower(
        ast: &AstFnDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let params: Vec<IrIdent> = ast
            .params
            .iter()
            .map(|p| IrIdent::lower(&p.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        let body = ast
            .body
            .iter()
            .map(|s| IrShellStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrFn::UserDefined {
            name,
            params,
            body,
            marker_recordings: Vec::new(),
            span: IrSpan::new(file.clone(), ast.span),
        })
    }
}

impl IrNodeLowering for IrPureFn {
    type Ast = AstPureFnDef;
    fn lower(
        ast: &AstPureFnDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let params: Vec<IrIdent> = ast
            .params
            .iter()
            .map(|p| IrIdent::lower(&p.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        let body = ast
            .body
            .iter()
            .map(|s| IrPureStmt::lower(&s.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrPureFn::UserDefined {
            name,
            params,
            body,
            marker_recordings: Vec::new(),
            span: IrSpan::new(file.clone(), ast.span),
        })
    }
}
