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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerEvalKind {
    Skip,
    Run,
    Flaky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerEvalModifier {
    If,
    Unless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerEvalDecision {
    /// The marker's action did **not** apply.
    Pass,
    /// The marker's action **applied** — the marker did what its
    /// kind says: skip-as-skip, run-as-run, flaky-as-flaky.
    Mark,
}

/// One marker's lowering-time evaluation, replayed by the runtime
/// under a synthetic `marker-eval` span. `ops` carries the pure-eval
/// sink trace (fn-call enter/leave, interpolations, string matches).
#[derive(Debug, Clone)]
pub struct MarkerRecording {
    pub marker_span: IrSpan,
    pub kind: MarkerEvalKind,
    pub modifier: MarkerEvalModifier,
    pub evaluation: SkipEvaluation,
    pub decision: MarkerEvalDecision,
    pub ops: Vec<crate::pure_sink::SinkOp>,
}

/// Result of evaluating condition markers on a definition.
pub struct MarkerResult {
    pub skip: Option<SkipReport>,
    pub flaky: bool,
    pub recordings: Vec<MarkerRecording>,
}

/// Evaluate condition markers on a definition.
///
/// Requires: scope already pushed on `ctx` (for resolving pure fn calls).
/// Returns `Ok(MarkerResult)` with skip/flaky status. `skip` is `Some` if a
/// marker triggers skip. `flaky` is true if a flaky marker's condition is met.
/// Returns `Err(LoweringBail)` if marker lowering fails (e.g., invalid regex, undefined fn).
pub fn eval_marker(
    markers: &[relux_core::Spanned<AstMarkerDecl>],
    definition: DefinitionRef,
    env: &Arc<LayeredEnv>,
    file_id: &FileId,
    ctx: &mut LoweringContext,
) -> Result<MarkerResult, LoweringBail> {
    let fns = ctx.pure_functions().clone();
    let mut flaky = false;
    let mut recordings: Vec<MarkerRecording> = Vec::new();

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
        let kind = match action {
            MarkerAction::Skip => MarkerEvalKind::Skip,
            MarkerAction::Run => MarkerEvalKind::Run,
            MarkerAction::Flaky => MarkerEvalKind::Flaky,
        };

        let Some(condition) = &decl.condition else {
            // No condition — unconditional marker. `# skip` and
            // `# flaky` always apply; `# run` (no condition) is a
            // no-op per the docs.
            let decision = match action {
                MarkerAction::Skip => MarkerEvalDecision::Mark,
                MarkerAction::Run => MarkerEvalDecision::Pass,
                MarkerAction::Flaky => MarkerEvalDecision::Mark,
            };
            recordings.push(MarkerRecording {
                marker_span: marker_span.clone(),
                kind,
                modifier: MarkerEvalModifier::If,
                evaluation: SkipEvaluation::Unconditional,
                decision,
                ops: Vec::new(),
            });
            match action {
                MarkerAction::Skip => {
                    return Ok(MarkerResult {
                        skip: Some(SkipReport {
                            definition,
                            marker_span,
                            evaluation: SkipEvaluation::Unconditional,
                        }),
                        flaky,
                        recordings,
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
        let modifier = if negate {
            MarkerEvalModifier::Unless
        } else {
            MarkerEvalModifier::If
        };

        let mut recording = crate::pure_sink::RecordingSink::default();
        let (mut met, evaluation) = match &condition.body {
            AstMarkerCondBody::Bare { expr, .. } => {
                let ir_expr = IrPureExpr::lower(expr, file_id, ctx)?;
                let value = crate::evaluator::eval_pure_expr(
                    &ir_expr,
                    &relux_core::pure::VarScope::new(),
                    env,
                    &fns,
                    &mut recording,
                );
                let met = !value.is_empty();
                (met, SkipEvaluation::Bare { value, met })
            }
            AstMarkerCondBody::Eq { lhs, rhs, .. } => {
                let ir_lhs = IrPureExpr::lower(lhs, file_id, ctx)?;
                let ir_rhs = IrPureExpr::lower(rhs, file_id, ctx)?;
                let vars = relux_core::pure::VarScope::new();
                let lhs_val =
                    crate::evaluator::eval_pure_expr(&ir_lhs, &vars, env, &fns, &mut recording);
                let rhs_val =
                    crate::evaluator::eval_pure_expr(&ir_rhs, &vars, env, &fns, &mut recording);
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
                let value =
                    crate::evaluator::eval_pure_expr(&ir_expr, &vars, env, &fns, &mut recording);

                // Lower pattern as interpolation, wrap in IrPureExpr to evaluate
                let ir_interp = IrInterpolation::lower(pattern, file_id, ctx)?;
                let pattern_expr = IrPureExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file_id.clone(), pattern.span),
                };
                let pattern_str = crate::evaluator::eval_pure_expr(
                    &pattern_expr,
                    &vars,
                    env,
                    &fns,
                    &mut recording,
                );

                let regex = regex::Regex::new(&pattern_str).map_err(|e| {
                    LoweringBail::invalid(InvalidReport::invalid_regex(
                        pattern_str.clone(),
                        e.to_string(),
                        IrSpan::new(file_id.clone(), *span),
                    ))
                })?;

                let (result_str, captures): (String, std::collections::HashMap<String, String>) =
                    if let Some(cap) = regex.captures(&value) {
                        let mut caps = std::collections::HashMap::new();
                        for i in 0..cap.len() {
                            if let Some(m) = cap.get(i) {
                                caps.insert(i.to_string(), m.as_str().to_string());
                            }
                        }
                        (
                            cap.get(0)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default(),
                            caps,
                        )
                    } else {
                        (String::new(), std::collections::HashMap::new())
                    };

                use crate::pure_sink::PureEvalSink;
                recording.record_match(
                    crate::pure_sink::MatchKind::Regex,
                    &value,
                    &pattern_str,
                    &result_str,
                    &captures,
                    &IrSpan::new(file_id.clone(), *span),
                );

                let met = !result_str.is_empty();
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

        // `met` here is the truthy outcome of the condition after the
        // modifier (`if` keeps it as-is; `unless` was already inverted
        // above). For every kind the rule is the same: if met, the
        // marker's action applies; otherwise it doesn't.
        let decision = if met {
            MarkerEvalDecision::Mark
        } else {
            MarkerEvalDecision::Pass
        };
        recordings.push(MarkerRecording {
            marker_span: marker_span.clone(),
            kind,
            modifier,
            evaluation: evaluation.clone(),
            decision,
            ops: std::mem::take(&mut recording.ops),
        });

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
                        recordings,
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

    Ok(MarkerResult {
        skip: None,
        flaky,
        recordings,
    })
}
