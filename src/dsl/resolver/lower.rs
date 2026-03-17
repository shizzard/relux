use crate::Spanned as AstSpanned;
use crate::config;

use super::*;

pub(super) fn sp(file_id: FileId, span: &parser::Span) -> Span {
    Span::new(file_id, (*span).into())
}

pub(super) fn lower_spanned<T>(file_id: FileId, node: T, span: &parser::Span) -> ir::Spanned<T> {
    ir::Spanned::new(node, sp(file_id, span))
}

pub(super) fn parse_timeout(
    kind: parser::AstTimeoutKind,
    raw: &str,
    multiplier: f64,
    file_id: FileId,
    span: &parser::Span,
    diagnostics: &mut Vec<DiagnosticError>,
) -> ir::Timeout {
    match humantime::parse_duration(raw.trim()) {
        Ok(d) => match kind {
            parser::AstTimeoutKind::Tolerance { .. } => ir::Timeout::Tolerance {
                duration: d,
                multiplier,
            },
            parser::AstTimeoutKind::Assertion { .. } => ir::Timeout::Assertion(d),
        },
        Err(_) => {
            // Point at the duration string (after the `~`/`@` prefix)
            let content_span = (span.start() + 1)..span.end();
            diagnostics.push(DiagnosticError::InvalidTimeout {
                raw: raw.to_string(),
                span: Span::new(file_id, content_span),
            });
            match kind {
                parser::AstTimeoutKind::Tolerance { .. } => ir::Timeout::Tolerance {
                    duration: config::DEFAULT_TIMEOUT,
                    multiplier,
                },
                parser::AstTimeoutKind::Assertion { .. } => {
                    ir::Timeout::Assertion(config::DEFAULT_TIMEOUT)
                }
            }
        }
    }
}

/// Lower an AST interpolation to IR. Escapes are already resolved by the parser.
fn lower_string_expr(
    file_id: FileId,
    ast: &parser::AstInterpolation,
    token_span: &parser::Span,
    prefix_len: usize,
) -> ir::Interpolation {
    let parts = ast
        .parts
        .iter()
        .map(|part| {
            let ir_part = match part {
                parser::AstStringPart::Literal { value, .. } => {
                    ir::StringPart::Literal(value.clone())
                }
                parser::AstStringPart::VarRef { name, .. } => ir::StringPart::VarRef(name.clone()),
                parser::AstStringPart::EscapedDollar { .. } => ir::StringPart::EscapedDollar,
                parser::AstStringPart::CaptureRef { index, .. } => {
                    ir::StringPart::CaptureRef(*index)
                }
            };
            ir::Spanned::new(ir_part, Span::new(file_id, 0..0))
        })
        .collect();
    string_expr_with_span(file_id, parts, token_span, prefix_len)
}

/// Lower an AST interpolation using its own span directly (no prefix/suffix adjustment).
fn lower_string_expr_direct(
    file_id: FileId,
    ast: &parser::AstInterpolation,
    interp_span: &parser::Span,
) -> ir::Interpolation {
    let parts = ast
        .parts
        .iter()
        .map(|part| {
            let ir_part = match part {
                parser::AstStringPart::Literal { value, .. } => {
                    ir::StringPart::Literal(value.clone())
                }
                parser::AstStringPart::VarRef { name, .. } => ir::StringPart::VarRef(name.clone()),
                parser::AstStringPart::EscapedDollar { .. } => ir::StringPart::EscapedDollar,
                parser::AstStringPart::CaptureRef { index, .. } => {
                    ir::StringPart::CaptureRef(*index)
                }
            };
            ir::Spanned::new(ir_part, Span::new(file_id, 0..0))
        })
        .collect();
    ir::Interpolation {
        parts,
        span: Span::new(file_id, interp_span.start()..interp_span.end()),
    }
}

fn string_expr_with_span(
    file_id: FileId,
    parts: Vec<ir::Spanned<ir::StringPart>>,
    token_span: &parser::Span,
    prefix_len: usize,
) -> ir::Interpolation {
    // Payload content starts after the operator prefix and leading space,
    // and excludes the trailing newline consumed by the lexer callback.
    let content_start = token_span.start() + prefix_len + 1;
    let content_end = if token_span.end() > content_start {
        token_span.end().saturating_sub(1)
    } else {
        content_start
    };
    ir::Interpolation {
        parts,
        span: Span::new(file_id, content_start..content_end),
    }
}

pub(super) fn lower_expr(
    ctx: &mut LoweringContext,
    ast: &parser::AstExpr,
    expr_span: &parser::Span,
) -> ir::Expr {
    match ast {
        parser::AstExpr::String { interp, .. } => {
            ir::Expr::String(lower_string_expr(ctx.file_id, interp, expr_span, 0))
        }
        parser::AstExpr::Var { name, .. } => ir::Expr::Var(name.clone()),
        parser::AstExpr::Call { call, .. } => {
            let arity = call.args.len();
            let fn_key = FnKey {
                name: call.name.node.clone(),
                arity,
            };
            if !ctx.scope.functions.contains_key(&fn_key)
                && !ctx.scope.pure_functions.contains_key(&fn_key)
                && !crate::runtime::bifs::is_known(&call.name.node, arity)
            {
                let mut available: Vec<usize> = ctx
                    .scope
                    .functions
                    .keys()
                    .chain(ctx.scope.pure_functions.keys())
                    .filter(|k| k.name == call.name.node)
                    .map(|k| k.arity)
                    .collect();
                available.sort();
                available.dedup();
                ctx.errors.push(DiagnosticError::UndefinedName {
                    name: format!("{}/{}", call.name.node, arity),
                    span: sp(ctx.file_id, &call.name.span),
                    available_arities: available,
                });
            }
            let args = call
                .args
                .iter()
                .map(|a| lower_spanned(ctx.file_id, lower_expr(ctx, &a.node, &a.span), &a.span))
                .collect();
            ir::Expr::Call(ir::FnCall {
                name: lower_spanned(ctx.file_id, call.name.node.clone(), &call.name.span),
                args,
            })
        }
        parser::AstExpr::CaptureRef { index, .. } => ir::Expr::Var(index.to_string()),
    }
}

pub(super) fn lower_stmt(
    ctx: &mut LoweringContext,
    ast: &parser::AstStmt,
    stmt_span: &parser::Span,
) -> Option<ir::Spanned<ir::ShellStmt>> {
    let ir_stmt = match ast {
        parser::AstStmt::Comment { .. } => return None,
        parser::AstStmt::Let { stmt: l, .. } => {
            let value = l
                .value
                .as_ref()
                .map(|v| lower_spanned(ctx.file_id, lower_expr(ctx, &v.node, &v.span), &v.span));
            ir::ShellStmt::Let(ir::VarDecl {
                name: lower_spanned(ctx.file_id, l.name.node.clone(), &l.name.span),
                value,
            })
        }
        parser::AstStmt::Assign { stmt: a, .. } => ir::ShellStmt::Assign(ir::VarAssign {
            name: lower_spanned(ctx.file_id, a.name.node.clone(), &a.name.span),
            value: lower_spanned(
                ctx.file_id,
                lower_expr(ctx, &a.value.node, &a.value.span),
                &a.value.span,
            ),
        }),
        parser::AstStmt::Timeout { kind, duration, .. } => {
            let t = parse_timeout(
                *kind,
                duration,
                ctx.multiplier,
                ctx.file_id,
                stmt_span,
                &mut ctx.errors,
            );
            ir::ShellStmt::Timeout(t)
        }
        parser::AstStmt::FailRegex { pattern, .. } => {
            ir::ShellStmt::FailRegex(lower_string_expr(ctx.file_id, pattern, stmt_span, 0))
        }
        parser::AstStmt::FailLiteral { pattern, .. } => {
            ir::ShellStmt::FailLiteral(lower_string_expr(ctx.file_id, pattern, stmt_span, 0))
        }
        parser::AstStmt::ClearFailPattern { .. } => ir::ShellStmt::ClearFailPattern,
        parser::AstStmt::Send { payload, .. } => ir::ShellStmt::Expr(ir::Expr::Send(
            lower_string_expr(ctx.file_id, payload, stmt_span, 0),
        )),
        parser::AstStmt::SendRaw { payload, .. } => ir::ShellStmt::Expr(ir::Expr::SendRaw(
            lower_string_expr(ctx.file_id, payload, stmt_span, 0),
        )),
        parser::AstStmt::MatchRegex { pattern, .. } => {
            ir::ShellStmt::Expr(ir::Expr::MatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(ctx.file_id, pattern, stmt_span, 0),
                timeout_override: None,
            }))
        }
        parser::AstStmt::MatchLiteral { pattern, .. } => {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(ctx.file_id, pattern, stmt_span, 0),
                timeout_override: None,
            }))
        }
        parser::AstStmt::TimedMatchRegex {
            timeout_kind,
            duration,
            pattern,
            ..
        } => ir::ShellStmt::Expr(ir::Expr::MatchRegex(ir::MatchExpr {
            pattern: lower_string_expr_direct(ctx.file_id, &pattern.node, &pattern.span),
            timeout_override: Some(parse_timeout(
                *timeout_kind,
                duration,
                ctx.multiplier,
                ctx.file_id,
                stmt_span,
                &mut ctx.errors,
            )),
        })),
        parser::AstStmt::TimedMatchLiteral {
            timeout_kind,
            duration,
            pattern,
            ..
        } => ir::ShellStmt::Expr(ir::Expr::MatchLiteral(ir::MatchExpr {
            pattern: lower_string_expr_direct(ctx.file_id, &pattern.node, &pattern.span),
            timeout_override: Some(parse_timeout(
                *timeout_kind,
                duration,
                ctx.multiplier,
                ctx.file_id,
                stmt_span,
                &mut ctx.errors,
            )),
        })),
        parser::AstStmt::BufferReset { .. } => ir::ShellStmt::Expr(ir::Expr::BufferReset),
        parser::AstStmt::Expr { expr, .. } => ir::ShellStmt::Expr(lower_expr(ctx, expr, stmt_span)),
    };
    Some(ir::Spanned::new(ir_stmt, sp(ctx.file_id, stmt_span)))
}

fn lower_cleanup_stmt(
    file_id: FileId,
    ast: &parser::AstStmt,
    stmt_span: &parser::Span,
    multiplier: f64,
    diagnostics: &mut Vec<DiagnosticError>,
) -> Option<ir::Spanned<ir::CleanupStmt>> {
    let empty_scope = ModuleScope {
        functions: LookupTable::new(),
        pure_functions: LookupTable::new(),
        effects: LookupTable::new(),
    };
    let mut ctx = LoweringContext {
        file_id,
        scope: &empty_scope,
        multiplier,
        errors: Vec::new(),
    };
    let ir_stmt = match ast {
        parser::AstStmt::Comment { .. } => return None,
        parser::AstStmt::Send { payload, .. } => {
            ir::CleanupStmt::Send(lower_string_expr(ctx.file_id, payload, stmt_span, 0))
        }
        parser::AstStmt::SendRaw { payload, .. } => {
            ir::CleanupStmt::SendRaw(lower_string_expr(ctx.file_id, payload, stmt_span, 0))
        }
        parser::AstStmt::Let { stmt: l, .. } => {
            let value = l.value.as_ref().map(|v| {
                lower_spanned(ctx.file_id, lower_expr(&mut ctx, &v.node, &v.span), &v.span)
            });
            ir::CleanupStmt::Let(ir::VarDecl {
                name: lower_spanned(ctx.file_id, l.name.node.clone(), &l.name.span),
                value,
            })
        }
        parser::AstStmt::Assign { stmt: a, .. } => ir::CleanupStmt::Assign(ir::VarAssign {
            name: lower_spanned(ctx.file_id, a.name.node.clone(), &a.name.span),
            value: lower_spanned(
                ctx.file_id,
                lower_expr(&mut ctx, &a.value.node, &a.value.span),
                &a.value.span,
            ),
        }),
        _ => {
            diagnostics.push(DiagnosticError::InvalidCleanupStatement {
                span: Span::new(file_id, (*stmt_span).into()),
            });
            return None;
        }
    };
    diagnostics.extend(ctx.errors);
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

// ─── Pure Function Lowering ─────────────────────────────────

fn validate_pure_calls(
    expr: &ir::PureExpr,
    scope: &ModuleScope,
    diagnostics: &mut Vec<DiagnosticError>,
) {
    if let ir::PureExpr::Call(call) = expr {
        let arity = call.args.len();
        let fn_key = FnKey {
            name: call.name.node.clone(),
            arity,
        };
        if !scope.pure_functions.contains_key(&fn_key)
            && !crate::runtime::bifs::is_pure_bif(&call.name.node, arity)
            && (scope.functions.contains_key(&fn_key)
                || crate::runtime::bifs::is_impure_bif(&call.name.node, arity))
        {
            diagnostics.push(DiagnosticError::ImpureInPureContext {
                what: format!("{}/{}", call.name.node, arity),
                span: call.name.span.clone(),
            });
        }
        for arg in &call.args {
            validate_pure_calls(&arg.node, scope, diagnostics);
        }
    }
}

pub(super) fn lower_as_pure_expr(
    ctx: &mut LoweringContext,
    ast: &parser::AstExpr,
    expr_span: &parser::Span,
) -> ir::PureExpr {
    let impure = lower_expr(ctx, ast, expr_span);
    let spanned = ir::Spanned::new(impure, sp(ctx.file_id, expr_span));
    match ir::Spanned::<ir::PureExpr>::try_from(spanned) {
        Ok(pure) => {
            validate_pure_calls(&pure.node, ctx.scope, &mut ctx.errors);
            pure.node
        }
        Err(e) => {
            ctx.errors.push(DiagnosticError::ImpureInPureContext {
                what: e.what,
                span: e.span,
            });
            ir::PureExpr::String(ir::Interpolation {
                parts: vec![],
                span: sp(ctx.file_id, expr_span),
            })
        }
    }
}

fn validate_pure_stmt_calls(
    stmt: &ir::PureStmt,
    scope: &ModuleScope,
    diagnostics: &mut Vec<DiagnosticError>,
) {
    match stmt {
        ir::PureStmt::Let(decl) => {
            if let Some(v) = &decl.value {
                validate_pure_calls(&v.node, scope, diagnostics);
            }
        }
        ir::PureStmt::Assign(assign) => {
            validate_pure_calls(&assign.value.node, scope, diagnostics);
        }
        ir::PureStmt::Expr(e) => {
            validate_pure_calls(e, scope, diagnostics);
        }
    }
}

pub(super) fn lower_as_pure_stmt(
    ctx: &mut LoweringContext,
    ast: &parser::AstStmt,
    stmt_span: &parser::Span,
) -> Option<ir::Spanned<ir::PureStmt>> {
    let shell_stmt = lower_stmt(ctx, ast, stmt_span)?;
    match ir::Spanned::<ir::PureStmt>::try_from(shell_stmt) {
        Ok(pure) => {
            validate_pure_stmt_calls(&pure.node, ctx.scope, &mut ctx.errors);
            Some(pure)
        }
        Err(e) => {
            ctx.errors.push(DiagnosticError::ImpureInPureContext {
                what: e.what,
                span: e.span,
            });
            None
        }
    }
}

pub(super) fn lower_shell_block(
    ctx: &mut LoweringContext,
    block: &parser::AstShellBlock,
    block_span: &parser::Span,
) -> ir::Spanned<ir::ShellBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_stmt(ctx, &s.node, &s.span))
        .collect();
    ir::Spanned::new(
        ir::ShellBlock {
            name: lower_spanned(ctx.file_id, block.name.node.clone(), &block.name.span),
            stmts,
        },
        sp(ctx.file_id, block_span),
    )
}

pub(super) fn lower_cleanup_block(
    file_id: FileId,
    block: &parser::AstCleanupBlock,
    block_span: &parser::Span,
    multiplier: f64,
    diagnostics: &mut Vec<DiagnosticError>,
) -> ir::Spanned<ir::CleanupBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_cleanup_stmt(file_id, &s.node, &s.span, multiplier, diagnostics))
        .collect();
    ir::Spanned::new(ir::CleanupBlock { stmts }, sp(file_id, block_span))
}

pub(super) fn lower_overlay(
    ctx: &mut LoweringContext,
    overlay: &[AstSpanned<parser::AstOverlayEntry>],
) -> Vec<ir::OverlayEntry> {
    overlay
        .iter()
        .map(|e| {
            let value_expr = lower_as_pure_expr(ctx, &e.node.value.node, &e.node.value.span);
            ir::OverlayEntry {
                key: lower_spanned(ctx.file_id, e.node.key.node.clone(), &e.node.key.span),
                value: ir::Spanned::new(value_expr, sp(ctx.file_id, &e.node.value.span)),
            }
        })
        .collect()
}

pub(super) fn lower_marker(
    ctx: &mut LoweringContext,
    m: &parser::AstMarkerDecl,
    marker_span: &parser::Span,
) -> ir::Condition {
    let kind: ir::CondKind = m.kind.clone().into();
    let cond = m.condition.as_ref().map(|c| {
        let modifier: ir::CondModifier = c.modifier.clone().into();
        let body = match &c.body {
            parser::AstMarkerCondBody::Bare { expr, .. } => {
                ir::CondBody::Bare(lower_as_pure_expr(ctx, expr, marker_span))
            }
            parser::AstMarkerCondBody::Eq { lhs, rhs, .. } => ir::CondBody::Eq(
                lower_as_pure_expr(ctx, lhs, marker_span),
                lower_as_pure_expr(ctx, rhs, marker_span),
            ),
            parser::AstMarkerCondBody::Regex {
                expr: lhs,
                pattern: pat_expr,
                ..
            } => {
                let pat_ir = lower_string_expr(ctx.file_id, pat_expr, marker_span, 0);
                // Validate regex if no interpolations
                let has_interps = pat_ir
                    .parts
                    .iter()
                    .any(|p| matches!(p.node, ir::StringPart::VarRef(_)));
                if !has_interps {
                    let literal: String = pat_ir
                        .parts
                        .iter()
                        .filter_map(|p| match &p.node {
                            ir::StringPart::Literal(s) => Some(s.as_str()),
                            ir::StringPart::EscapedDollar => Some("$"),
                            _ => None,
                        })
                        .collect();
                    if let Err(e) = regex::Regex::new(&literal) {
                        ctx.errors.push(DiagnosticError::InvalidRegex {
                            pattern: literal,
                            message: format!("{e}"),
                            span: sp(ctx.file_id, marker_span),
                        });
                    }
                }
                ir::CondBody::Regex(lower_as_pure_expr(ctx, lhs, marker_span), pat_ir)
            }
        };
        ir::CondExpr { modifier, body }
    });
    ir::Condition { kind, cond }
}

pub(super) fn lower_effect_def(
    ctx: &mut LoweringContext,
    def: &parser::AstEffectDef,
) -> ir::Effect {
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(ctx, &m.node, &m.span),
                sp(ctx.file_id, &m.span),
            )
        })
        .collect();
    let mut vars = Vec::new();
    let mut shells = Vec::new();
    let mut cleanup = None;

    for item in &def.body {
        match &item.node {
            parser::AstEffectItem::Comment { .. } => {}
            parser::AstEffectItem::Need { .. } => {} // handled by graph builder
            parser::AstEffectItem::Let { stmt: l, .. } => {
                let value = l.value.as_ref().map(|v| {
                    lower_spanned(
                        ctx.file_id,
                        lower_as_pure_expr(ctx, &v.node, &v.span),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::PureVarDecl {
                        name: lower_spanned(ctx.file_id, l.name.node.clone(), &l.name.span),
                        value,
                    },
                    sp(ctx.file_id, &item.span),
                ));
            }
            parser::AstEffectItem::Shell { block, .. } => {
                shells.push(lower_shell_block(ctx, block, &item.span));
            }
            parser::AstEffectItem::Cleanup { block, .. } => {
                cleanup = Some(lower_cleanup_block(
                    ctx.file_id,
                    block,
                    &item.span,
                    ctx.multiplier,
                    &mut ctx.errors,
                ));
            }
        }
    }

    ir::Effect {
        name: lower_spanned(ctx.file_id, def.name.node.clone(), &def.name.span),
        exported_shell: lower_spanned(
            ctx.file_id,
            def.exported_shell.node.clone(),
            &def.exported_shell.span,
        ),
        conditions,
        vars,
        shells,
        cleanup,
        span: sp(ctx.file_id, &def.name.span),
    }
}

pub(super) fn lower_test_def(
    ctx: &mut LoweringContext,
    def: &parser::AstTestDef,
    test_span: &parser::Span,
    needs: Vec<ir::Spanned<ir::TestNeed>>,
) -> ir::Test {
    let mut doc = None;
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(ctx, &m.node, &m.span),
                sp(ctx.file_id, &m.span),
            )
        })
        .collect();
    let mut vars = Vec::new();
    let mut shells = Vec::new();
    let mut cleanup = None;

    for item in &def.body {
        match &item.node {
            parser::AstTestItem::Comment { .. } => {}
            parser::AstTestItem::DocString { text, .. } => {
                doc = Some(lower_spanned(ctx.file_id, text.clone(), &item.span));
            }
            parser::AstTestItem::Need { .. } => {} // already resolved into `needs`
            parser::AstTestItem::Let { stmt: l, .. } => {
                let value = l.value.as_ref().map(|v| {
                    lower_spanned(
                        ctx.file_id,
                        lower_as_pure_expr(ctx, &v.node, &v.span),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::PureVarDecl {
                        name: lower_spanned(ctx.file_id, l.name.node.clone(), &l.name.span),
                        value,
                    },
                    sp(ctx.file_id, &item.span),
                ));
            }
            parser::AstTestItem::Shell { block, .. } => {
                shells.push(lower_shell_block(ctx, block, &item.span));
            }
            parser::AstTestItem::Cleanup { block, .. } => {
                cleanup = Some(lower_cleanup_block(
                    ctx.file_id,
                    block,
                    &item.span,
                    ctx.multiplier,
                    &mut ctx.errors,
                ));
            }
        }
    }

    let timeout = def.timeout.as_ref().map(|t| {
        let (kind, ref raw) = t.node;
        parse_timeout(
            kind,
            raw,
            ctx.multiplier,
            ctx.file_id,
            &t.span,
            &mut ctx.errors,
        )
    });

    ir::Test {
        name: lower_spanned(ctx.file_id, def.name.node.clone(), &def.name.span),
        timeout,
        doc,
        conditions,
        needs,
        vars,
        shells,
        cleanup,
        span: sp(ctx.file_id, test_span),
    }
}
