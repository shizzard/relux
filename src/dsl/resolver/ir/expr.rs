use crate::core::table::FileId;
use crate::diagnostics::FnId;
use crate::diagnostics::InvalidReport;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::diagnostics::ModulePath;
use crate::dsl::parser::ast::AstCallExpr;
use crate::dsl::parser::ast::AstExpr;
use crate::dsl::parser::ast::AstStringPart;

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
                // Check for CaptureRef in interpolation parts
                for part in &interp.parts {
                    if let AstStringPart::CaptureRef { span: cap_span, .. } = part {
                        return Err(LoweringBail::invalid(InvalidReport::purity_violation(
                            IrSpan::new(file.clone(), *cap_span),
                        )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use crate::diagnostics::ModulePath;
    use std::path::PathBuf;

    fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(0, 10))
    }

    fn test_span2() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(20, 30))
    }

    #[test]
    fn ir_expr_string_variant() {
        let s = test_span();
        let e = IrExpr::String {
            value: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(e, IrExpr::String { .. }));
    }

    #[test]
    fn ir_expr_var_variant() {
        let e = IrExpr::Var {
            name: "x".into(),
            span: test_span(),
        };
        assert!(matches!(e, IrExpr::Var { .. }));
        let _ = e.span();
    }

    #[test]
    fn ir_expr_call_variant() {
        let s = test_span();
        let name = IrIdent::new("foo", s.clone());
        let resolved = FnId {
            module: ModulePath("test".into()),
            name: "foo".into(),
            arity: 0,
        };
        let call = IrCallExpr::new(name, resolved, vec![], s.clone());
        let e = IrExpr::Call { call, span: s };
        assert!(matches!(e, IrExpr::Call { .. }));
    }

    #[test]
    fn ir_expr_capture_ref_variant() {
        let e = IrExpr::CaptureRef {
            index: 1,
            span: test_span(),
        };
        assert!(matches!(e, IrExpr::CaptureRef { index: 1, .. }));
    }

    #[test]
    fn ir_expr_capture_ref_index_zero() {
        let e = IrExpr::CaptureRef {
            index: 0,
            span: test_span(),
        };
        assert!(matches!(e, IrExpr::CaptureRef { index: 0, .. }));
    }

    #[test]
    fn ir_pure_expr_all_three_variants() {
        let s = test_span();
        let _ = IrPureExpr::String {
            value: IrInterpolation::new(vec![], s.clone()),
            span: s.clone(),
        };
        let _ = IrPureExpr::Var {
            name: "x".into(),
            span: s.clone(),
        };
        let name = IrIdent::new("f", s.clone());
        let resolved = FnId {
            module: ModulePath("test".into()),
            name: "f".into(),
            arity: 0,
        };
        let call = IrPureCallExpr::new(name, resolved, vec![], s.clone());
        let _ = IrPureExpr::Call { call, span: s };
    }

    #[test]
    fn ir_node_enum_span_each_variant() {
        let s = test_span();
        let expr_str = IrExpr::String {
            value: IrInterpolation::new(vec![], s.clone()),
            span: s.clone(),
        };
        let expr_var = IrExpr::Var {
            name: "x".into(),
            span: s.clone(),
        };
        let _ = expr_str.span();
        let _ = expr_var.span();
    }

    #[test]
    fn ir_node_enum_span_different_values() {
        let s1 = test_span();
        let s2 = test_span2();
        let e1 = IrExpr::Var {
            name: "a".into(),
            span: s1,
        };
        let e2 = IrExpr::Var {
            name: "b".into(),
            span: s2,
        };
        assert_eq!(e1.span().span(), &crate::Span::new(0, 10));
        assert_eq!(e2.span().span(), &crate::Span::new(20, 30));
    }

    // ─── Expression lowering ──────────────────────────────────

    use crate::Span;
    use crate::diagnostics::FnId;
    use crate::diagnostics::LoweringBail;
    use crate::dsl::parser::ast::*;
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_expr_string() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let expr = extract_let_expr("fn x() {\n  let v = \"hello\"\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), IrExpr::String { .. }));
    }

    #[test]
    fn lower_expr_var() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let expr = extract_let_expr("fn x() {\n  let v = name\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), IrExpr::Var { .. }));
    }

    #[test]
    fn lower_expr_call_resolved() {
        let mut ctx = ctx_with_source(
            r#"fn foo(x) {
  > ${x}
}
fn bar() {
  let v = foo("a")
}
"#,
        );
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let expr = extract_let_expr("fn t() {\n  let v = foo(\"a\")\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), IrExpr::Call { .. }));
        let fn_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "foo".into(),
            arity: 1,
        };
        assert!(ctx.functions().get(&fn_id).is_some());
    }

    #[test]
    fn lower_expr_call_bif() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let expr = extract_let_expr("fn t() {\n  let v = trim(\"hello\")\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        if let IrExpr::Call { call, .. } = result.unwrap() {
            assert_eq!(call.name().name(), "trim");
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn lower_expr_call_multi_arg() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let expr = extract_let_expr("fn t() {\n  let v = replace(\"s\", \"a\", \"b\")\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        if let IrExpr::Call { call, .. } = result.unwrap() {
            assert_eq!(call.args().len(), 3);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn lower_expr_call_nested() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let expr = extract_let_expr("fn t() {\n  let v = trim(upper(\"x\"))\n}\n");
        let result = IrExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        if let IrExpr::Call { call, .. } = result.unwrap() {
            assert_eq!(call.name().name(), "trim");
            assert!(matches!(&call.args()[0], IrExpr::Call { .. }));
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn lower_expr_capture_ref() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::CaptureRef {
            index: 1,
            span: Span::new(0, 2),
        };
        let result = IrExpr::lower(&ast, &file, &mut ctx).unwrap();
        assert!(matches!(result, IrExpr::CaptureRef { index: 1, .. }));
    }

    #[test]
    fn lower_expr_capture_ref_multi_digit() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::CaptureRef {
            index: 12,
            span: Span::new(0, 3),
        };
        let result = IrExpr::lower(&ast, &file, &mut ctx).unwrap();
        assert!(matches!(result, IrExpr::CaptureRef { index: 12, .. }));
    }

    #[test]
    fn lower_pure_expr_string() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::String {
            interp: AstInterpolation {
                parts: vec![AstStringPart::Literal {
                    value: "hi".into(),
                    span: Span::new(1, 3),
                }],
                span: Span::new(0, 4),
            },
            span: Span::new(0, 4),
        };
        let result = IrPureExpr::lower(&ast, &file, &mut ctx).unwrap();
        assert!(matches!(result, IrPureExpr::String { .. }));
    }

    #[test]
    fn lower_pure_expr_var() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::Var {
            name: "x".into(),
            span: Span::new(0, 1),
        };
        let result = IrPureExpr::lower(&ast, &file, &mut ctx).unwrap();
        assert!(matches!(result, IrPureExpr::Var { .. }));
    }

    #[test]
    fn lower_pure_expr_call() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let expr = extract_let_expr("fn t() {\n  let v = trim(\"x\")\n}\n");
        let result = IrPureExpr::lower(&expr, &file, &mut ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), IrPureExpr::Call { .. }));
    }

    #[test]
    fn lower_pure_expr_rejects_capture_ref() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::CaptureRef {
            index: 1,
            span: Span::new(0, 2),
        };
        let result = IrPureExpr::lower(&ast, &file, &mut ctx);
        assert!(result.is_err());
        if let Err(LoweringBail::Invalid(_)) = result {
            // OK
        } else {
            panic!("expected PurityViolation, got {:?}", result);
        }
    }

    #[test]
    fn lower_pure_expr_string_rejects_capture_ref_in_parts() {
        let mut ctx = ctx_with_source("fn dummy() {}");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");

        let ast = AstExpr::String {
            interp: AstInterpolation {
                parts: vec![AstStringPart::CaptureRef {
                    index: 1,
                    span: Span::new(1, 5),
                }],
                span: Span::new(0, 6),
            },
            span: Span::new(0, 6),
        };
        let result = IrPureExpr::lower(&ast, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }
}
