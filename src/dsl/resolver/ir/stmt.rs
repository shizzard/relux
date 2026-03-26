use crate::core::table::FileId;
use crate::diagnostics::{InvalidReport, IrSpan, LoweringBail};
use crate::dsl::parser::ast::{AstAssignStmt, AstLetStmt, AstStmt};

use super::comment::IrComment;
use super::expr::{IrExpr, IrPureExpr};
use super::ident::IrIdent;
use super::interpolation::IrInterpolation;
use super::timeout::IrTimeout;
use super::{IrNode, IrNodeLowering, LoweringContext};

// ─── IrLetStmt ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrLetStmt {
    name: IrIdent,
    value: Option<IrExpr>,
    span: IrSpan,
}

impl IrLetStmt {
    pub fn new(name: IrIdent, value: Option<IrExpr>, span: IrSpan) -> Self {
        Self { name, value, span }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn value(&self) -> Option<&IrExpr> {
        self.value.as_ref()
    }
}

impl_ir_node_struct!(IrLetStmt);

// ─── IrAssignStmt ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrAssignStmt {
    name: IrIdent,
    value: IrExpr,
    span: IrSpan,
}

impl IrAssignStmt {
    pub fn new(name: IrIdent, value: IrExpr, span: IrSpan) -> Self {
        Self { name, value, span }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn value(&self) -> &IrExpr {
        &self.value
    }
}

impl_ir_node_struct!(IrAssignStmt);

// ─── IrShellStmt ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrShellStmt {
    Comment {
        comment: IrComment,
        span: IrSpan,
    },
    Let {
        stmt: IrLetStmt,
        span: IrSpan,
    },
    Assign {
        stmt: IrAssignStmt,
        span: IrSpan,
    },
    Expr {
        expr: IrExpr,
        span: IrSpan,
    },
    Send {
        payload: IrInterpolation,
        span: IrSpan,
    },
    SendRaw {
        payload: IrInterpolation,
        span: IrSpan,
    },
    MatchRegex {
        pattern: IrInterpolation,
        span: IrSpan,
    },
    MatchLiteral {
        pattern: IrInterpolation,
        span: IrSpan,
    },
    TimedMatchRegex {
        timeout: IrTimeout,
        pattern: IrInterpolation,
        span: IrSpan,
    },
    TimedMatchLiteral {
        timeout: IrTimeout,
        pattern: IrInterpolation,
        span: IrSpan,
    },
    Timeout {
        timeout: IrTimeout,
        span: IrSpan,
    },
    FailRegex {
        pattern: IrInterpolation,
        span: IrSpan,
    },
    FailLiteral {
        pattern: IrInterpolation,
        span: IrSpan,
    },
    ClearFailPattern {
        span: IrSpan,
    },
    BufferReset {
        span: IrSpan,
    },
}

impl_ir_node_enum!(IrShellStmt {
    Comment,
    Let,
    Assign,
    Expr,
    Send,
    SendRaw,
    MatchRegex,
    MatchLiteral,
    TimedMatchRegex,
    TimedMatchLiteral,
    Timeout,
    FailRegex,
    FailLiteral,
    ClearFailPattern,
    BufferReset
});

// ─── IrPureLetStmt ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrPureLetStmt {
    name: IrIdent,
    value: Option<IrPureExpr>,
    span: IrSpan,
}

impl IrPureLetStmt {
    pub fn new(name: IrIdent, value: Option<IrPureExpr>, span: IrSpan) -> Self {
        Self { name, value, span }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn value(&self) -> Option<&IrPureExpr> {
        self.value.as_ref()
    }
}

impl_ir_node_struct!(IrPureLetStmt);

// ─── IrPureAssignStmt ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrPureAssignStmt {
    name: IrIdent,
    value: IrPureExpr,
    span: IrSpan,
}

impl IrPureAssignStmt {
    pub fn new(name: IrIdent, value: IrPureExpr, span: IrSpan) -> Self {
        Self { name, value, span }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn value(&self) -> &IrPureExpr {
        &self.value
    }
}

impl_ir_node_struct!(IrPureAssignStmt);

// ─── IrPureStmt ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrPureStmt {
    Comment {
        comment: IrComment,
        span: IrSpan,
    },
    Let {
        stmt: IrPureLetStmt,
        span: IrSpan,
    },
    Assign {
        stmt: IrPureAssignStmt,
        span: IrSpan,
    },
    Expr {
        expr: IrPureExpr,
        span: IrSpan,
    },
}

impl_ir_node_enum!(IrPureStmt {
    Comment,
    Let,
    Assign,
    Expr
});

// ═══════════════════════════════════════════════════════════════
// IrNodeLowering implementations
// ═══════════════════════════════════════════════════════════════

impl IrNodeLowering for IrLetStmt {
    type Ast = AstLetStmt;
    fn lower(
        ast: &AstLetStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let value = ast
            .value
            .as_ref()
            .map(|v| IrExpr::lower(&v.node, file, ctx))
            .transpose()?;
        Ok(IrLetStmt::new(
            name,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrAssignStmt {
    type Ast = AstAssignStmt;
    fn lower(
        ast: &AstAssignStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let value = IrExpr::lower(&ast.value.node, file, ctx)?;
        Ok(IrAssignStmt::new(
            name,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrPureLetStmt {
    type Ast = AstLetStmt;
    fn lower(
        ast: &AstLetStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let value = ast
            .value
            .as_ref()
            .map(|v| IrPureExpr::lower(&v.node, file, ctx))
            .transpose()?;
        Ok(IrPureLetStmt::new(
            name,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrPureAssignStmt {
    type Ast = AstAssignStmt;
    fn lower(
        ast: &AstAssignStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;
        let value = IrPureExpr::lower(&ast.value.node, file, ctx)?;
        Ok(IrPureAssignStmt::new(
            name,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrShellStmt {
    type Ast = AstStmt;
    fn lower(
        ast: &AstStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let s = |span: &crate::Span| IrSpan::new(file.clone(), *span);
        match ast {
            AstStmt::Comment { span, .. } => {
                let comment = IrComment::lower(span, file, ctx)?;
                Ok(IrShellStmt::Comment {
                    comment,
                    span: s(span),
                })
            }
            AstStmt::Let { stmt, span } => {
                let ir = IrLetStmt::lower(stmt, file, ctx)?;
                Ok(IrShellStmt::Let {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstStmt::Assign { stmt, span } => {
                let ir = IrAssignStmt::lower(stmt, file, ctx)?;
                Ok(IrShellStmt::Assign {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstStmt::Expr { expr, span } => {
                let ir = IrExpr::lower(expr, file, ctx)?;
                Ok(IrShellStmt::Expr {
                    expr: ir,
                    span: s(span),
                })
            }
            AstStmt::Send { payload, span } => {
                let ir = IrInterpolation::lower(payload, file, ctx)?;
                Ok(IrShellStmt::Send {
                    payload: ir,
                    span: s(span),
                })
            }
            AstStmt::SendRaw { payload, span } => {
                let ir = IrInterpolation::lower(payload, file, ctx)?;
                Ok(IrShellStmt::SendRaw {
                    payload: ir,
                    span: s(span),
                })
            }
            AstStmt::MatchRegex { pattern, span } => {
                let ir_pattern = IrInterpolation::lower(pattern, file, ctx)?;
                super::regex_validate::validate_static_regex(pattern, file)?;
                Ok(IrShellStmt::MatchRegex {
                    pattern: ir_pattern,
                    span: s(span),
                })
            }
            AstStmt::MatchLiteral { pattern, span } => {
                let ir = IrInterpolation::lower(pattern, file, ctx)?;
                Ok(IrShellStmt::MatchLiteral {
                    pattern: ir,
                    span: s(span),
                })
            }
            AstStmt::TimedMatchRegex {
                timeout,
                pattern,
                span,
            } => {
                let ir_timeout = IrTimeout::lower(timeout, file, ctx)?;
                let ir_pattern = IrInterpolation::lower(&pattern.node, file, ctx)?;
                super::regex_validate::validate_static_regex(&pattern.node, file)?;
                Ok(IrShellStmt::TimedMatchRegex {
                    timeout: ir_timeout,
                    pattern: ir_pattern,
                    span: s(span),
                })
            }
            AstStmt::TimedMatchLiteral {
                timeout,
                pattern,
                span,
            } => {
                let ir_timeout = IrTimeout::lower(timeout, file, ctx)?;
                let ir_pattern = IrInterpolation::lower(&pattern.node, file, ctx)?;
                Ok(IrShellStmt::TimedMatchLiteral {
                    timeout: ir_timeout,
                    pattern: ir_pattern,
                    span: s(span),
                })
            }
            AstStmt::Timeout { timeout, span } => {
                let ir = IrTimeout::lower(timeout, file, ctx)?;
                Ok(IrShellStmt::Timeout {
                    timeout: ir,
                    span: s(span),
                })
            }
            AstStmt::FailRegex { pattern, span } => {
                let ir = IrInterpolation::lower(pattern, file, ctx)?;
                super::regex_validate::validate_static_regex(pattern, file)?;
                Ok(IrShellStmt::FailRegex {
                    pattern: ir,
                    span: s(span),
                })
            }
            AstStmt::FailLiteral { pattern, span } => {
                let ir = IrInterpolation::lower(pattern, file, ctx)?;
                Ok(IrShellStmt::FailLiteral {
                    pattern: ir,
                    span: s(span),
                })
            }
            AstStmt::ClearFailPattern { span } => {
                Ok(IrShellStmt::ClearFailPattern { span: s(span) })
            }
            AstStmt::BufferReset { span } => Ok(IrShellStmt::BufferReset { span: s(span) }),
        }
    }
}

impl IrNodeLowering for IrPureStmt {
    type Ast = AstStmt;
    fn lower(
        ast: &AstStmt,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let s = |span: &crate::Span| IrSpan::new(file.clone(), *span);
        match ast {
            AstStmt::Comment { span, .. } => {
                let comment = IrComment::lower(span, file, ctx)?;
                Ok(IrPureStmt::Comment {
                    comment,
                    span: s(span),
                })
            }
            AstStmt::Let { stmt, span } => {
                let ir = IrPureLetStmt::lower(stmt, file, ctx)?;
                Ok(IrPureStmt::Let {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstStmt::Assign { stmt, span } => {
                let ir = IrPureAssignStmt::lower(stmt, file, ctx)?;
                Ok(IrPureStmt::Assign {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstStmt::Expr { expr, span } => {
                let ir = IrPureExpr::lower(expr, file, ctx)?;
                Ok(IrPureStmt::Expr {
                    expr: ir,
                    span: s(span),
                })
            }
            // All shell operators are purity violations
            AstStmt::Send { span, .. }
            | AstStmt::SendRaw { span, .. }
            | AstStmt::MatchRegex { span, .. }
            | AstStmt::MatchLiteral { span, .. }
            | AstStmt::TimedMatchRegex { span, .. }
            | AstStmt::TimedMatchLiteral { span, .. }
            | AstStmt::Timeout { span, .. }
            | AstStmt::FailRegex { span, .. }
            | AstStmt::FailLiteral { span, .. }
            | AstStmt::ClearFailPattern { span }
            | AstStmt::BufferReset { span } => {
                Err(LoweringBail::invalid(InvalidReport::PurityViolation {
                    span: s(span),
                }))
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::interpolation::IrStringPart;
    use super::super::timeout::IrTimeout;
    use super::*;
    use crate::core::table::FileId;
    use crate::diagnostics::LoweringBail;
    use crate::dsl::parser::ast::*;
    use crate::dsl::resolver::lower::test_helpers::*;
    use std::path::PathBuf;
    use std::time::Duration;

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
    fn ir_shell_stmt_send() {
        let s = test_span();
        let stmt = IrShellStmt::Send {
            payload: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::Send { .. }));
    }

    #[test]
    fn ir_shell_stmt_send_raw() {
        let s = test_span();
        let stmt = IrShellStmt::SendRaw {
            payload: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::SendRaw { .. }));
    }

    #[test]
    fn ir_shell_stmt_match_regex() {
        let s = test_span();
        let stmt = IrShellStmt::MatchRegex {
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::MatchRegex { .. }));
    }

    #[test]
    fn ir_shell_stmt_match_literal() {
        let s = test_span();
        let stmt = IrShellStmt::MatchLiteral {
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::MatchLiteral { .. }));
    }

    #[test]
    fn ir_shell_stmt_timed_match_regex() {
        let s = test_span();
        let timeout = IrTimeout::Tolerance {
            duration: Duration::from_secs(5),
            multiplier: 1.0,
            span: s.clone(),
        };
        let stmt = IrShellStmt::TimedMatchRegex {
            timeout,
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::TimedMatchRegex { .. }));
    }

    #[test]
    fn ir_shell_stmt_timed_match_literal() {
        let s = test_span();
        let timeout = IrTimeout::Assertion {
            duration: Duration::from_secs(2),
            span: s.clone(),
        };
        let stmt = IrShellStmt::TimedMatchLiteral {
            timeout,
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::TimedMatchLiteral { .. }));
    }

    #[test]
    fn ir_shell_stmt_timeout() {
        let s = test_span();
        let timeout = IrTimeout::Tolerance {
            duration: Duration::from_secs(10),
            multiplier: 1.0,
            span: s.clone(),
        };
        let stmt = IrShellStmt::Timeout { timeout, span: s };
        assert!(matches!(stmt, IrShellStmt::Timeout { .. }));
    }

    #[test]
    fn ir_shell_stmt_fail_regex() {
        let s = test_span();
        let stmt = IrShellStmt::FailRegex {
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::FailRegex { .. }));
    }

    #[test]
    fn ir_shell_stmt_fail_literal() {
        let s = test_span();
        let stmt = IrShellStmt::FailLiteral {
            pattern: IrInterpolation::new(vec![], s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::FailLiteral { .. }));
    }

    #[test]
    fn ir_shell_stmt_clear_fail_pattern() {
        let stmt = IrShellStmt::ClearFailPattern { span: test_span() };
        assert!(matches!(stmt, IrShellStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn ir_shell_stmt_buffer_reset() {
        let stmt = IrShellStmt::BufferReset { span: test_span() };
        assert!(matches!(stmt, IrShellStmt::BufferReset { .. }));
    }

    #[test]
    fn ir_shell_stmt_let() {
        let s = test_span();
        let stmt = IrShellStmt::Let {
            stmt: IrLetStmt::new(test_ident("x"), None, s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::Let { .. }));
    }

    #[test]
    fn ir_shell_stmt_assign() {
        let s = test_span();
        let val = IrExpr::Var {
            name: "y".into(),
            span: s.clone(),
        };
        let stmt = IrShellStmt::Assign {
            stmt: IrAssignStmt::new(test_ident("x"), val, s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrShellStmt::Assign { .. }));
    }

    #[test]
    fn ir_shell_stmt_expr() {
        let s = test_span();
        let expr = IrExpr::Var {
            name: "x".into(),
            span: s.clone(),
        };
        let stmt = IrShellStmt::Expr { expr, span: s };
        assert!(matches!(stmt, IrShellStmt::Expr { .. }));
    }

    #[test]
    fn ir_pure_stmt_let() {
        let s = test_span();
        let stmt = IrPureStmt::Let {
            stmt: IrPureLetStmt::new(test_ident("x"), None, s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrPureStmt::Let { .. }));
    }

    #[test]
    fn ir_pure_stmt_assign() {
        let s = test_span();
        let val = IrPureExpr::Var {
            name: "y".into(),
            span: s.clone(),
        };
        let stmt = IrPureStmt::Assign {
            stmt: IrPureAssignStmt::new(test_ident("x"), val, s.clone()),
            span: s,
        };
        assert!(matches!(stmt, IrPureStmt::Assign { .. }));
    }

    #[test]
    fn ir_pure_stmt_expr() {
        let s = test_span();
        let expr = IrPureExpr::Var {
            name: "x".into(),
            span: s.clone(),
        };
        let stmt = IrPureStmt::Expr { expr, span: s };
        assert!(matches!(stmt, IrPureStmt::Expr { .. }));
    }

    // ─── Shell statement lowering ─────────────────────────────

    #[test]
    fn lower_let_stmt_with_value() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  let x = \"val\"\n}\n");
        if let crate::dsl::parser::ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
            let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
            assert_eq!(ir.name().name(), "x");
            assert!(ir.value().is_some());
        } else {
            panic!("expected Let");
        }
    }

    #[test]
    fn lower_let_stmt_no_value() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  let x\n}\n");
        if let crate::dsl::parser::ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
            let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
            assert_eq!(ir.name().name(), "x");
            assert!(ir.value().is_none());
        } else {
            panic!("expected Let");
        }
    }

    #[test]
    fn lower_let_stmt_with_call_value() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  let x = trim(\"v\")\n}\n");
        if let crate::dsl::parser::ast::AstStmt::Let { stmt: let_stmt, .. } = &stmt {
            let ir = IrLetStmt::lower(let_stmt, &file, &mut ctx).unwrap();
            assert!(matches!(ir.value(), Some(IrExpr::Call { .. })));
        } else {
            panic!("expected Let");
        }
    }

    #[test]
    fn lower_assign_stmt_basic() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  x = \"val\"\n}\n");
        if let crate::dsl::parser::ast::AstStmt::Assign {
            stmt: assign_stmt, ..
        } = &stmt
        {
            let ir = IrAssignStmt::lower(assign_stmt, &file, &mut ctx).unwrap();
            assert_eq!(ir.name().name(), "x");
        } else {
            panic!("expected Assign");
        }
    }

    #[test]
    fn lower_assign_stmt_with_call() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  x = trim(\"v\")\n}\n");
        if let crate::dsl::parser::ast::AstStmt::Assign {
            stmt: assign_stmt, ..
        } = &stmt
        {
            let ir = IrAssignStmt::lower(assign_stmt, &file, &mut ctx).unwrap();
            assert!(matches!(ir.value(), IrExpr::Call { .. }));
        } else {
            panic!("expected Assign");
        }
    }

    #[test]
    fn lower_send_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  > cmd\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::Send { .. }));
    }

    #[test]
    fn lower_send_stmt_with_var() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  > echo ${x}\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::Send { payload, .. } = &ir {
            assert!(
                payload
                    .parts()
                    .iter()
                    .any(|p| matches!(p, IrStringPart::Var { .. }))
            );
        } else {
            panic!("expected Send");
        }
    }

    #[test]
    fn lower_send_raw_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  => raw\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::SendRaw { .. }));
    }

    #[test]
    fn lower_match_regex_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? pattern\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::MatchRegex { .. }));
    }

    #[test]
    fn lower_match_regex_with_var() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? ${prefix}.*\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::MatchRegex { pattern, .. } = &ir {
            assert!(
                pattern
                    .parts()
                    .iter()
                    .any(|p| matches!(p, IrStringPart::Var { .. }))
            );
        } else {
            panic!("expected MatchRegex");
        }
    }

    #[test]
    fn lower_match_literal_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <= text\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::MatchLiteral { .. }));
    }

    #[test]
    fn lower_match_literal_with_var() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <= ${expected}\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::MatchLiteral { pattern, .. } = &ir {
            assert!(
                pattern
                    .parts()
                    .iter()
                    .any(|p| matches!(p, IrStringPart::Var { .. }))
            );
        } else {
            panic!("expected MatchLiteral");
        }
    }

    #[test]
    fn lower_timed_match_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <~5s? pattern\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::TimedMatchRegex { .. }));
    }

    #[test]
    fn lower_timed_match_literal() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <~5s= text\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::TimedMatchLiteral { .. }));
    }

    #[test]
    fn lower_timed_match_milliseconds() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <~500ms? pat\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::TimedMatchRegex { timeout, .. } = &ir {
            assert_eq!(
                timeout.raw_duration(),
                std::time::Duration::from_millis(500)
            );
        } else {
            panic!("expected TimedMatchRegex");
        }
    }

    #[test]
    fn lower_timed_match_assertion() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <@2s? pat\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::TimedMatchRegex { timeout, .. } = &ir {
            assert!(timeout.is_assertion());
        } else {
            panic!("expected TimedMatchRegex");
        }
    }

    #[test]
    fn lower_timeout_stmt_tolerance() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  ~10s\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::Timeout { timeout, .. } = &ir {
            assert!(!timeout.is_assertion());
            assert_eq!(timeout.raw_duration(), std::time::Duration::from_secs(10));
        } else {
            panic!("expected Timeout, got {:?}", ir);
        }
    }

    #[test]
    fn lower_timeout_stmt_assertion() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  @5s\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        if let IrShellStmt::Timeout { timeout, .. } = &ir {
            assert!(timeout.is_assertion());
            assert_eq!(timeout.raw_duration(), std::time::Duration::from_secs(5));
        } else {
            panic!("expected Timeout, got {:?}", ir);
        }
    }

    #[test]
    fn lower_fail_regex_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !? pattern\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::FailRegex { .. }));
    }

    #[test]
    fn lower_fail_literal_stmt() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  != text\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::FailLiteral { .. }));
    }

    #[test]
    fn lower_buffer_reset_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <?\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::BufferReset { .. }));
    }

    #[test]
    fn lower_buffer_reset_literal() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <=\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::BufferReset { .. }));
    }

    #[test]
    fn lower_clear_fail_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !?\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn lower_clear_fail_literal() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !=\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrShellStmt::ClearFailPattern { .. }));
    }

    #[test]
    fn lower_comments_pass_through() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let module = parse_module("fn t() {\n  // a comment\n  > cmd\n}\n");
        if let AstItem::Fn { def, .. } = &module.items[0].node {
            let stmts: Vec<IrShellStmt> = def
                .body
                .iter()
                .map(|s| IrShellStmt::lower(&s.node, &file, &mut ctx).unwrap())
                .collect();
            assert_eq!(stmts.len(), 2);
            assert!(matches!(stmts[0], IrShellStmt::Comment { .. }));
            assert!(matches!(stmts[1], IrShellStmt::Send { .. }));
        } else {
            panic!("expected fn");
        }
    }

    // ─── Pure statement lowering ──────────────────────────────

    #[test]
    fn lower_pure_stmt_let() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  let x = \"v\"\n}\n");
        let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrPureStmt::Let { .. }));
    }

    #[test]
    fn lower_pure_stmt_assign() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  x = \"v\"\n}\n");
        let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrPureStmt::Assign { .. }));
    }

    #[test]
    fn lower_pure_stmt_expr() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  trim(\"v\")\n}\n");
        let ir = IrPureStmt::lower(&stmt, &file, &mut ctx).unwrap();
        assert!(matches!(ir, IrPureStmt::Expr { .. }));
    }

    #[test]
    fn lower_pure_stmt_rejects_send() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  > cmd\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_send_raw() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  => cmd\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_match_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? pat\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_match_literal() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <= text\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_timed_match() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <~5s? pat\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_timeout() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  ~10s\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_fail_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !? pat\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_fail_literal() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  != text\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_buffer_reset() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <?\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_pure_stmt_rejects_clear_fail() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !?\n}\n");
        let result = IrPureStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }
}
