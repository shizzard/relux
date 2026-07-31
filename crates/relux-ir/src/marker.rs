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
    /// The marker's action **applied** - the marker did what its
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

/// A marker's condition lowered to env-independent IR. Regex patterns are held
/// as their lowered interpolation (an `IrPureExpr::String`); the regex is only
/// compiled at decision time, because the resolved pattern string - and thus
/// the regex's validity - can depend on the env.
#[derive(Debug, Clone)]
pub enum IrMarkerCond {
    /// `# skip` / `# flaky` / `# run` with no condition.
    Unconditional,
    /// `<expr>` - truthy if the evaluated value is non-empty.
    Bare { expr: IrPureExpr },
    /// `<lhs> = <rhs>` - literal match: met when `lhs` contains `rhs`
    /// (substring), not exact equality. Anchor a regex for exact.
    /// `cond_span` locates the `lhs = rhs` body for diagnostics, mirroring
    /// `Regex`'s `pattern_span`.
    Eq {
        lhs: IrPureExpr,
        rhs: IrPureExpr,
        cond_span: IrSpan,
    },
    /// `<expr> =~ /<pattern>/`. `pattern` is the interpolation lowered into an
    /// `IrPureExpr::String`; `pattern_span` locates the `/.../` for diagnostics.
    Regex {
        expr: IrPureExpr,
        pattern: IrPureExpr,
        pattern_span: IrSpan,
    },
}

/// One marker, lowered but not yet decided. `decide_markers` evaluates the
/// `cond` against an env to produce a `MarkerRecording` and skip/flaky verdict.
#[derive(Debug, Clone)]
pub struct IrMarker {
    pub marker_span: IrSpan,
    pub kind: MarkerEvalKind,
    pub modifier: MarkerEvalModifier,
    pub cond: IrMarkerCond,
}

/// The env-dependent outcome of deciding a definition's markers against one env.
/// (Same shape as `MarkerResult`; named distinctly to mark the split.)
#[derive(Debug, Clone)]
pub struct MarkerDecision {
    pub skip: Option<SkipReport>,
    pub flaky: bool,
    pub recordings: Vec<MarkerRecording>,
}

/// Lower a definition's markers to env-independent IR. Bails `Invalid` only on
/// env-independent failures (undefined fn, cycle) surfaced by `IrPureExpr::lower`
/// / `IrInterpolation::lower`. Does not evaluate or compile regexes.
pub fn lower_markers(
    markers: &[relux_core::Spanned<AstMarkerDecl>],
    file_id: &FileId,
    ctx: &mut LoweringContext,
) -> Result<Vec<IrMarker>, LoweringBail> {
    let mut lowered = Vec::new();
    for marker in markers {
        let decl = &marker.node;
        let marker_span = IrSpan::new(file_id.clone(), decl.span);

        let kind = match &decl.kind {
            AstMarkerKind::Skip { .. } => MarkerEvalKind::Skip,
            AstMarkerKind::Run { .. } => MarkerEvalKind::Run,
            AstMarkerKind::Flaky { .. } => MarkerEvalKind::Flaky,
        };

        let Some(condition) = &decl.condition else {
            lowered.push(IrMarker {
                marker_span,
                kind,
                modifier: MarkerEvalModifier::If,
                cond: IrMarkerCond::Unconditional,
            });
            continue;
        };

        let modifier = if matches!(&condition.modifier, AstCondModifier::Unless { .. }) {
            MarkerEvalModifier::Unless
        } else {
            MarkerEvalModifier::If
        };

        let cond = match &condition.body {
            AstMarkerCondBody::Bare { expr, .. } => IrMarkerCond::Bare {
                expr: IrPureExpr::lower(expr, file_id, ctx)?,
            },
            AstMarkerCondBody::Eq { lhs, rhs, span } => IrMarkerCond::Eq {
                lhs: IrPureExpr::lower(lhs, file_id, ctx)?,
                rhs: IrPureExpr::lower(rhs, file_id, ctx)?,
                cond_span: IrSpan::new(file_id.clone(), *span),
            },
            AstMarkerCondBody::Regex {
                expr,
                pattern,
                span,
            } => {
                let expr = IrPureExpr::lower(expr, file_id, ctx)?;
                let ir_interp = IrInterpolation::lower(pattern, file_id, ctx)?;
                let pattern = IrPureExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file_id.clone(), pattern.span),
                };
                IrMarkerCond::Regex {
                    expr,
                    pattern,
                    pattern_span: IrSpan::new(file_id.clone(), *span),
                }
            }
        };

        lowered.push(IrMarker {
            marker_span,
            kind,
            modifier,
            cond,
        });
    }
    Ok(lowered)
}

/// Decide a definition's lowered markers against `env`. Produces the recordings
/// and skip/flaky verdict; compiles marker regexes from the resolved pattern
/// (an invalid pattern is a `LoweringBail::Invalid`, matching current behavior).
pub fn decide_markers(
    lowered: &[IrMarker],
    definition: DefinitionRef,
    env: &Arc<LayeredEnv>,
    fns: &crate::PureFnTable,
) -> Result<MarkerDecision, LoweringBail> {
    let mut flaky = false;
    let mut recordings: Vec<MarkerRecording> = Vec::new();

    for marker in lowered {
        let marker_span = marker.marker_span.clone();
        let kind = marker.kind;

        // Determine marker kind: skip, run, or flaky
        enum MarkerAction {
            Skip,
            Run,
            Flaky,
        }
        let action = match kind {
            MarkerEvalKind::Skip => MarkerAction::Skip,
            MarkerEvalKind::Run => MarkerAction::Run,
            MarkerEvalKind::Flaky => MarkerAction::Flaky,
        };

        let IrMarkerCond::Unconditional = &marker.cond else {
            let negate = matches!(marker.modifier, MarkerEvalModifier::Unless);

            let mut recording = crate::pure_sink::RecordingSink::default();
            let (mut met, evaluation) = match &marker.cond {
                IrMarkerCond::Unconditional => unreachable!(),
                IrMarkerCond::Bare { expr } => {
                    let value = crate::evaluator::eval_pure_expr(
                        expr,
                        &relux_core::pure::VarScope::new(),
                        env,
                        fns,
                        &mut recording,
                    );
                    let met = !value.is_empty();
                    (met, SkipEvaluation::Bare { value, met })
                }
                IrMarkerCond::Eq {
                    lhs,
                    rhs,
                    cond_span,
                } => {
                    let vars = relux_core::pure::VarScope::new();
                    let lhs_val =
                        crate::evaluator::eval_pure_expr(lhs, &vars, env, fns, &mut recording);
                    let rhs_val =
                        crate::evaluator::eval_pure_expr(rhs, &vars, env, fns, &mut recording);
                    // Literal mode never errors, so the Result/Option unwraps cleanly.
                    let met = crate::eval_pure_match(
                        &mut recording,
                        &lhs_val,
                        &rhs_val,
                        false,
                        cond_span,
                    )
                    .expect("literal pure_match cannot fail")
                    .is_some();
                    (
                        met,
                        SkipEvaluation::PureMatch {
                            value: lhs_val,
                            pattern: rhs_val,
                            is_regex: false,
                            met,
                        },
                    )
                }
                IrMarkerCond::Regex {
                    expr,
                    pattern,
                    pattern_span,
                } => {
                    let vars = relux_core::pure::VarScope::new();
                    let value =
                        crate::evaluator::eval_pure_expr(expr, &vars, env, fns, &mut recording);
                    let pattern_str =
                        crate::evaluator::eval_pure_expr(pattern, &vars, env, fns, &mut recording);

                    let hit = crate::eval_pure_match(
                        &mut recording,
                        &value,
                        &pattern_str,
                        true,
                        pattern_span,
                    )
                    .map_err(|e| {
                        LoweringBail::invalid(InvalidReport::invalid_regex(
                            e.pattern,
                            e.reason,
                            pattern_span.clone(),
                        ))
                    })?;
                    let met = hit.is_some();

                    (
                        met,
                        SkipEvaluation::PureMatch {
                            value,
                            pattern: pattern_str,
                            is_regex: true,
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
                modifier: marker.modifier,
                evaluation: evaluation.clone(),
                decision,
                ops: std::mem::take(&mut recording.ops),
            });

            match action {
                MarkerAction::Skip | MarkerAction::Run => {
                    let should_skip = match action {
                        MarkerAction::Skip => met,
                        MarkerAction::Run => !met,
                        MarkerAction::Flaky => unreachable!(),
                    };
                    if should_skip {
                        return Ok(MarkerDecision {
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
            continue;
        };

        // No condition - unconditional marker. `# skip` and
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
                return Ok(MarkerDecision {
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
    }

    Ok(MarkerDecision {
        skip: None,
        flaky,
        recordings,
    })
}
