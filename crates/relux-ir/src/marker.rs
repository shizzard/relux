use std::sync::Arc;

use relux_ast::AstCondModifier;
use relux_ast::AstMarkerCondBody;
use relux_ast::AstMarkerDecl;
use relux_ast::AstMarkerKind;
use relux_core::diagnostics::DefinitionRef;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::SkipEvaluation;
use relux_core::diagnostics::SkipReport;
use relux_core::pure::LayeredEnv;
use relux_core::table::FileId;

use super::IrNodeLowering;
use super::LoweringContext;
use super::expr::IrPureExpr;
use super::interpolation::IrInterpolation;

/// Result of evaluating condition markers on a definition.
pub(crate) struct MarkerResult {
    pub skip: Option<SkipReport>,
    pub flaky: bool,
}

/// Evaluate condition markers on a definition.
///
/// Requires: scope already pushed on `ctx` (for resolving pure fn calls).
/// Returns `Ok(MarkerResult)` with skip/flaky status. `skip` is `Some` if a
/// marker triggers skip. `flaky` is true if a flaky marker's condition is met.
/// Returns `Err(LoweringBail)` if marker lowering fails (e.g., invalid regex, undefined fn).
pub(crate) fn eval_marker(
    markers: &[relux_core::Spanned<AstMarkerDecl>],
    definition: DefinitionRef,
    env: &Arc<LayeredEnv>,
    file_id: &FileId,
    ctx: &mut LoweringContext,
) -> Result<MarkerResult, LoweringBail> {
    let fns = ctx.pure_functions().clone();
    let mut flaky = false;

    for marker in markers {
        let decl = &marker.node;
        let marker_span = IrSpan::new(file_id.clone(), decl.span);

        // Determine marker kind: skip, run, or flaky
        enum MarkerAction {
            Skip,
            Run,
            Flaky,
        }
        let action = match &decl.kind {
            AstMarkerKind::Skip { .. } => MarkerAction::Skip,
            AstMarkerKind::Run { .. } => MarkerAction::Run,
            AstMarkerKind::Flaky { .. } => MarkerAction::Flaky,
        };

        let Some(condition) = &decl.condition else {
            // No condition — unconditional marker
            match action {
                MarkerAction::Skip => {
                    return Ok(MarkerResult {
                        skip: Some(SkipReport {
                            definition,
                            marker_span,
                            evaluation: SkipEvaluation::Unconditional,
                        }),
                        flaky,
                    });
                }
                MarkerAction::Run => continue, // @run with no condition = always run
                MarkerAction::Flaky => {
                    flaky = true;
                    continue;
                }
            }
        };

        let negate = matches!(&condition.modifier, AstCondModifier::Unless { .. });

        let (mut met, evaluation) = match &condition.body {
            AstMarkerCondBody::Bare { expr, .. } => {
                let ir_expr = IrPureExpr::lower(expr, file_id, ctx)?;
                let value = crate::evaluator::eval_pure_expr(
                    &ir_expr,
                    &relux_core::pure::VarScope::new(),
                    env,
                    &fns,
                );
                let met = !value.is_empty();
                (met, SkipEvaluation::Bare { value, met })
            }
            AstMarkerCondBody::Eq { lhs, rhs, .. } => {
                let ir_lhs = IrPureExpr::lower(lhs, file_id, ctx)?;
                let ir_rhs = IrPureExpr::lower(rhs, file_id, ctx)?;
                let vars = relux_core::pure::VarScope::new();
                let lhs_val = crate::evaluator::eval_pure_expr(&ir_lhs, &vars, env, &fns);
                let rhs_val = crate::evaluator::eval_pure_expr(&ir_rhs, &vars, env, &fns);
                let met = lhs_val == rhs_val;
                (
                    met,
                    SkipEvaluation::Eq {
                        lhs: lhs_val,
                        rhs: rhs_val,
                        met,
                    },
                )
            }
            AstMarkerCondBody::Regex {
                expr,
                pattern,
                span,
            } => {
                let ir_expr = IrPureExpr::lower(expr, file_id, ctx)?;
                let vars = relux_core::pure::VarScope::new();
                let value = crate::evaluator::eval_pure_expr(&ir_expr, &vars, env, &fns);

                // Lower pattern as interpolation, wrap in IrPureExpr to evaluate
                let ir_interp = IrInterpolation::lower(pattern, file_id, ctx)?;
                let pattern_expr = IrPureExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file_id.clone(), pattern.span),
                };
                let pattern_str = crate::evaluator::eval_pure_expr(&pattern_expr, &vars, env, &fns);

                let regex = regex::Regex::new(&pattern_str).map_err(|e| {
                    LoweringBail::invalid(InvalidReport::invalid_regex(
                        pattern_str.clone(),
                        e.to_string(),
                        IrSpan::new(file_id.clone(), *span),
                    ))
                })?;

                let met = regex.is_match(&value);
                (
                    met,
                    SkipEvaluation::Regex {
                        value,
                        pattern: pattern_str,
                        met,
                    },
                )
            }
        };

        if negate {
            met = !met;
        }

        match action {
            MarkerAction::Skip | MarkerAction::Run => {
                let should_skip = match action {
                    MarkerAction::Skip => met,
                    MarkerAction::Run => !met,
                    _ => unreachable!(),
                };
                if should_skip {
                    return Ok(MarkerResult {
                        skip: Some(SkipReport {
                            definition,
                            marker_span,
                            evaluation,
                        }),
                        flaky,
                    });
                }
            }
            MarkerAction::Flaky => {
                if met {
                    flaky = true;
                }
            }
        }
    }

    Ok(MarkerResult { skip: None, flaky })
}
