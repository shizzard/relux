//! DAG enrichment for effect start-lists. (See RFC R015.)

use std::collections::HashMap;
use std::collections::HashSet;

use relux_core::diagnostics::CycleReport;
use relux_core::diagnostics::EffectCycleEntry;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;

use crate::IrNode;
use crate::IrPureExpr;
use crate::IrStringPart;
use crate::effect::IrEffectStart;

fn collect_qualified_refs(expr: &IrPureExpr, out: &mut Vec<(String, String, IrSpan)>) {
    match expr {
        IrPureExpr::QualifiedVar {
            qualifier,
            name,
            span,
        } => out.push((qualifier.clone(), name.clone(), span.clone())),
        IrPureExpr::String { value, .. } => {
            for part in value.parts() {
                match part {
                    IrStringPart::QualifiedVar {
                        qualifier,
                        name,
                        span,
                    } => out.push((qualifier.clone(), name.clone(), span.clone())),
                    IrStringPart::Literal { .. }
                    | IrStringPart::Var { .. }
                    | IrStringPart::CaptureRef { .. }
                    | IrStringPart::EscapedDollar { .. } => {}
                }
            }
        }
        IrPureExpr::Call { call, .. } => {
            for arg in call.args() {
                collect_qualified_refs(arg, out);
            }
        }
        IrPureExpr::Var { .. } | IrPureExpr::Capture { .. } => {}
    }
}

/// Validates every qualified overlay reference in `starts` against the
/// `exposed` map (alias -> exposed var names), fills each start's wait-set
/// (`deps`), and rejects reference cycles.
///
/// Contract on `exposed`: every aliased start in `starts` is expected to
/// have an entry, even if it exposes nothing (an empty set) -- this
/// mirrors the precedent in `effect.rs`, which always inserts an entry per
/// aliased start when building `dep_exposed_vars`. A missing entry is not
/// treated as a caller bug; it fails closed the same as a present-but-empty
/// set, surfacing as `variable_not_exposed` for any qualified ref against
/// that alias.
pub fn enrich_start_dag(
    starts: &mut [IrEffectStart],
    exposed: &HashMap<String, HashSet<String>>,
) -> Result<(), LoweringBail> {
    let alias_index: HashMap<&str, usize> = starts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.alias().map(|a| (a, i)))
        .collect();

    let mut all_deps: Vec<Vec<usize>> = Vec::with_capacity(starts.len());
    for start in starts.iter() {
        let mut refs = Vec::new();
        for entry in start.overlay() {
            collect_qualified_refs(entry.value(), &mut refs);
        }
        let mut deps: HashSet<usize> = HashSet::new();
        for (qualifier, var, span) in refs {
            let &idx = alias_index.get(qualifier.as_str()).ok_or_else(|| {
                LoweringBail::invalid(InvalidReport::unknown_qualifier(
                    qualifier.clone(),
                    span.clone(),
                ))
            })?;
            let exposes_var = exposed
                .get(qualifier.as_str())
                .is_some_and(|vars| vars.contains(&var));
            if !exposes_var {
                return Err(LoweringBail::invalid(InvalidReport::variable_not_exposed(
                    qualifier.clone(),
                    var.clone(),
                    span,
                )));
            }
            deps.insert(idx);
        }
        let mut deps: Vec<usize> = deps.into_iter().collect();
        deps.sort_unstable();
        all_deps.push(deps);
    }

    detect_cycle(starts, &all_deps)?;
    for (start, deps) in starts.iter_mut().zip(all_deps) {
        start.set_deps(deps);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

fn visit(
    u: usize,
    deps: &[Vec<usize>],
    color: &mut [Color],
    stack: &mut Vec<usize>,
    starts: &[IrEffectStart],
) -> Result<(), LoweringBail> {
    color[u] = Color::Gray;
    stack.push(u);
    for &v in &deps[u] {
        match color[v] {
            Color::Gray => {
                let pos = stack.iter().position(|&x| x == v).unwrap();
                let chain = stack[pos..]
                    .iter()
                    .map(|&i| EffectCycleEntry {
                        id: starts[i].effect().clone(),
                        start_span: starts[i].span().clone(),
                    })
                    .collect();
                return Err(LoweringBail::invalid(InvalidReport::cycle(
                    CycleReport::Effect { chain },
                )));
            }
            Color::White => visit(v, deps, color, stack, starts)?,
            Color::Black => {}
        }
    }
    stack.pop();
    color[u] = Color::Black;
    Ok(())
}

fn detect_cycle(starts: &[IrEffectStart], deps: &[Vec<usize>]) -> Result<(), LoweringBail> {
    let n = starts.len();
    let mut color = vec![Color::White; n];
    let mut stack: Vec<usize> = Vec::new();

    for u in 0..n {
        if color[u] == Color::White {
            visit(u, deps, &mut color, &mut stack, starts)?;
        }
    }
    Ok(())
}

// --- Tests -------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use relux_core::diagnostics::EffectId;
    use relux_core::diagnostics::EffectName;
    use relux_core::diagnostics::ModulePath;

    use crate::effect::IrOverlayEntry;
    use crate::ident::IrIdent;
    use crate::interpolation::IrInterpolation;

    /// Builds an `IrEffectStart` from an alias, effect name, and overlay
    /// entries shaped `(overlay_key, qualifier, var)` -- each produces an
    /// `IrPureExpr::QualifiedVar` overlay value.
    fn start(alias: &str, effect_name: &str, overlay: &[(&str, &str, &str)]) -> IrEffectStart {
        let entries = overlay
            .iter()
            .map(|(key, qualifier, var)| {
                IrOverlayEntry::new(
                    IrIdent::new(*key, IrSpan::synthetic()),
                    IrPureExpr::QualifiedVar {
                        qualifier: (*qualifier).to_string(),
                        name: (*var).to_string(),
                        span: IrSpan::synthetic(),
                    },
                    IrSpan::synthetic(),
                )
            })
            .collect();
        start_with_entries(alias, effect_name, entries)
    }

    /// Builds an `IrEffectStart` from an alias, effect name, and pre-built
    /// overlay entries -- used when the overlay value is not a bare
    /// `QualifiedVar` (e.g. a `String` with interpolated qualified refs).
    fn start_with_entries(
        alias: &str,
        effect_name: &str,
        entries: Vec<IrOverlayEntry>,
    ) -> IrEffectStart {
        IrEffectStart::new(
            EffectId {
                module: ModulePath("test".into()),
                name: EffectName(effect_name.into()),
            },
            entries,
            Some(alias.to_string()),
            IrSpan::synthetic(),
            IrSpan::synthetic(),
        )
    }

    /// Builds an overlay entry whose value is a pure `String` interpolation
    /// containing one or more `${Qualifier.var}` refs -- exercises the
    /// nested-interpolation path in `collect_qualified_refs`, distinct from
    /// the bare-`QualifiedVar` overlay entries `start()` produces.
    fn string_overlay_entry(key: &str, qualified_refs: &[(&str, &str)]) -> IrOverlayEntry {
        let mut parts = Vec::new();
        for (i, (qualifier, var)) in qualified_refs.iter().enumerate() {
            if i > 0 {
                parts.push(IrStringPart::Literal {
                    value: ":".to_string(),
                    span: IrSpan::synthetic(),
                });
            }
            parts.push(IrStringPart::QualifiedVar {
                qualifier: (*qualifier).to_string(),
                name: (*var).to_string(),
                span: IrSpan::synthetic(),
            });
        }
        IrOverlayEntry::new(
            IrIdent::new(key, IrSpan::synthetic()),
            IrPureExpr::String {
                value: IrInterpolation::new(parts, IrSpan::synthetic()),
                span: IrSpan::synthetic(),
            },
            IrSpan::synthetic(),
        )
    }

    fn exposed_map(entries: &[(&str, &[&str])]) -> HashMap<String, HashSet<String>> {
        entries
            .iter()
            .map(|(alias, vars)| {
                (
                    (*alias).to_string(),
                    vars.iter().map(|v| (*v).to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn records_edge_for_valid_sibling_ref() {
        let mut starts = vec![
            start("Db", "Db", &[]),
            start("Api", "Api", &[("DB_PORT", "Db", "port")]),
        ];
        let exposed = exposed_map(&[("Db", &["port"])]);

        enrich_start_dag(&mut starts, &exposed).unwrap();

        assert!(starts[0].deps().is_empty());
        assert_eq!(starts[1].deps(), &[0]);
    }

    #[test]
    fn forward_reference_is_legal() {
        let mut starts = vec![
            start("Api", "Api", &[("DB_PORT", "Db", "port")]),
            start("Db", "Db", &[]),
        ];
        let exposed = exposed_map(&[("Db", &["port"])]);

        enrich_start_dag(&mut starts, &exposed).unwrap();

        assert_eq!(starts[0].deps(), &[1]);
        assert!(starts[1].deps().is_empty());
    }

    #[test]
    fn unknown_qualifier_is_rejected() {
        let mut starts = vec![start("Api", "Api", &[("X", "Nope", "port")])];
        let exposed = exposed_map(&[]);

        let err = enrich_start_dag(&mut starts, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ));
    }

    #[test]
    fn non_exposed_variable_is_rejected() {
        let mut starts = vec![
            start("Db", "Db", &[]),
            start("Api", "Api", &[("X", "Db", "secret")]),
        ];
        let exposed = exposed_map(&[("Db", &["port"])]);

        let err = enrich_start_dag(&mut starts, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::VariableNotExposed { .. })
        ));
    }

    #[test]
    fn nested_interpolation_records_edge() {
        // RFC R015's headline case: a `${Db.host}:${Db.port}`-shaped overlay
        // value, not a bare `QualifiedVar` -- proves `collect_qualified_refs`
        // walks `IrStringPart`s inside a `String` interpolation.
        let mut starts = vec![
            start("Db", "Db", &[]),
            start_with_entries(
                "Api",
                "Api",
                vec![string_overlay_entry(
                    "DB_ADDR",
                    &[("Db", "host"), ("Db", "port")],
                )],
            ),
        ];
        let exposed = exposed_map(&[("Db", &["host", "port"])]);

        enrich_start_dag(&mut starts, &exposed).unwrap();

        assert!(starts[0].deps().is_empty());
        assert_eq!(starts[1].deps(), &[0]);
    }

    #[test]
    fn self_reference_is_a_rejected_cycle() {
        let mut starts = vec![start("A", "A", &[("X", "A", "out")])];
        let exposed = exposed_map(&[("A", &["out"])]);

        let err = enrich_start_dag(&mut starts, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::Cycle(CycleReport::Effect { .. }))
        ));
    }

    #[test]
    fn sibling_cycle_is_rejected() {
        let mut starts = vec![
            start("A", "A", &[("X", "B", "out")]),
            start("B", "B", &[("Y", "A", "out")]),
        ];
        let exposed = exposed_map(&[("A", &["out"]), ("B", &["out"])]);

        let err = enrich_start_dag(&mut starts, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::Cycle(CycleReport::Effect { .. }))
        ));
    }

    #[test]
    fn fan_out_records_two_independent_edges() {
        let mut starts = vec![
            start("Db", "Db", &[]),
            start("Api", "Api", &[("DB_PORT", "Db", "port")]),
            start("Worker", "Worker", &[("DB_PORT", "Db", "port")]),
        ];
        let exposed = exposed_map(&[("Db", &["port"])]);

        enrich_start_dag(&mut starts, &exposed).unwrap();

        assert_eq!(starts[1].deps(), &[0]);
        assert_eq!(starts[2].deps(), &[0]);
    }
}
