//! DAG enrichment for effect start-lists. (See RFC R015.)

use std::collections::HashMap;
use std::collections::HashSet;

use relux_core::diagnostics::CycleReport;
use relux_core::diagnostics::EffectCycleEntry;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;

use crate::IrEffectItem;
use crate::IrExpr;
use crate::IrInterpolation;
use crate::IrNode;
use crate::IrPureExpr;
use crate::IrShellStmt;
use crate::IrStringPart;
use crate::IrTestItem;
use crate::LoweringContext;
use crate::effect::IrEffectStart;

/// Collects every `${Alias.var}`-shaped ref out of an `IrInterpolation`'s
/// parts. Shared by the pure-expr walk below and the shell-statement walk
/// in `validate_shell_body_refs`.
fn collect_interp_refs(interp: &IrInterpolation, out: &mut Vec<(String, String, IrSpan)>) {
    for part in interp.parts() {
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

fn collect_pure_expr_refs(expr: &IrPureExpr, out: &mut Vec<(String, String, IrSpan)>) {
    match expr {
        IrPureExpr::QualifiedVar {
            qualifier,
            name,
            span,
        } => out.push((qualifier.clone(), name.clone(), span.clone())),
        IrPureExpr::String { value, .. } => collect_interp_refs(value, out),
        IrPureExpr::Call { call, .. } => {
            for arg in call.args() {
                collect_pure_expr_refs(arg, out);
            }
        }
        IrPureExpr::Var { .. } | IrPureExpr::Capture { .. } => {}
    }
}

/// Collects every `${Alias.var}`-shaped ref out of an `IrExpr` -- the shell
/// (impure) expression type, which (unlike `IrPureExpr`) allows a bare
/// `QualifiedVar` directly, not just nested inside a `String` interpolation
/// (e.g. `let x := Alias.var`).
fn collect_expr_refs(expr: &IrExpr, out: &mut Vec<(String, String, IrSpan)>) {
    match expr {
        IrExpr::QualifiedVar {
            qualifier,
            name,
            span,
        } => out.push((qualifier.clone(), name.clone(), span.clone())),
        IrExpr::String { value, .. } => collect_interp_refs(value, out),
        IrExpr::Call { call, .. } => {
            for arg in call.args() {
                collect_expr_refs(arg, out);
            }
        }
        IrExpr::Var { .. } | IrExpr::CaptureRef { .. } => {}
    }
}

/// Collects every `${Alias.var}`-shaped ref reachable from a single shell
/// statement. Matches `IrShellStmt` exhaustively (no wildcard arm) so a
/// future variant that carries an interpolation cannot silently skip
/// validation.
fn collect_shell_stmt_refs(stmt: &IrShellStmt, out: &mut Vec<(String, String, IrSpan)>) {
    match stmt {
        IrShellStmt::Let { stmt, .. } => {
            if let Some(value) = stmt.value() {
                collect_expr_refs(value, out);
            }
        }
        IrShellStmt::Assign { stmt, .. } => collect_expr_refs(stmt.value(), out),
        IrShellStmt::Expr { expr, .. } => collect_expr_refs(expr, out),
        IrShellStmt::Send { payload, .. } | IrShellStmt::SendRaw { payload, .. } => {
            collect_interp_refs(payload, out)
        }
        IrShellStmt::MatchRegex { pattern, .. } | IrShellStmt::MatchLiteral { pattern, .. } => {
            collect_interp_refs(pattern, out)
        }
        IrShellStmt::PureMatch { lhs, pattern, .. } => {
            collect_expr_refs(lhs, out);
            collect_interp_refs(pattern, out);
        }
        IrShellStmt::TimedMatchRegex { pattern, .. }
        | IrShellStmt::TimedMatchLiteral { pattern, .. } => collect_interp_refs(pattern, out),
        IrShellStmt::FailRegex { pattern, .. } | IrShellStmt::FailLiteral { pattern, .. } => {
            collect_interp_refs(pattern, out)
        }
        IrShellStmt::MultiMatch { patterns, .. } => {
            for pattern in patterns {
                collect_interp_refs(pattern.pattern(), out);
            }
        }
        // No interpolation reachable from these variants.
        IrShellStmt::Comment { .. }
        | IrShellStmt::Timeout { .. }
        | IrShellStmt::ClearFailPattern { .. }
        | IrShellStmt::BufferReset { .. } => {}
    }
}

/// Validates every `${Alias.var}` reference in one shell-statement body
/// (either a `shell { }` block's body or a `cleanup { }` block's body)
/// against `exposed`.
fn validate_body_refs(
    body: &[IrShellStmt],
    exposed: &HashMap<String, HashSet<String>>,
) -> Result<(), LoweringBail> {
    for stmt in body {
        let mut refs = Vec::new();
        collect_shell_stmt_refs(stmt, &mut refs);
        for (qualifier, var, span) in refs {
            let Some(vars) = exposed.get(qualifier.as_str()) else {
                return Err(LoweringBail::invalid(InvalidReport::unknown_qualifier(
                    qualifier, span,
                )));
            };
            if !vars.contains(&var) {
                return Err(LoweringBail::invalid(InvalidReport::variable_not_exposed(
                    qualifier, var, span,
                )));
            }
        }
    }
    Ok(())
}

/// Builds the `alias -> exposed surface` maps for a start-list: for every
/// aliased start whose effect resolved successfully, one entry per map
/// keyed by the alias, holding the set of shell names (first) and var
/// names (second) that effect exposes. Shared by effect lowering (which
/// needs both maps, for expose-reference validation) and test lowering
/// (which needs only the vars map, for `enrich_start_dag` and
/// `validate_test_shell_body_refs`) so the alias -> exposed-surface
/// construction is written once.
///
/// An unaliased start or one whose effect failed to resolve contributes no
/// entry to either map -- matching the precedent this replaces: only
/// aliased, successfully-resolved starts can be referenced by a qualified
/// `${Alias.var}` in the first place.
pub fn build_dep_exposed(
    starts: &[IrEffectStart],
    ctx: &LoweringContext,
) -> (
    HashMap<String, HashSet<String>>,
    HashMap<String, HashSet<String>>,
) {
    let mut shells_map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut vars_map: HashMap<String, HashSet<String>> = HashMap::new();
    for start in starts {
        if let Some(alias) = start.alias()
            && let Some(Ok(eff)) = ctx.effects().get(start.effect()).map(|r| r.as_ref())
        {
            let shells: HashSet<String> = eff
                .shell_exposes()
                .map(|e| e.exposed_name().to_string())
                .collect();
            let vars: HashSet<String> = eff
                .var_exposes()
                .map(|e| e.exposed_name().to_string())
                .collect();
            shells_map.insert(alias.to_string(), shells);
            vars_map.insert(alias.to_string(), vars);
        }
    }
    (shells_map, vars_map)
}

/// Vars-only convenience wrapper around `build_dep_exposed`, for callers
/// (test lowering) that have no use for the exposed-shells map.
pub fn build_dep_exposed_vars(
    starts: &[IrEffectStart],
    ctx: &LoweringContext,
) -> HashMap<String, HashSet<String>> {
    build_dep_exposed(starts, ctx).1
}

/// Validates every `${Alias.var}` reference in `body_items`'s shell and
/// cleanup blocks against `exposed` (alias -> exposed var names) -- the
/// same dependency map `enrich_start_dag` validates overlay refs against.
/// All of an effect's dependencies are in scope in both its shell bodies
/// and its cleanup body regardless of which block does the referencing (a
/// qualified `Alias.shell { }` block re-entering a dep's own shell can
/// still interpolate a *different* dep's exposed var, and the runtime
/// shares the same `VarScope` between a `shell { }` block and the effect's
/// `cleanup { }` block -- see `EffectManager` step 5c), so every
/// `IrEffectItem::Shell` and `IrEffectItem::Cleanup` body is walked
/// uniformly. Unlike overlay refs, these refs need no wait-set entry: by
/// the time any shell or cleanup block runs, every start in the effect's
/// start-list has already completed (shells and cleanup run after `starts`
/// enrichment, not interleaved with it).
pub fn validate_shell_body_refs(
    body_items: &[IrEffectItem],
    exposed: &HashMap<String, HashSet<String>>,
) -> Result<(), LoweringBail> {
    for item in body_items {
        let body: &[IrShellStmt] = match item {
            IrEffectItem::Shell { block, .. } => block.body(),
            IrEffectItem::Cleanup { block, .. } => block.body(),
            IrEffectItem::Comment { .. }
            | IrEffectItem::Expect { .. }
            | IrEffectItem::Start { .. }
            | IrEffectItem::Let { .. }
            | IrEffectItem::PureMatch { .. }
            | IrEffectItem::Expose { .. } => continue,
        };
        validate_body_refs(body, exposed)?;
    }
    Ok(())
}

/// Test-level counterpart of `validate_shell_body_refs`. `IrTestItem` is a
/// distinct enum from `IrEffectItem` (no `Start`/`Expect`/`Expose`
/// variants; a `DocString` variant instead), so the match arms differ, but
/// the underlying per-statement walk is the same `validate_body_refs`.
///
/// This exists because a started effect's exposed vars are injected into
/// the *test's own* shell/cleanup scope at runtime (mirroring the
/// dep-var injection into an effect's own scope that
/// `validate_shell_body_refs` guards) -- so a `${Alias.var}` in a test's
/// `shell { }` or `cleanup { }` body is subject to the same
/// unknown-qualifier / non-exposed-var footgun as inside an effect body.
pub fn validate_test_shell_body_refs(
    body_items: &[IrTestItem],
    exposed: &HashMap<String, HashSet<String>>,
) -> Result<(), LoweringBail> {
    for item in body_items {
        let body: &[IrShellStmt] = match item {
            IrTestItem::Shell { block, .. } => block.body(),
            IrTestItem::Cleanup { block, .. } => block.body(),
            IrTestItem::Comment { .. }
            | IrTestItem::DocString { .. }
            | IrTestItem::Start { .. }
            | IrTestItem::Let { .. }
            | IrTestItem::PureMatch { .. } => continue,
        };
        validate_body_refs(body, exposed)?;
    }
    Ok(())
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
            collect_pure_expr_refs(entry.value(), &mut refs);
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

/// Provenance of a start's overlay values: for each overlay entry whose
/// value references a sibling alias via a qualified ref, one
/// `(overlay_key, source_alias)` pair. An overlay value referencing
/// several aliases yields several pairs; an overlay value referencing the
/// same alias more than once (e.g. `${Db.host}:${Db.port}`) yields it only
/// once, deduplicated per overlay entry. Empty when the start has no
/// implicit deps. Used for structured-log provenance (viewer, R015 Task D2).
pub fn overlay_dep_sources(start: &IrEffectStart) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in start.overlay() {
        let mut refs = Vec::new();
        collect_pure_expr_refs(entry.value(), &mut refs);
        let mut seen = HashSet::new();
        for (qualifier, _var, _span) in refs {
            if seen.insert(qualifier.clone()) {
                out.push((entry.key().name().to_string(), qualifier));
            }
        }
    }
    out
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

    use crate::block::IrShellBlock;
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
    /// nested-interpolation path in `collect_pure_expr_refs`, distinct from
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
        // value, not a bare `QualifiedVar` -- proves `collect_pure_expr_refs`
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

    /// Builds a single-statement shell body item: `Alias.shell_name { >
    /// ${qualifier.var} }` -- one `IrEffectItem::Shell` whose only statement
    /// is a `send` interpolating one qualified ref.
    fn shell_body_with_send_ref(qualifier: &str, var: &str) -> Vec<IrEffectItem> {
        let payload = IrInterpolation::new(
            vec![IrStringPart::QualifiedVar {
                qualifier: qualifier.to_string(),
                name: var.to_string(),
                span: IrSpan::synthetic(),
            }],
            IrSpan::synthetic(),
        );
        let stmt = IrShellStmt::Send {
            payload,
            span: IrSpan::synthetic(),
        };
        let block = IrShellBlock::new(
            None,
            IrIdent::new("client", IrSpan::synthetic()),
            vec![stmt],
            IrSpan::synthetic(),
        );
        vec![IrEffectItem::Shell {
            block,
            span: IrSpan::synthetic(),
        }]
    }

    #[test]
    fn shell_body_valid_qualified_ref_is_accepted() {
        let body_items = shell_body_with_send_ref("Db", "port");
        let exposed = exposed_map(&[("Db", &["port"])]);

        validate_shell_body_refs(&body_items, &exposed).unwrap();
    }

    #[test]
    fn shell_body_unknown_qualifier_is_rejected() {
        let body_items = shell_body_with_send_ref("Nope", "port");
        let exposed = exposed_map(&[]);

        let err = validate_shell_body_refs(&body_items, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::UnknownQualifier { .. })
        ));
    }

    #[test]
    fn shell_body_non_exposed_variable_is_rejected() {
        let body_items = shell_body_with_send_ref("Db", "secret");
        let exposed = exposed_map(&[("Db", &["port"])]);

        let err = validate_shell_body_refs(&body_items, &exposed).unwrap_err();

        assert!(matches!(
            err.invalid_report(),
            Some(InvalidReport::VariableNotExposed { .. })
        ));
    }

    #[test]
    fn overlay_dep_sources_records_one_pair_per_qualified_overlay_entry() {
        let s = start("Api", "Api", &[("DB_PORT", "Db", "port")]);

        assert_eq!(
            overlay_dep_sources(&s),
            vec![("DB_PORT".to_string(), "Db".to_string())]
        );
    }

    #[test]
    fn overlay_dep_sources_is_empty_for_plain_value() {
        let entries = vec![IrOverlayEntry::new(
            IrIdent::new("PLAIN", IrSpan::synthetic()),
            IrPureExpr::String {
                value: IrInterpolation::new(
                    vec![IrStringPart::Literal {
                        value: "static".to_string(),
                        span: IrSpan::synthetic(),
                    }],
                    IrSpan::synthetic(),
                ),
                span: IrSpan::synthetic(),
            },
            IrSpan::synthetic(),
        )];
        let s = start_with_entries("Api", "Api", entries);

        assert!(overlay_dep_sources(&s).is_empty());
    }

    #[test]
    fn overlay_dep_sources_dedups_repeated_alias_in_one_entry_but_keeps_distinct_aliases() {
        let s = start_with_entries(
            "Api",
            "Api",
            vec![
                string_overlay_entry("DB_ADDR", &[("Db", "host"), ("Db", "port")]),
                string_overlay_entry("URL", &[("Db", "host"), ("Cache", "port")]),
            ],
        );

        assert_eq!(
            overlay_dep_sources(&s),
            vec![
                ("DB_ADDR".to_string(), "Db".to_string()),
                ("URL".to_string(), "Db".to_string()),
                ("URL".to_string(), "Cache".to_string()),
            ]
        );
    }
}
