use std::collections::HashMap;

use crate::Spanned as AstSpanned;
use daggy::Walker;

use super::lower::{lower_effect_def, lower_overlay};
use super::*;

/// Identity key for deduplicating effect instances.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct EffectIdentity {
    name: EffectName,
    overlay_keys: Vec<(String, String)>,
}

fn overlay_identity(overlay: &[AstSpanned<parser::AstOverlayEntry>]) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = overlay
        .iter()
        .map(|e| (e.node.key.node.clone(), e.node.value.node.canonical()))
        .collect();
    entries.sort();
    entries
}

pub(super) struct EffectGraphBuilder<'a> {
    scopes_by_file: &'a ir::IndexVec<FileId, Option<&'a ModuleScope>>,
    pub(super) dag: daggy::Dag<ir::EffectInstance, ir::EffectEdge>,
    identity_map: HashMap<EffectIdentity, daggy::NodeIndex>,
    pub(super) effects: ir::IndexVec<ir::EffectId, ir::Effect>,
    effect_id_map: HashMap<EffectName, ir::EffectId>,
    multiplier: f64,
    pub(super) diagnostics: Vec<DiagnosticError>,
}

impl<'a> EffectGraphBuilder<'a> {
    pub(super) fn new(
        scopes_by_file: &'a ir::IndexVec<FileId, Option<&'a ModuleScope>>,
        multiplier: f64,
    ) -> Self {
        Self {
            scopes_by_file,
            dag: daggy::Dag::new(),
            identity_map: HashMap::new(),
            effects: ir::IndexVec::new(),
            effect_id_map: HashMap::new(),
            multiplier,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn resolve_need(
        &mut self,
        need: &parser::AstNeedDecl,
        need_file_id: FileId,
        dependent_node: Option<daggy::NodeIndex>,
    ) -> Option<daggy::NodeIndex> {
        let effect_name = &need.effect.node;
        let lookup_scope =
            self.scopes_by_file[need_file_id].expect("scope missing for file containing need");

        let located = match lookup_scope.effects.get(effect_name.as_str()) {
            Some(located) => located,
            None => {
                self.diagnostics.push(DiagnosticError::UndefinedName {
                    name: effect_name.clone(),
                    span: Span::new(need_file_id, need.effect.span.into()),
                    available_arities: vec![],
                });
                return None;
            }
        };
        let effect_file_id = located.file;

        let overlay_keys = overlay_identity(&need.overlay);
        let identity = EffectIdentity {
            name: EffectName::from(effect_name.clone()),
            overlay_keys,
        };

        let node_idx = if let Some(&existing) = self.identity_map.get(&identity) {
            existing
        } else {
            let effect_def = located.def.clone();
            let effect_id = self.ensure_effect_def(&identity.name, effect_file_id, &effect_def);
            let mut ctx = LoweringContext {
                file_id: need_file_id,
                scope: lookup_scope,
                multiplier: self.multiplier,
                errors: Vec::new(),
            };
            let overlay = lower_overlay(&mut ctx, &need.overlay);
            self.diagnostics.extend(ctx.errors);
            let instance = ir::EffectInstance {
                effect: effect_id,
                overlay,
            };
            let idx = self.dag.add_node(instance);
            self.identity_map.insert(identity, idx);

            // Recursively resolve this effect's own needs
            let sub_needs: Vec<_> = effect_def
                .body
                .iter()
                .filter_map(|item| {
                    if let parser::AstEffectItem::Need { decl: n, .. } = &item.node {
                        Some(n.clone())
                    } else {
                        None
                    }
                })
                .collect();

            for sub_need in &sub_needs {
                self.resolve_need(sub_need, effect_file_id, Some(idx));
            }

            idx
        };

        if let Some(dep_node) = dependent_node {
            let alias = need
                .alias
                .as_ref()
                .map(|a| ir::Spanned::new(a.node.clone(), Span::new(need_file_id, a.span.into())));

            let edge = ir::EffectEdge {
                alias,
                need_effect_span: Span::new(need_file_id, need.effect.span.into()),
            };
            if self.dag.add_edge(node_idx, dep_node, edge).is_err() {
                let closing_span = Span::new(need_file_id, need.effect.span.into());
                let closing_name = self.effect_name_at(dep_node);
                let cycle =
                    self.build_effect_cycle(dep_node, node_idx, &closing_name, closing_span);
                self.diagnostics
                    .push(DiagnosticError::CircularEffectDependency { cycle });
            }
        }

        Some(node_idx)
    }

    fn effect_name_at(&self, node: daggy::NodeIndex) -> String {
        let instance = &self.dag[node];
        self.effects[instance.effect].name.node.clone()
    }

    fn build_effect_cycle(
        &self,
        from: daggy::NodeIndex,
        to: daggy::NodeIndex,
        closing_name: &str,
        closing_span: Span,
    ) -> Vec<(String, Span)> {
        // DFS from `from` to `to` through existing edges to find the cycle path
        let mut path = Vec::new();
        if self.dfs_cycle(from, to, &mut path) {
            // path contains (name, span) for each step from `from` to `to`
            // Add the closing edge: the `need` that would create the cycle
            path.push((closing_name.to_string(), closing_span));
            path
        } else {
            // Fallback: shouldn't happen, but at least report the closing edge
            vec![(closing_name.to_string(), closing_span)]
        }
    }

    fn dfs_cycle(
        &self,
        current: daggy::NodeIndex,
        target: daggy::NodeIndex,
        path: &mut Vec<(String, Span)>,
    ) -> bool {
        // Edge B→A means "A depends on B" — the edge was created when A
        // processed `need B`, so child_node is A (the dependent) and the
        // edge's need_effect_span lives in A's source.
        for child_edge in self.dag.children(current).iter(&self.dag) {
            let (edge_idx, child_node) = child_edge;
            let edge = &self.dag[edge_idx];
            let child_name = self.effect_name_at(child_node);
            let span = edge.need_effect_span.clone();
            path.push((child_name, span));
            if child_node == target {
                return true;
            }
            if self.dfs_cycle(child_node, target, path) {
                return true;
            }
            path.pop();
        }
        false
    }

    fn ensure_effect_def(
        &mut self,
        name: &EffectName,
        file_id: FileId,
        def: &parser::AstEffectDef,
    ) -> ir::EffectId {
        if let Some(&id) = self.effect_id_map.get(name) {
            return id;
        }
        let effect_scope = self.scopes_by_file[file_id]
            .expect("scope missing for file containing effect definition");
        let mut ctx = LoweringContext {
            file_id,
            scope: effect_scope,
            multiplier: self.multiplier,
            errors: Vec::new(),
        };
        let effect = lower_effect_def(&mut ctx, def);
        self.diagnostics.extend(ctx.errors);
        let id = self.effects.push(effect);
        self.effect_id_map.insert(name.clone(), id);
        id
    }
}
