use relux_ast::AstCallExpr;
use relux_ast::AstExpr;
use relux_ast::AstStringPart;
use relux_core::diagnostics::FnId;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::ModulePath;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::ident::IrIdent;
use super::interpolation::IrInterpolation;
use super::tables::LocalFnKey;

// ─── IrExpr ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrExpr {
    String {
        value: IrInterpolation,
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
    Call {
        call: IrCallExpr,
        span: IrSpan,
    },
    CaptureRef {
        index: usize,
        span: IrSpan,
    },
}

impl_ir_node_enum!(IrExpr {
    String,
    Var,
    QualifiedVar,
    Call,
    CaptureRef
});

// ─── IrPureExpr ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrPureExpr {
    String {
        value: IrInterpolation,
        span: IrSpan,
    },
    Var {
        name: String,
        span: IrSpan,
    },
    Call {
        call: IrPureCallExpr,
        span: IrSpan,
    },
}

impl_ir_node_enum!(IrPureExpr { String, Var, Call });

// ─── IrCallExpr ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrCallExpr {
    name: IrIdent,
    resolved: FnId,
    args: Vec<IrExpr>,
    span: IrSpan,
}

impl IrCallExpr {
    pub fn new(name: IrIdent, resolved: FnId, args: Vec<IrExpr>, span: IrSpan) -> Self {
        Self {
            name,
            resolved,
            args,
            span,
        }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn resolved(&self) -> &FnId {
        &self.resolved
    }

    pub fn args(&self) -> &[IrExpr] {
        &self.args
    }
}

impl_ir_node_struct!(IrCallExpr);

// ─── IrPureCallExpr ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrPureCallExpr {
    name: IrIdent,
    resolved: FnId,
    args: Vec<IrPureExpr>,
    span: IrSpan,
}

impl IrPureCallExpr {
    pub fn new(name: IrIdent, resolved: FnId, args: Vec<IrPureExpr>, span: IrSpan) -> Self {
        Self {
            name,
            resolved,
            args,
            span,
        }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn resolved(&self) -> &FnId {
        &self.resolved
    }

    pub fn args(&self) -> &[IrPureExpr] {
        &self.args
    }
}

impl_ir_node_struct!(IrPureCallExpr);

// ─── IrNodeLowering: IrExpr ─────────────────────────────────

impl IrNodeLowering for IrExpr {
    type Ast = AstExpr;
    fn lower(
        ast: &AstExpr,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        match ast {
            AstExpr::String { interp, span } => {
                let ir_interp = IrInterpolation::lower(interp, file, ctx)?;
                Ok(IrExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file.clone(), *span),
                })
            }
            AstExpr::Var { name, span } => Ok(IrExpr::Var {
                name: name.clone(),
                span: IrSpan::new(file.clone(), *span),
            }),
            AstExpr::QualifiedVar {
                qualifier,
                name,
                span,
            } => Ok(IrExpr::QualifiedVar {
                qualifier: qualifier.clone(),
                name: name.clone(),
                span: IrSpan::new(file.clone(), *span),
            }),
            AstExpr::CaptureRef { index, span } => Ok(IrExpr::CaptureRef {
                index: *index,
                span: IrSpan::new(file.clone(), *span),
            }),
            AstExpr::Call { call, span } => {
                let ir_call = IrCallExpr::lower(call, file, ctx)?;
                Ok(IrExpr::Call {
                    call: ir_call,
                    span: IrSpan::new(file.clone(), *span),
                })
            }
        }
    }
}

// ─── IrNodeLowering: IrPureExpr ─────────────────────────────

impl IrNodeLowering for IrPureExpr {
    type Ast = AstExpr;
    fn lower(
        ast: &AstExpr,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        match ast {
            AstExpr::String { interp, span } => {
                // Check for impure constructs in interpolation parts
                for part in &interp.parts {
                    match part {
                        AstStringPart::CaptureRef { span: s, .. }
                        | AstStringPart::QualifiedVarRef { span: s, .. } => {
                            return Err(LoweringBail::invalid(InvalidReport::purity_violation(
                                IrSpan::new(file.clone(), *s),
                            )));
                        }
                        _ => {}
                    }
                }
                let ir_interp = IrInterpolation::lower(interp, file, ctx)?;
                Ok(IrPureExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file.clone(), *span),
                })
            }
            AstExpr::Var { name, span } => Ok(IrPureExpr::Var {
                name: name.clone(),
                span: IrSpan::new(file.clone(), *span),
            }),
            AstExpr::QualifiedVar { span, .. } => Err(LoweringBail::invalid(
                InvalidReport::purity_violation(IrSpan::new(file.clone(), *span)),
            )),
            AstExpr::CaptureRef { span, .. } => Err(LoweringBail::invalid(
                InvalidReport::purity_violation(IrSpan::new(file.clone(), *span)),
            )),
            AstExpr::Call { call, span } => {
                let ir_call = IrPureCallExpr::lower(call, file, ctx)?;
                Ok(IrPureExpr::Call {
                    call: ir_call,
                    span: IrSpan::new(file.clone(), *span),
                })
            }
        }
    }
}

// ─── IrNodeLowering: IrCallExpr ─────────────────────────────

impl IrNodeLowering for IrCallExpr {
    type Ast = AstCallExpr;
    fn lower(
        ast: &AstCallExpr,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = &ast.name.node.name;
        let arity = ast.args.len();
        let local_key = LocalFnKey::new(name, arity);

        // Look up in current scope's fn table, then fall back to BIF
        let global_key = {
            let scope = ctx.current_scope();
            scope.tables.fns.get_global_key(&local_key).cloned()
        }
        .or_else(|| {
            let bif_id = FnId {
                module: ModulePath("@builtin".into()),
                name: name.clone(),
                arity,
            };
            if ctx.functions().contains(&bif_id) {
                Some(bif_id)
            } else {
                None
            }
        });

        let global_key = global_key.ok_or_else(|| {
            LoweringBail::invalid(InvalidReport::undefined_function_call(
                name.clone(),
                arity,
                IrSpan::new(file.clone(), ast.name.node.span),
            ))
        })?;

        // Resolve the callee (ensures it's lowered and cached).
        // Check if this is a pure fn first (pure fns are also in fn_table).
        let is_pure = {
            let scope = ctx.current_scope();
            scope.tables.pure_fns.get_global_key(&local_key).is_some()
        } || ctx.pure_functions().contains(&global_key);

        if is_pure {
            ctx.resolve_pure_fn(&global_key)?;
        } else {
            ctx.resolve_fn(&global_key)?;
        }

        // Lower args
        let ir_args = ast
            .args
            .iter()
            .map(|arg| IrExpr::lower(&arg.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrCallExpr::new(
            IrIdent::lower(&ast.name.node, file, ctx)?,
            global_key,
            ir_args,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

// ─── IrNodeLowering: IrPureCallExpr ─────────────────────────

impl IrNodeLowering for IrPureCallExpr {
    type Ast = AstCallExpr;
    fn lower(
        ast: &AstCallExpr,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = &ast.name.node.name;
        let arity = ast.args.len();
        let local_key = LocalFnKey::new(name, arity);

        // Look up in pure fn table, then fall back to pure BIF
        let global_key = {
            let scope = ctx.current_scope();
            scope.tables.pure_fns.get_global_key(&local_key).cloned()
        }
        .or_else(|| {
            let bif_id = FnId {
                module: ModulePath("@builtin".into()),
                name: name.clone(),
                arity,
            };
            if ctx.pure_functions().contains(&bif_id) {
                Some(bif_id)
            } else {
                None
            }
        });

        let global_key = match global_key {
            Some(key) => key,
            None => {
                // Check if it's impure (local table or impure BIF) → PurityViolation
                let in_impure = {
                    let scope = ctx.current_scope();
                    scope.tables.fns.get_global_key(&local_key).is_some()
                } || {
                    let bif_id = FnId {
                        module: ModulePath("@builtin".into()),
                        name: name.clone(),
                        arity,
                    };
                    ctx.functions().contains(&bif_id)
                };
                if in_impure {
                    return Err(LoweringBail::invalid(InvalidReport::purity_violation(
                        IrSpan::new(file.clone(), ast.name.node.span),
                    )));
                } else {
                    return Err(LoweringBail::invalid(
                        InvalidReport::undefined_function_call(
                            name.clone(),
                            arity,
                            IrSpan::new(file.clone(), ast.name.node.span),
                        ),
                    ));
                }
            }
        };

        // Resolve
        ctx.resolve_pure_fn(&global_key)?;

        // Lower args as pure
        let ir_args = ast
            .args
            .iter()
            .map(|arg| IrPureExpr::lower(&arg.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IrPureCallExpr::new(
            IrIdent::lower(&ast.name.node, file, ctx)?,
            global_key,
            ir_args,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

// ─── Tests ───────────────────────────────────────────────────
