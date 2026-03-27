use std::sync::Arc;

use crate::core::table::FileId;
use crate::diagnostics::DefinitionRef;
use crate::diagnostics::InvalidReport;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::diagnostics::SkipEvaluation;
use crate::diagnostics::SkipReport;
use crate::dsl::parser::ast::AstCondModifier;
use crate::dsl::parser::ast::AstMarkerCondBody;
use crate::dsl::parser::ast::AstMarkerDecl;
use crate::dsl::parser::ast::AstMarkerKind;
use crate::pure::Env;

use super::IrNodeLowering;
use super::LoweringContext;
use super::expr::IrPureExpr;
use super::interpolation::IrInterpolation;

/// Evaluate condition markers on a definition.
///
/// Requires: scope already pushed on `ctx` (for resolving pure fn calls).
/// Returns `Ok(Some(SkipReport))` if a marker triggers skip, `Ok(None)` otherwise.
/// Returns `Err(LoweringBail)` if marker lowering fails (e.g., invalid regex, undefined fn).
pub(crate) fn eval_marker(
    markers: &[crate::Spanned<AstMarkerDecl>],
    definition: DefinitionRef,
    env: &Arc<Env>,
    file_id: &FileId,
    ctx: &mut LoweringContext,
) -> Result<Option<SkipReport>, LoweringBail> {
    let fns = ctx.pure_functions().clone();

    for marker in markers {
        let decl = &marker.node;
        let marker_span = IrSpan::new(file_id.clone(), decl.span);

        let is_skip = match &decl.kind {
            AstMarkerKind::Skip { .. } => true,
            AstMarkerKind::Run { .. } => false,
            AstMarkerKind::Flaky { .. } => continue,
        };

        let Some(condition) = &decl.condition else {
            // No condition
            if is_skip {
                return Ok(Some(SkipReport {
                    definition,
                    marker_span,
                    evaluation: SkipEvaluation::Unconditional,
                }));
            }
            // @run with no condition = always run
            continue;
        };

        let negate = matches!(&condition.modifier, AstCondModifier::Unless { .. });

        let (mut met, evaluation) = match &condition.body {
            AstMarkerCondBody::Bare { expr, .. } => {
                let ir_expr = IrPureExpr::lower(expr, file_id, ctx)?;
                let value = crate::pure::evaluator::eval_pure_expr(
                    &ir_expr,
                    &crate::pure::VarScope::new(),
                    env,
                    &fns,
                );
                let met = !value.is_empty();
                (met, SkipEvaluation::Bare { value, met })
            }
            AstMarkerCondBody::Eq { lhs, rhs, .. } => {
                let ir_lhs = IrPureExpr::lower(lhs, file_id, ctx)?;
                let ir_rhs = IrPureExpr::lower(rhs, file_id, ctx)?;
                let vars = crate::pure::VarScope::new();
                let lhs_val = crate::pure::evaluator::eval_pure_expr(&ir_lhs, &vars, env, &fns);
                let rhs_val = crate::pure::evaluator::eval_pure_expr(&ir_rhs, &vars, env, &fns);
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
                let vars = crate::pure::VarScope::new();
                let value = crate::pure::evaluator::eval_pure_expr(&ir_expr, &vars, env, &fns);

                // Lower pattern as interpolation, wrap in IrPureExpr to evaluate
                let ir_interp = IrInterpolation::lower(pattern, file_id, ctx)?;
                let pattern_expr = IrPureExpr::String {
                    value: ir_interp,
                    span: IrSpan::new(file_id.clone(), pattern.span),
                };
                let pattern_str =
                    crate::pure::evaluator::eval_pure_expr(&pattern_expr, &vars, env, &fns);

                let regex = regex::Regex::new(&pattern_str).map_err(|e| {
                    LoweringBail::invalid(InvalidReport::InvalidRegex {
                        pattern: pattern_str.clone(),
                        error: e.to_string(),
                        span: IrSpan::new(file_id.clone(), *span),
                    })
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

        let should_skip = if is_skip { met } else { !met };

        if should_skip {
            return Ok(Some(SkipReport {
                definition,
                marker_span,
                evaluation,
            }));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::dsl::resolver::lower::test_helpers::*;

    use std::collections::HashMap;

    // ─── eval_marker: @skip ────────────────────────────────────

    #[test]
    fn marker_skip_unconditional() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 1);
        assert!(is_skipped(&suite.plans[0]));
        assert_eq!(plan_name(&suite.plans[0]), "skipped");
    }

    #[test]
    fn marker_skip_if_bare_truthy() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "yes".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert_eq!(suite.plans.len(), 1);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_bare_falsy() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if MY_VAR
test "skipped" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 1);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_bare_whitespace_is_truthy() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "  ".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR
test "ws" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert_eq!(suite.plans.len(), 1);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_eq_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "expected".into());
        env.insert("EXPECTED".into(), "expected".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR = EXPECTED
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_eq_no_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "other".into());
        env.insert("EXPECTED".into(), "expected".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR = EXPECTED
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_eq_both_empty() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if UNSET_A = UNSET_B
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_eq_one_empty() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "val".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR = UNSET_B
test "eq" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_regex_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "abc123".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR ? \d+
test "rx" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_regex_no_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "abc".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR ? ^\d+$
test "rx" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_regex_empty_value() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if UNSET ? .*
test "rx" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    // ─── eval_marker: @run ─────────────────────────────────────

    #[test]
    fn marker_run_unconditional() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# run
test "always" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_bare_truthy() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "yes".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if MY_VAR
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_bare_falsy() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# run if MY_VAR
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_eq_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "expected".into());
        env.insert("EXPECTED".into(), "expected".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if MY_VAR = EXPECTED
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_eq_no_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "other".into());
        env.insert("EXPECTED".into(), "expected".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if MY_VAR = EXPECTED
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_regex_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "123".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if MY_VAR ? ^\d+$
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_regex_no_match() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "abc".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if MY_VAR ? ^\d+$
test "run" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    // ─── eval_marker: unless ───────────────────────────────────

    #[test]
    fn marker_skip_unless_truthy() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "yes".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_unless_falsy() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_unless_truthy() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "yes".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_unless_falsy() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# run unless MY_VAR
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    // ─── eval_marker: with expressions ─────────────────────────

    #[test]
    fn marker_skip_if_env_var() {
        let mut env = HashMap::new();
        env.insert("CI".into(), "true".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if CI
test "ci" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_missing_env() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if CI
test "ci" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_if_pure_fn_call() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"pure fn always_true() {
  "yes"
}

# skip if always_true()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_if_pure_fn_call_returns_empty() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"pure fn always_empty() {
  ""
}

# run if always_empty()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    // ─── eval_marker: errors ───────────────────────────────────

    #[test]
    fn marker_invalid_regex_in_condition() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "test".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if MY_VAR ? [invalid
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_invalid(&suite.plans[0]));
    }

    #[test]
    fn marker_undefined_fn_in_condition() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if nonexistent()
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_invalid(&suite.plans[0]));
    }

    // ─── eval_marker: multiple markers ─────────────────────────

    #[test]
    fn marker_first_skip_triggers() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
# run
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_second_skip_triggers() {
        let mut env = HashMap::new();
        env.insert("CI".into(), "true".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if UNSET
# skip if CI
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_none_trigger() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip if UNSET_A
# skip if UNSET_B
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_flaky_ignored() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# flaky
test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    // ─── Marker on fn/effect ───────────────────────────────────

    #[test]
    fn marker_skip_on_fn_propagates_to_test() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
fn helper() {
  > echo hello
}

test "t" {
  shell sh {
    helper()
  }
}
"#,
        )]);
        // fn is skipped → calling it from test body propagates skip
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_skip_on_effect_propagates_to_test() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
effect Setup -> sh {
  shell sh {
    > echo setup
  }
}

test "t" {
  need Setup
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn marker_run_met_on_fn_allows_test() {
        let mut env = HashMap::new();
        env.insert("CI".into(), "true".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# run if CI
fn helper() {
  > echo hello
}

test "t" {
  shell sh {
    helper()
  }
}
"#,
            )],
            env,
        );
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn marker_run_unmet_on_fn_propagates_skip_to_test() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# run if CI
fn helper() {
  > echo hello
}

test "t" {
  shell sh {
    helper()
  }
}
"#,
        )]);
        // CI not set → run condition unmet → fn skipped → test skipped
        assert!(is_skipped(&suite.plans[0]));
    }
}
