use relux_ast::AstAssignStmt;
use relux_ast::AstLetStmt;
use relux_ast::AstStmt;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::comment::IrComment;
use super::expr::IrExpr;
use super::expr::IrPureExpr;
use super::ident::IrIdent;
use super::interpolation::IrInterpolation;
use super::timeout::IrTimeout;

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
        let s = |span: &relux_core::Span| IrSpan::new(file.clone(), *span);
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
        let s = |span: &relux_core::Span| IrSpan::new(file.clone(), *span);
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
            | AstStmt::BufferReset { span } => Err(LoweringBail::invalid(
                InvalidReport::purity_violation(s(span)),
            )),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────
