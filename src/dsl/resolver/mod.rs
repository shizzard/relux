pub mod ir;

use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};


use daggy::Walker;
use crate::dsl::discover_relux_files;

use crate::Spanned as AstSpanned;
use crate::config;
use crate::dsl::parser;
use ir::{FileId, SourceMap, Span};

// ─── Source Loader ──────────────────────────────────────────

pub trait SourceLoader {
    fn load(&self, mod_path: &str) -> Option<(PathBuf, String)>;
}

pub struct FsSourceLoader {
    search_paths: Vec<PathBuf>,
}

impl FsSourceLoader {
    pub fn new(project_root: PathBuf, extra_search_paths: Vec<PathBuf>) -> Self {
        let mut search_paths = vec![project_root];
        search_paths.extend(extra_search_paths);
        Self { search_paths }
    }
}

impl SourceLoader for FsSourceLoader {
    fn load(&self, mod_path: &str) -> Option<(PathBuf, String)> {
        for base in &self.search_paths {
            let file_path = base.join(mod_path).with_extension("relux");
            if let Ok(source) = std::fs::read_to_string(&file_path) {
                return Some((file_path, source));
            }
        }
        None
    }
}

// ─── Diagnostic ─────────────────────────────────────────────

#[derive(Debug)]
pub enum Diagnostic {
    Parse {
        file: FileId,
        error: parser::ParseError,
    },
    ModuleNotFound {
        path: String,
        referenced_from: Span,
    },
    RootNotFound {
        path: String,
    },
    CircularImport {
        cycle: Vec<(String, Option<Span>)>,
    },
    UndefinedName {
        name: String,
        span: Span,
        available_arities: Vec<usize>,
    },
    DuplicateDefinition {
        name: String,
        arity: Option<usize>,
        first: Span,
        second: Span,
    },
    UndefinedVariable {
        name: String,
        span: Span,
    },
    CircularEffectDependency {
        cycle: Vec<(String, Span)>,
    },
    InvalidTimeout {
        raw: String,
        span: Span,
    },
    ImportNotExported {
        name: String,
        module_path: String,
        span: Span,
    },
    InvalidRegex {
        pattern: String,
        message: String,
        span: Span,
    },
    ImpureInPureContext {
        what: String,
        span: Span,
    },
}

// ─── Name Resolution Types ──────────────────────────────────

/// A definition paired with the file it originates from.
#[derive(Debug, Clone)]
struct Located<T> {
    file: FileId,
    def: T,
}

/// A keyed lookup table built during resolution.
#[derive(Debug, Clone)]
struct LookupTable<K, V>(HashMap<K, V>);

impl<K, V> std::ops::Deref for LookupTable<K, V> {
    type Target = HashMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> std::ops::DerefMut for LookupTable<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V> LookupTable<K, V> {
    fn new() -> Self {
        LookupTable(HashMap::new())
    }
}

/// Function identity: name + arity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FnKey {
    name: String,
    arity: usize,
}

/// A module path like "imports/scoped".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModulePath(String);

impl std::ops::Deref for ModulePath {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ModulePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModulePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ModulePath {
    fn from(s: String) -> Self {
        ModulePath(s)
    }
}

impl From<&str> for ModulePath {
    fn from(s: &str) -> Self {
        ModulePath(s.to_string())
    }
}

impl Borrow<str> for ModulePath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// An effect name like "StartDb" — CamelCase by convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EffectName(String);

impl std::ops::Deref for EffectName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for EffectName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EffectName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for EffectName {
    fn from(s: String) -> Self {
        EffectName(s)
    }
}

impl From<&str> for EffectName {
    fn from(s: &str) -> Self {
        EffectName(s.to_string())
    }
}

impl Borrow<str> for EffectName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

// ─── Table Aliases ──────────────────────────────────────────

type FnTable = LookupTable<FnKey, Located<parser::FnDef>>;
type PureFnTable = LookupTable<FnKey, Located<parser::PureFnDef>>;
type EffectTable = LookupTable<EffectName, Located<parser::EffectDef>>;
type AstTable = LookupTable<ModulePath, Located<parser::Module>>;

/// What a module exports: all its fn and effect definitions.
#[derive(Debug, Clone)]
struct ModuleExports {
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
}

/// The resolved scope for a single module: own definitions + imports.
#[derive(Debug)]
struct ModuleScope {
    functions: FnTable,
    pure_functions: PureFnTable,
    effects: EffectTable,
}

// ─── Loader ─────────────────────────────────────────────────

struct Loader<'a> {
    source_map: SourceMap,
    asts: AstTable,
    loading_stack: Vec<(ModulePath, Option<Span>)>,
    diagnostics: Vec<Diagnostic>,
    source_loader: &'a dyn SourceLoader,
}

impl<'a> Loader<'a> {
    fn new(source_loader: &'a dyn SourceLoader) -> Self {
        Self {
            source_map: SourceMap::new(),
            asts: LookupTable::new(),
            loading_stack: Vec::new(),
            diagnostics: Vec::new(),
            source_loader,
        }
    }

    fn load_module(&mut self, mod_path: &str, referenced_from: Option<Span>) {
        if self.asts.contains_key(mod_path) {
            return;
        }

        if let Some(pos) = self.loading_stack.iter().position(|(p, _)| p.as_ref() == mod_path) {
            let cycle: Vec<(String, Option<Span>)> = self.loading_stack[pos..]
                .iter()
                .map(|(p, s)| (p.to_string(), s.clone()))
                .chain(std::iter::once((mod_path.to_string(), referenced_from.clone())))
                .collect();
            self.diagnostics.push(Diagnostic::CircularImport { cycle });
            return;
        }

        let (file_path, source) = match self.source_loader.load(mod_path) {
            Some(pair) => pair,
            None => {
                let span = referenced_from
                    .expect("root module should have been validated before resolving");
                self.diagnostics.push(Diagnostic::ModuleNotFound {
                    path: mod_path.to_string(),
                    referenced_from: span,
                });
                return;
            }
        };

        let file_id = self.source_map.add(file_path, source.clone());
        let (module, errors) = parser::parse(&source);

        for error in errors {
            self.diagnostics.push(Diagnostic::Parse {
                file: file_id,
                error,
            });
        }

        let module = match module {
            Some(m) => m,
            None => return,
        };

        self.loading_stack.push((ModulePath::from(mod_path), referenced_from));

        let import_paths: Vec<_> = module
            .items
            .iter()
            .filter_map(|item| {
                if let parser::Item::Import(imp) = &item.node {
                    Some((
                        imp.path.node.clone(),
                        Span::new(file_id, imp.path.span.clone()),
                    ))
                } else {
                    None
                }
            })
            .collect();

        for (path, span) in &import_paths {
            self.load_module(path, Some(span.clone()));
        }

        self.loading_stack.pop();
        self.asts.insert(ModulePath::from(mod_path), Located { file: file_id, def: module });
    }
}

// ─── Scope Builder ──────────────────────────────────────────

fn build_module_exports(file_id: FileId, module: &parser::Module) -> ModuleExports {
    let mut functions: FnTable = LookupTable::new();
    let mut pure_functions: PureFnTable = LookupTable::new();
    let mut effects: EffectTable = LookupTable::new();

    for item in &module.items {
        match &item.node {
            parser::Item::Fn(f) => {
                let key = FnKey { name: f.name.node.clone(), arity: f.params.len() };
                functions.insert(key, Located { file: file_id, def: f.clone() });
            }
            parser::Item::PureFn(f) => {
                let key = FnKey { name: f.name.node.clone(), arity: f.params.len() };
                pure_functions.insert(key, Located { file: file_id, def: f.clone() });
            }
            parser::Item::Effect(e) => {
                effects.insert(EffectName::from(e.name.node.clone()), Located { file: file_id, def: e.clone() });
            }
            _ => {}
        }
    }

    ModuleExports { functions, pure_functions, effects }
}

fn build_module_scope(
    file_id: FileId,
    module: &parser::Module,
    all_asts: &AstTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> ModuleScope {
    let own_exports = build_module_exports(file_id, module);
    let mut scope = ModuleScope {
        functions: own_exports.functions.clone(),
        pure_functions: own_exports.pure_functions.clone(),
        effects: own_exports.effects.clone(),
    };

    for item in &module.items {
        let imp = match &item.node {
            parser::Item::Import(imp) => imp,
            _ => continue,
        };

        let target_path = &imp.path.node;
        let Located { file: target_file_id, def: target_module } = match all_asts.get(target_path.as_str()) {
            Some(located) => located,
            None => continue, // already reported as ModuleNotFound
        };

        let target_exports = build_module_exports(*target_file_id, target_module);

        match &imp.names {
            None => {
                // Wildcard import: bring everything in
                for (key, val) in target_exports.functions.iter() {
                    if let Some(existing) = scope.functions.get(key) {
                        diagnostics.push(Diagnostic::DuplicateDefinition {
                            name: key.name.clone(),
                            arity: Some(key.arity),
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.functions.insert(key.clone(), val.clone());
                    }
                }
                for (key, val) in target_exports.pure_functions.iter() {
                    if let Some(existing) = scope.pure_functions.get(key) {
                        diagnostics.push(Diagnostic::DuplicateDefinition {
                            name: key.name.clone(),
                            arity: Some(key.arity),
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.pure_functions.insert(key.clone(), val.clone());
                    }
                }
                for (name, val) in target_exports.effects.iter() {
                    if let Some(existing) = scope.effects.get(name) {
                        diagnostics.push(Diagnostic::DuplicateDefinition {
                            name: name.to_string(),
                            arity: None,
                            first: def_span(existing.file, &existing.def.name.span),
                            second: def_span(val.file, &val.def.name.span),
                        });
                    } else {
                        scope.effects.insert(name.clone(), val.clone());
                    }
                }
            }
            Some(names) => {
                for import_name in names {
                    let raw_name = &import_name.node.name.node;
                    let local_name = import_name
                        .node
                        .alias
                        .as_ref()
                        .map(|a| a.node.clone())
                        .unwrap_or_else(|| raw_name.clone());

                    let mut found = false;

                    // Try functions (all arities)
                    let fn_matches: Vec<_> = target_exports
                        .functions
                        .iter()
                        .filter(|(key, _)| key.name == *raw_name)
                        .collect();
                    for (key, val) in &fn_matches {
                        found = true;
                        let new_key = FnKey { name: local_name.clone(), arity: key.arity };
                        if let Some(existing) = scope.functions.get(&new_key) {
                            diagnostics.push(Diagnostic::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: Some(key.arity),
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope.functions.insert(new_key, (*val).clone());
                        }
                    }

                    // Try pure functions (all arities)
                    let pure_fn_matches: Vec<_> = target_exports
                        .pure_functions
                        .iter()
                        .filter(|(key, _)| key.name == *raw_name)
                        .collect();
                    for (key, val) in &pure_fn_matches {
                        found = true;
                        let new_key = FnKey { name: local_name.clone(), arity: key.arity };
                        if let Some(existing) = scope.pure_functions.get(&new_key) {
                            diagnostics.push(Diagnostic::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: Some(key.arity),
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope.pure_functions.insert(new_key, (*val).clone());
                        }
                    }

                    // Try effects
                    if let Some(val) = target_exports.effects.get(raw_name.as_str()) {
                        found = true;
                        if let Some(existing) = scope.effects.get(local_name.as_str()) {
                            diagnostics.push(Diagnostic::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: None,
                                first: def_span(existing.file, &existing.def.name.span),
                                second: def_span(val.file, &val.def.name.span),
                            });
                        } else {
                            scope.effects.insert(EffectName::from(local_name.clone()), val.clone());
                        }
                    }

                    if !found {
                        diagnostics.push(Diagnostic::ImportNotExported {
                            name: raw_name.clone(),
                            module_path: target_path.clone(),
                            span: Span::new(file_id, import_name.node.name.span.clone()),
                        });
                    }
                }
            }
        }
    }

    scope
}

fn def_span(file_id: FileId, name_span: &parser::Span) -> Span {
    Span::new(file_id, name_span.clone())
}

// ─── Effect Graph Builder ───────────────────────────────────

/// Identity key for deduplicating effect instances.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct EffectIdentity {
    name: EffectName,
    overlay_keys: Vec<(String, String)>,
}

fn overlay_identity(overlay: &[AstSpanned<parser::OverlayEntry>]) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = overlay
        .iter()
        .map(|e| (e.node.key.node.clone(), format!("{:?}", e.node.value.node)))
        .collect();
    entries.sort();
    entries
}

struct EffectGraphBuilder<'a> {
    scope: &'a ModuleScope,
    scopes_by_file: &'a HashMap<FileId, &'a ModuleScope>,
    dag: daggy::Dag<ir::EffectInstance, ir::EffectEdge>,
    identity_map: HashMap<EffectIdentity, daggy::NodeIndex>,
    effects: Vec<ir::Effect>,
    effect_id_map: HashMap<EffectName, ir::EffectId>,
    multiplier: f64,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> EffectGraphBuilder<'a> {
    fn new(scope: &'a ModuleScope, scopes_by_file: &'a HashMap<FileId, &'a ModuleScope>, multiplier: f64) -> Self {
        Self {
            scope,
            scopes_by_file,
            dag: daggy::Dag::new(),
            identity_map: HashMap::new(),
            effects: Vec::new(),
            effect_id_map: HashMap::new(),
            multiplier,
            diagnostics: Vec::new(),
        }
    }

    fn resolve_need(
        &mut self,
        need: &parser::NeedDecl,
        need_file_id: FileId,
        dependent_node: Option<daggy::NodeIndex>,
    ) -> Option<daggy::NodeIndex> {
        let effect_name = &need.effect.node;

        let located = match self.scope.effects.get(effect_name.as_str()) {
            Some(located) => located,
            None => {
                self.diagnostics.push(Diagnostic::UndefinedName {
                    name: effect_name.clone(),
                    span: Span::new(need_file_id, need.effect.span.clone()),
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
            let overlay = lower_overlay(&need.overlay, need_file_id, self.scope, &mut self.diagnostics);
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
                    if let parser::EffectItem::Need(n) = &item.node {
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
            let alias = need.alias.as_ref().map(|a| {
                ir::Spanned::new(a.node.clone(), Span::new(need_file_id, a.span.clone()))
            });

            let edge = ir::EffectEdge {
                alias,
                need_effect_span: Span::new(need_file_id, need.effect.span.clone()),
            };
            if self.dag.add_edge(node_idx, dep_node, edge).is_err() {
                let closing_span = Span::new(need_file_id, need.effect.span.clone());
                let closing_name = self.effect_name_at(dep_node);
                let cycle =
                    self.build_effect_cycle(dep_node, node_idx, &closing_name, closing_span);
                self.diagnostics.push(Diagnostic::CircularEffectDependency { cycle });
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
        def: &parser::EffectDef,
    ) -> ir::EffectId {
        if let Some(&id) = self.effect_id_map.get(name) {
            return id;
        }
        let id = self.effects.len();
        let effect_scope = self
            .scopes_by_file
            .get(&file_id)
            .copied()
            .unwrap_or(self.scope);
        let effect = lower_effect_def(file_id, def, effect_scope, self.multiplier, &mut self.diagnostics);
        self.effects.push(effect);
        self.effect_id_map.insert(name.clone(), id);
        id
    }
}

// ─── AST → IR Lowering ─────────────────────────────────────

fn sp(file_id: FileId, span: &parser::Span) -> Span {
    Span::new(file_id, span.clone())
}

fn lower_spanned<T>(file_id: FileId, node: T, span: &parser::Span) -> ir::Spanned<T> {
    ir::Spanned::new(node, sp(file_id, span))
}

fn parse_timeout(
    kind: parser::TimeoutKind,
    raw: &str,
    multiplier: f64,
    file_id: FileId,
    span: &parser::Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Timeout {
    match humantime::parse_duration(raw.trim()) {
        Ok(d) => match kind {
            parser::TimeoutKind::Tolerance => ir::Timeout::Tolerance {
                duration: d,
                multiplier,
            },
            parser::TimeoutKind::Assertion => ir::Timeout::Assertion(d),
        },
        Err(_) => {
            // Point at the duration string (after the `~`/`@` prefix)
            let content_span = (span.start + 1)..span.end;
            diagnostics.push(Diagnostic::InvalidTimeout {
                raw: raw.to_string(),
                span: Span::new(file_id, content_span),
            });
            match kind {
                parser::TimeoutKind::Tolerance => ir::Timeout::Tolerance {
                    duration: crate::config::DEFAULT_TIMEOUT,
                    multiplier,
                },
                parser::TimeoutKind::Assertion => {
                    ir::Timeout::Assertion(crate::config::DEFAULT_TIMEOUT)
                }
            }
        }
    }
}

fn lower_string_expr(
    file_id: FileId,
    ast: &parser::AstStringExpr,
    token_span: &parser::Span,
    prefix_len: usize,
) -> ir::StringExpr {
    let parts = ast
        .parts
        .iter()
        .map(|part| {
            let ir_part = match part {
                parser::AstStringPart::Literal(s) => ir::StringPart::Literal(s.clone()),
                parser::AstStringPart::Interp(s) => ir::StringPart::Interp(s.clone()),
                parser::AstStringPart::Escape(s) => ir::StringPart::Literal(s.clone()),
                parser::AstStringPart::EscapedDollar => ir::StringPart::EscapedDollar,
            };
            ir::Spanned::new(ir_part, Span::new(file_id, 0..0))
        })
        .collect();
    // Payload content starts after the operator prefix and leading space,
    // and excludes the trailing newline consumed by the lexer callback.
    let content_start = token_span.start + prefix_len + 1;
    let content_end = if token_span.end > content_start {
        token_span.end.saturating_sub(1)
    } else {
        content_start
    };
    ir::StringExpr {
        parts,
        span: Span::new(file_id, content_start..content_end),
    }
}

fn lower_expr(
    file_id: FileId,
    ast: &parser::AstExpr,
    expr_span: &parser::Span,
    scope: &ModuleScope,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Expr {
    match ast {
        parser::AstExpr::String(s) => {
            ir::Expr::String(lower_string_expr(file_id, s, expr_span, 0))
        }
        parser::AstExpr::Var(name) => ir::Expr::Var(name.clone()),
        parser::AstExpr::Call(call) => {
            let arity = call.args.len();
            let fn_key = FnKey { name: call.name.node.clone(), arity };
            if !scope.functions.contains_key(&fn_key)
                && !scope.pure_functions.contains_key(&fn_key)
                && !crate::runtime::bifs::is_known(&call.name.node, arity)
            {
                let mut available: Vec<usize> = scope
                    .functions
                    .keys()
                    .chain(scope.pure_functions.keys())
                    .filter(|k| k.name == call.name.node)
                    .map(|k| k.arity)
                    .collect();
                available.sort();
                available.dedup();
                diagnostics.push(Diagnostic::UndefinedName {
                    name: format!("{}/{}", call.name.node, arity),
                    span: sp(file_id, &call.name.span),
                    available_arities: available,
                });
            }
            let args = call
                .args
                .iter()
                .map(|a| {
                    lower_spanned(
                        file_id,
                        lower_expr(file_id, &a.node, &a.span, scope, multiplier, diagnostics),
                        &a.span,
                    )
                })
                .collect();
            ir::Expr::Call(ir::FnCall {
                name: lower_spanned(file_id, call.name.node.clone(), &call.name.span),
                args,
            })
        }
        parser::AstExpr::Send(s) => {
            ir::Expr::Send(lower_string_expr(file_id, s, expr_span, 1))
        }
        parser::AstExpr::SendRaw(s) => {
            ir::Expr::SendRaw(lower_string_expr(file_id, s, expr_span, 2))
        }
        parser::AstExpr::MatchRegex(s) => {
            ir::Expr::MatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: None,
            })
        }
        parser::AstExpr::MatchLiteral(s) => {
            ir::Expr::MatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: None,
            })
        }
        parser::AstExpr::TimedMatchRegex(kind, dur, s) => {
            ir::Expr::MatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(*kind, dur, multiplier, file_id, expr_span, diagnostics)),
            })
        }
        parser::AstExpr::TimedMatchLiteral(kind, dur, s) => {
            ir::Expr::MatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(*kind, dur, multiplier, file_id, expr_span, diagnostics)),
            })
        }
        parser::AstExpr::BufferReset => ir::Expr::BufferReset,
    }
}

fn lower_stmt(
    file_id: FileId,
    ast: &parser::Stmt,
    stmt_span: &parser::Span,
    scope: &ModuleScope,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ir::Spanned<ir::ShellStmt>> {
    let ir_stmt = match ast {
        parser::Stmt::Comment(_) => return None,
        parser::Stmt::Let(l) => {
            let value = l.value.as_ref().map(|v| {
                lower_spanned(
                    file_id,
                    lower_expr(file_id, &v.node, &v.span, scope, multiplier, diagnostics),
                    &v.span,
                )
            });
            ir::ShellStmt::Let(ir::VarDecl {
                name: lower_spanned(file_id, l.name.node.clone(), &l.name.span),
                value,
            })
        }
        parser::Stmt::Assign(a) => ir::ShellStmt::Assign(ir::VarAssign {
            name: lower_spanned(file_id, a.name.node.clone(), &a.name.span),
            value: lower_spanned(
                file_id,
                lower_expr(file_id, &a.value.node, &a.value.span, scope, multiplier, diagnostics),
                &a.value.span,
            ),
        }),
        parser::Stmt::Timeout(kind, raw) => {
            let t = parse_timeout(*kind, raw, multiplier, file_id, stmt_span, diagnostics);
            ir::ShellStmt::Timeout(t)
        }
        parser::Stmt::FailRegex(s) => {
            ir::ShellStmt::FailRegex(lower_string_expr(file_id, s, stmt_span, 2))
        }
        parser::Stmt::FailLiteral(s) => {
            ir::ShellStmt::FailLiteral(lower_string_expr(file_id, s, stmt_span, 2))
        }
        parser::Stmt::ClearFailPattern => ir::ShellStmt::ClearFailPattern,
        parser::Stmt::Expr(e) => {
            ir::ShellStmt::Expr(lower_expr(file_id, e, stmt_span, scope, multiplier, diagnostics))
        }
    };
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

fn lower_cleanup_stmt(
    file_id: FileId,
    ast: &parser::CleanupStmt,
    stmt_span: &parser::Span,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ir::Spanned<ir::CleanupStmt>> {
    let empty_scope = ModuleScope {
        functions: LookupTable::new(),
        pure_functions: LookupTable::new(),
        effects: LookupTable::new(),
    };
    let ir_stmt = match ast {
        parser::CleanupStmt::Comment(_) => return None,
        parser::CleanupStmt::Send(s) => {
            ir::CleanupStmt::Send(lower_string_expr(file_id, s, stmt_span, 1))
        }
        parser::CleanupStmt::SendRaw(s) => {
            ir::CleanupStmt::SendRaw(lower_string_expr(file_id, s, stmt_span, 2))
        }
        parser::CleanupStmt::Let(l) => {
            let value = l.value.as_ref().map(|v| {
                ir::Spanned::new(
                    lower_expr(file_id, &v.node, &v.span, &empty_scope, multiplier, diagnostics),
                    sp(file_id, &v.span),
                )
            });
            ir::CleanupStmt::Let(ir::VarDecl {
                name: lower_spanned(file_id, l.name.node.clone(), &l.name.span),
                value,
            })
        }
        parser::CleanupStmt::Assign(a) => ir::CleanupStmt::Assign(ir::VarAssign {
            name: lower_spanned(file_id, a.name.node.clone(), &a.name.span),
            value: ir::Spanned::new(
                lower_expr(file_id, &a.value.node, &a.value.span, &empty_scope, multiplier, diagnostics),
                sp(file_id, &a.value.span),
            ),
        }),
    };
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

// ─── Pure Function Lowering ─────────────────────────────────

fn lower_pure_expr(
    file_id: FileId,
    ast: &parser::PureAstExpr,
    expr_span: &parser::Span,
    scope: &ModuleScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::PureExpr {
    match ast {
        parser::PureAstExpr::String(s) => {
            ir::PureExpr::String(lower_string_expr(file_id, s, expr_span, 0))
        }
        parser::PureAstExpr::Var(name) => ir::PureExpr::Var(name.clone()),
        parser::PureAstExpr::Call(call) => {
            let arity = call.args.len();
            let fn_key = FnKey { name: call.name.node.clone(), arity };
            if !scope.pure_functions.contains_key(&fn_key)
                && !crate::runtime::bifs::is_pure_bif(&call.name.node, arity)
            {
                if scope.functions.contains_key(&fn_key)
                    || crate::runtime::bifs::is_impure_bif(&call.name.node, arity)
                {
                    diagnostics.push(Diagnostic::ImpureInPureContext {
                        what: format!("{}/{}", call.name.node, arity),
                        span: sp(file_id, &call.name.span),
                    });
                } else {
                    let available: Vec<usize> = scope
                        .pure_functions
                        .keys()
                        .filter(|k| k.name == call.name.node)
                        .map(|k| k.arity)
                        .collect();
                    diagnostics.push(Diagnostic::UndefinedName {
                        name: format!("{}/{}", call.name.node, arity),
                        span: sp(file_id, &call.name.span),
                        available_arities: available,
                    });
                }
            }
            let args = call
                .args
                .iter()
                .map(|a| {
                    lower_spanned(
                        file_id,
                        lower_pure_expr(file_id, &a.node, &a.span, scope, diagnostics),
                        &a.span,
                    )
                })
                .collect();
            ir::PureExpr::Call(ir::PureFnCall {
                name: lower_spanned(file_id, call.name.node.clone(), &call.name.span),
                args,
            })
        }
    }
}

fn lower_pure_stmt(
    file_id: FileId,
    ast: &parser::PureAstStmt,
    stmt_span: &parser::Span,
    scope: &ModuleScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ir::Spanned<ir::PureStmt>> {
    let ir_stmt = match ast {
        parser::PureAstStmt::Comment(_) => return None,
        parser::PureAstStmt::Let(l) => {
            let value = l.value.as_ref().map(|v| {
                lower_spanned(
                    file_id,
                    lower_pure_expr(file_id, &v.node, &v.span, scope, diagnostics),
                    &v.span,
                )
            });
            ir::PureStmt::Let(ir::PureVarDecl {
                name: lower_spanned(file_id, l.name.node.clone(), &l.name.span),
                value,
            })
        }
        parser::PureAstStmt::Assign(a) => ir::PureStmt::Assign(ir::PureVarAssign {
            name: lower_spanned(file_id, a.name.node.clone(), &a.name.span),
            value: lower_spanned(
                file_id,
                lower_pure_expr(file_id, &a.value.node, &a.value.span, scope, diagnostics),
                &a.value.span,
            ),
        }),
        parser::PureAstStmt::Expr(e) => {
            ir::PureStmt::Expr(lower_pure_expr(file_id, e, stmt_span, scope, diagnostics))
        }
        parser::PureAstStmt::ImpureViolation => {
            diagnostics.push(Diagnostic::ImpureInPureContext {
                what: "shell operator".to_string(),
                span: sp(file_id, stmt_span),
            });
            return None;
        }
    };
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

fn lower_shell_block(
    file_id: FileId,
    block: &parser::ShellBlock,
    block_span: &parser::Span,
    scope: &ModuleScope,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Spanned<ir::ShellBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_stmt(file_id, &s.node, &s.span, scope, multiplier, diagnostics))
        .collect();
    ir::Spanned::new(
        ir::ShellBlock {
            name: lower_spanned(file_id, block.name.node.clone(), &block.name.span),
            stmts,
        },
        sp(file_id, block_span),
    )
}

fn lower_cleanup_block(
    file_id: FileId,
    block: &parser::CleanupBlock,
    block_span: &parser::Span,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Spanned<ir::CleanupBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_cleanup_stmt(file_id, &s.node, &s.span, multiplier, diagnostics))
        .collect();
    ir::Spanned::new(ir::CleanupBlock { stmts }, sp(file_id, block_span))
}

fn lower_overlay(
    overlay: &[AstSpanned<parser::OverlayEntry>],
    file_id: FileId,
    scope: &ModuleScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ir::OverlayEntry> {
    overlay
        .iter()
        .map(|e| {
            let value_expr =
                lower_pure_expr(file_id, &e.node.value.node, &e.node.value.span, scope, diagnostics);
            ir::OverlayEntry {
                key: lower_spanned(file_id, e.node.key.node.clone(), &e.node.key.span),
                value: ir::Spanned::new(value_expr, sp(file_id, &e.node.value.span)),
            }
        })
        .collect()
}

fn lower_marker(
    file_id: FileId,
    m: &parser::MarkerDecl,
    marker_span: &parser::Span,
    scope: &ModuleScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Condition {
    let kind = match m.kind {
        parser::MarkerKind::Skip => ir::CondKind::Skip,
        parser::MarkerKind::Run => ir::CondKind::Run,
        parser::MarkerKind::Flaky => ir::CondKind::Flaky,
    };
    let cond = m.condition.as_ref().map(|c| {
        let modifier = match c.modifier {
            parser::CondModifier::If => ir::CondModifier::If,
            parser::CondModifier::Unless => ir::CondModifier::Unless,
        };
        let body = match &c.body {
            parser::AstMarkerCondBody::Bare(expr) => {
                ir::CondBody::Bare(lower_pure_expr(file_id, expr, marker_span, scope, diagnostics))
            }
            parser::AstMarkerCondBody::Eq(lhs, rhs) => ir::CondBody::Eq(
                lower_pure_expr(file_id, lhs, marker_span, scope, diagnostics),
                lower_pure_expr(file_id, rhs, marker_span, scope, diagnostics),
            ),
            parser::AstMarkerCondBody::Regex(lhs, pat_expr) => {
                let pat_ir = lower_string_expr(file_id, pat_expr, marker_span, 0);
                // Validate regex if no interpolations
                let has_interps = pat_ir.parts.iter().any(|p| {
                    matches!(p.node, ir::StringPart::Interp(_))
                });
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
                        diagnostics.push(Diagnostic::InvalidRegex {
                            pattern: literal,
                            message: format!("{e}"),
                            span: sp(file_id, marker_span),
                        });
                    }
                }
                ir::CondBody::Regex(
                    lower_pure_expr(file_id, lhs, marker_span, scope, diagnostics),
                    pat_ir,
                )
            }
        };
        ir::CondExpr { modifier, body }
    });
    ir::Condition { kind, cond }
}

fn lower_effect_def(
    file_id: FileId,
    def: &parser::EffectDef,
    scope: &ModuleScope,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Effect {
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(file_id, &m.node, &m.span, scope, diagnostics),
                sp(file_id, &m.span),
            )
        })
        .collect();
    let mut vars = Vec::new();
    let mut shells = Vec::new();
    let mut cleanup = None;

    for item in &def.body {
        match &item.node {
            parser::EffectItem::Comment(_) => {}
            parser::EffectItem::Need(_) => {} // handled by graph builder
            parser::EffectItem::Let(l) => {
                let value = l.value.as_ref().map(|v| {
                    lower_spanned(
                        file_id,
                        lower_pure_expr(file_id, &v.node, &v.span, scope, diagnostics),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::PureVarDecl {
                        name: lower_spanned(file_id, l.name.node.clone(), &l.name.span),
                        value,
                    },
                    sp(file_id, &item.span),
                ));
            }
            parser::EffectItem::Shell(block) => {
                shells.push(lower_shell_block(
                    file_id,
                    block,
                    &item.span,
                    scope,
                    multiplier,
                    diagnostics,
                ));
            }
            parser::EffectItem::Cleanup(block) => {
                cleanup = Some(lower_cleanup_block(file_id, block, &item.span, multiplier, diagnostics));
            }
        }
    }

    ir::Effect {
        name: lower_spanned(file_id, def.name.node.clone(), &def.name.span),
        exported_shell: lower_spanned(
            file_id,
            def.exported_shell.node.clone(),
            &def.exported_shell.span,
        ),
        conditions,
        vars,
        shells,
        cleanup,
        span: sp(file_id, &def.name.span),
    }
}

fn lower_test_def(
    file_id: FileId,
    def: &parser::TestDef,
    test_span: &parser::Span,
    scope: &ModuleScope,
    needs: Vec<ir::Spanned<ir::TestNeed>>,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Test {
    let mut doc = None;
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(file_id, &m.node, &m.span, scope, diagnostics),
                sp(file_id, &m.span),
            )
        })
        .collect();
    let mut vars = Vec::new();
    let mut shells = Vec::new();
    let mut cleanup = None;

    for item in &def.body {
        match &item.node {
            parser::TestItem::Comment(_) => {}
            parser::TestItem::DocString(s) => {
                doc = Some(lower_spanned(file_id, s.clone(), &item.span));
            }
            parser::TestItem::Need(_) => {} // already resolved into `needs`
            parser::TestItem::Let(l) => {
                let value = l.value.as_ref().map(|v| {
                    lower_spanned(
                        file_id,
                        lower_pure_expr(file_id, &v.node, &v.span, scope, diagnostics),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::PureVarDecl {
                        name: lower_spanned(file_id, l.name.node.clone(), &l.name.span),
                        value,
                    },
                    sp(file_id, &item.span),
                ));
            }
            parser::TestItem::Shell(block) => {
                shells.push(lower_shell_block(
                    file_id,
                    block,
                    &item.span,
                    scope,
                    multiplier,
                    diagnostics,
                ));
            }
            parser::TestItem::Cleanup(block) => {
                cleanup = Some(lower_cleanup_block(file_id, block, &item.span, multiplier, diagnostics));
            }
        }
    }

    let timeout = def.timeout.as_ref().map(|t| {
        let (kind, ref raw) = t.node;
        let to = parse_timeout(kind, raw, multiplier, file_id, &t.span, diagnostics);
        ir::TestTimeout::Explicit(to)
    });

    ir::Test {
        name: lower_spanned(file_id, def.name.node.clone(), &def.name.span),
        timeout,
        doc,
        conditions,
        needs,
        vars,
        shells,
        cleanup,
        span: sp(file_id, test_span),
    }
}

// ─── Plan Builder ───────────────────────────────────────────

fn build_plan(
    file_id: FileId,
    test_def: &parser::TestDef,
    test_span: &parser::Span,
    scope: &ModuleScope,
    scopes_by_file: &HashMap<FileId, &ModuleScope>,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Plan {
    let mut graph_builder = EffectGraphBuilder::new(scope, scopes_by_file, multiplier);

    // Resolve test-level needs
    let mut ir_needs = Vec::new();
    let test_needs: Vec<_> = test_def
        .body
        .iter()
        .filter_map(|item| {
            if let parser::TestItem::Need(n) = &item.node {
                Some((n.clone(), item.span.clone()))
            } else {
                None
            }
        })
        .collect();

    for (need, need_span) in &test_needs {
        if let Some(node_idx) = graph_builder.resolve_need(need, file_id, None) {
            let alias = need.alias.as_ref().map(|a| {
                ir::Spanned::new(a.node.clone(), sp(file_id, &a.span))
            });

            ir_needs.push(ir::Spanned::new(
                ir::TestNeed {
                    instance: node_idx,
                    alias,
                },
                sp(file_id, need_span),
            ));
        }
    }

    diagnostics.extend(graph_builder.diagnostics);

    // Collect reachable functions (both impure and pure)
    let mut reachable_fns = Vec::new();
    let mut seen_fns: HashMap<FnKey, ir::FnId> = HashMap::new();
    let mut reachable_pure_fns = Vec::new();
    let mut seen_pure_fns: HashMap<FnKey, ir::PureFnId> = HashMap::new();
    collect_reachable_functions(
        test_def,
        scope,
        scopes_by_file,
        &mut reachable_fns,
        &mut seen_fns,
        &mut reachable_pure_fns,
        &mut seen_pure_fns,
        multiplier,
        diagnostics,
    );

    let test = lower_test_def(file_id, test_def, test_span, scope, ir_needs, multiplier, diagnostics);

    ir::Plan {
        functions: reachable_fns,
        pure_functions: reachable_pure_fns,
        effects: graph_builder.effects,
        effect_graph: ir::EffectGraph {
            dag: graph_builder.dag,
        },
        test,
    }
}

fn collect_reachable_functions(
    test_def: &parser::TestDef,
    scope: &ModuleScope,
    scopes_by_file: &HashMap<FileId, &ModuleScope>,
    functions: &mut Vec<ir::Function>,
    seen: &mut HashMap<FnKey, ir::FnId>,
    pure_functions: &mut Vec<ir::PureFunction>,
    seen_pure: &mut HashMap<FnKey, ir::PureFnId>,
    multiplier: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Walk test body to find all function calls
    let mut call_keys: Vec<FnKey> = Vec::new();

    // Walk test markers for pure function calls
    for marker in &test_def.markers {
        collect_pure_calls_from_marker(&marker.node, &mut call_keys);
    }

    for item in &test_def.body {
        match &item.node {
            parser::TestItem::Shell(block) => {
                collect_calls_from_stmts(&block.stmts, &mut call_keys);
            }
            parser::TestItem::Let(l) => {
                if let Some(v) = &l.value {
                    collect_pure_calls_from_pure_expr(&v.node, &mut call_keys);
                }
            }
            parser::TestItem::Need(need) => {
                for entry in &need.overlay {
                    collect_pure_calls_from_pure_expr(&entry.node.value.node, &mut call_keys);
                }
            }
            _ => {}
        }
    }

    // Also walk effect shells, lets, need overlays, and markers.
    // These calls originate from the effect's home module, so carry its FileId.
    let mut effect_call_keys: Vec<(FnKey, Option<FileId>)> = Vec::new();
    for (_, located) in scope.effects.iter() {
        let effect_file = located.file;
        let effect_def = &located.def;
        let mut keys = Vec::new();
        for marker in &effect_def.markers {
            collect_pure_calls_from_marker(&marker.node, &mut keys);
        }
        for item in &effect_def.body {
            match &item.node {
                parser::EffectItem::Shell(block) => {
                    collect_calls_from_stmts(&block.stmts, &mut keys);
                }
                parser::EffectItem::Let(l) => {
                    if let Some(v) = &l.value {
                        collect_pure_calls_from_pure_expr(&v.node, &mut keys);
                    }
                }
                parser::EffectItem::Need(need) => {
                    for entry in &need.overlay {
                        collect_pure_calls_from_pure_expr(&entry.node.value.node, &mut keys);
                    }
                }
                _ => {}
            }
        }
        effect_call_keys.extend(keys.into_iter().map(|k| (k, Some(effect_file))));
    }

    // Resolve each call — check impure functions first, then pure functions.
    // Each queue entry carries an optional FileId indicating the source module
    // whose scope should be used for resolution. None means use the test module's scope.
    // This ensures that when an imported function calls a sibling in its home module,
    // the sibling is resolved against the home module's scope, not the importer's.
    let mut queue: Vec<(FnKey, Option<FileId>)> =
        call_keys.into_iter().map(|k| (k, None)).collect();
    queue.extend(effect_call_keys);
    while let Some((key, source_file)) = queue.pop() {
        if seen.contains_key(&key) || seen_pure.contains_key(&key) {
            continue;
        }
        let resolve_scope = source_file
            .and_then(|fid| scopes_by_file.get(&fid).copied())
            .unwrap_or(scope);
        if let Some(located) = resolve_scope.functions.get(&key) {
            let fn_file_id = located.file;
            let fn_def = &located.def;
            seen.insert(key.clone(), functions.len());

            let mut child_keys = Vec::new();
            collect_calls_from_stmts(&fn_def.body, &mut child_keys);
            queue.extend(child_keys.into_iter().map(|k| (k, Some(fn_file_id))));

            let fn_scope = scopes_by_file.get(&fn_file_id).copied().unwrap_or(scope);
            let body = fn_def
                .body
                .iter()
                .filter_map(|s| lower_stmt(fn_file_id, &s.node, &s.span, fn_scope, multiplier, diagnostics))
                .collect();

            functions.push(ir::Function {
                name: lower_spanned(fn_file_id, key.name.clone(), &fn_def.name.span),
                params: fn_def
                    .params
                    .iter()
                    .map(|p| lower_spanned(fn_file_id, p.node.clone(), &p.span))
                    .collect(),
                body,
                span: sp(fn_file_id, &fn_def.name.span),
            });
        } else if let Some(located) = resolve_scope.pure_functions.get(&key) {
            let fn_file_id = located.file;
            let fn_def = &located.def;
            seen_pure.insert(key.clone(), pure_functions.len());

            let mut child_keys = Vec::new();
            collect_pure_calls_from_pure_stmts(&fn_def.body, &mut child_keys);
            queue.extend(child_keys.into_iter().map(|k| (k, Some(fn_file_id))));

            let fn_scope = scopes_by_file.get(&fn_file_id).copied().unwrap_or(scope);
            let body = fn_def
                .body
                .iter()
                .filter_map(|s| {
                    lower_pure_stmt(fn_file_id, &s.node, &s.span, fn_scope, diagnostics)
                })
                .collect();

            pure_functions.push(ir::PureFunction {
                name: lower_spanned(fn_file_id, key.name.clone(), &fn_def.name.span),
                params: fn_def
                    .params
                    .iter()
                    .map(|p| lower_spanned(fn_file_id, p.node.clone(), &p.span))
                    .collect(),
                body,
                span: sp(fn_file_id, &fn_def.name.span),
            });
        }
    }
}

fn collect_calls_from_stmts(stmts: &[AstSpanned<parser::Stmt>], keys: &mut Vec<FnKey>) {
    for stmt in stmts {
        match &stmt.node {
            parser::Stmt::Expr(e) => {
                collect_calls_from_expr(e, keys);
            }
            parser::Stmt::Let(l) => {
                if let Some(v) = &l.value {
                    collect_calls_from_expr(&v.node, keys);
                }
            }
            parser::Stmt::Assign(a) => {
                collect_calls_from_expr(&a.value.node, keys);
            }
            _ => {}
        }
    }
}

fn collect_calls_from_expr(expr: &parser::AstExpr, keys: &mut Vec<FnKey>) {
    match expr {
        parser::AstExpr::Call(call) => {
            keys.push(FnKey { name: call.name.node.clone(), arity: call.args.len() });
            for arg in &call.args {
                collect_calls_from_expr(&arg.node, keys);
            }
        }
        _ => {}
    }
}

// ─── Pure Function Collection ───────────────────────────────

fn collect_pure_calls_from_marker(marker: &parser::MarkerDecl, keys: &mut Vec<FnKey>) {
    if let Some(cond) = &marker.condition {
        match &cond.body {
            parser::AstMarkerCondBody::Bare(expr) => {
                collect_pure_calls_from_pure_expr(expr, keys);
            }
            parser::AstMarkerCondBody::Eq(lhs, rhs) => {
                collect_pure_calls_from_pure_expr(lhs, keys);
                collect_pure_calls_from_pure_expr(rhs, keys);
            }
            parser::AstMarkerCondBody::Regex(lhs, _) => {
                collect_pure_calls_from_pure_expr(lhs, keys);
            }
        }
    }
}

fn collect_pure_calls_from_pure_expr(expr: &parser::PureAstExpr, keys: &mut Vec<FnKey>) {
    match expr {
        parser::PureAstExpr::Call(call) => {
            keys.push(FnKey { name: call.name.node.clone(), arity: call.args.len() });
            for arg in &call.args {
                collect_pure_calls_from_pure_expr(&arg.node, keys);
            }
        }
        _ => {}
    }
}

fn collect_pure_calls_from_pure_stmts(
    stmts: &[AstSpanned<parser::PureAstStmt>],
    keys: &mut Vec<FnKey>,
) {
    for stmt in stmts {
        match &stmt.node {
            parser::PureAstStmt::Let(l) => {
                if let Some(v) = &l.value {
                    collect_pure_calls_from_pure_expr(&v.node, keys);
                }
            }
            parser::PureAstStmt::Assign(a) => {
                collect_pure_calls_from_pure_expr(&a.value.node, keys);
            }
            parser::PureAstStmt::Expr(e) => {
                collect_pure_calls_from_pure_expr(e, keys);
            }
            parser::PureAstStmt::Comment(_) | parser::PureAstStmt::ImpureViolation => {}
        }
    }
}

// ─── File Discovery ─────────────────────────────────────────

fn resolve_paths(
    paths: &[PathBuf],
    project_root: &Path,
) -> (Vec<PathBuf>, Vec<Diagnostic>) {
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        if path.is_dir() {
            match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(project_root) => {
                    files.extend(discover_relux_files(&canonical));
                }
                Ok(_) => {
                    diagnostics.push(Diagnostic::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
                Err(_) => {
                    diagnostics.push(Diagnostic::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
            }
        } else if path.exists() {
            match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(project_root) => {
                    files.push(canonical);
                }
                Ok(_) => {
                    diagnostics.push(Diagnostic::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
                Err(_) => {
                    diagnostics.push(Diagnostic::RootNotFound {
                        path: path.display().to_string(),
                    });
                }
            }
        } else {
            diagnostics.push(Diagnostic::RootNotFound {
                path: path.display().to_string(),
            });
        }
    }
    files.sort();
    files.dedup();
    (files, diagnostics)
}

fn path_to_mod(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}

// ─── Public API ─────────────────────────────────────────────

pub fn resolve(
    project_root: &Path,
    paths: Option<&[PathBuf]>,
    multiplier: f64,
) -> (Vec<ir::Plan>, SourceMap, Vec<Diagnostic>) {
    let lib_dir = config::lib_dir(project_root);
    let tests_dir = config::tests_dir(project_root);

    let lib_files = discover_relux_files(&lib_dir);

    let (test_files, mut early_diagnostics) = match paths {
        Some(paths) => resolve_paths(paths, project_root),
        None => (discover_relux_files(&tests_dir), Vec::new()),
    };

    let mut all_files = lib_files;
    all_files.extend(test_files);

    let loader = FsSourceLoader::new(project_root.to_path_buf(), vec![lib_dir.to_path_buf()]);
    let mod_paths: Vec<String> = all_files
        .iter()
        .map(|p| path_to_mod(p, project_root))
        .collect();

    let (plans, mut source_map, mut diagnostics) = resolve_with(&mod_paths, &loader, multiplier);
    source_map.project_root = Some(project_root.to_path_buf());
    early_diagnostics.append(&mut diagnostics);

    // Filter out plans from lib-only modules
    let plans = plans
        .into_iter()
        .filter(|plan| {
            let source_path = &source_map.files[plan.test.span.file].path;
            !source_path.starts_with(&lib_dir)
        })
        .collect();

    (plans, source_map, early_diagnostics)
}

pub fn resolve_with(
    root_mod_paths: &[String],
    source_loader: &dyn SourceLoader,
    multiplier: f64,
) -> (Vec<ir::Plan>, SourceMap, Vec<Diagnostic>) {
    let mut loader = Loader::new(source_loader);

    // Phase 1: Load all modules
    for mod_path in root_mod_paths {
        loader.load_module(mod_path, None);
    }

    let mut diagnostics = loader.diagnostics;

    // Phase 2: Build scopes for all modules
    let mut scopes: LookupTable<ModulePath, ModuleScope> = LookupTable::new();
    for (mod_path, located) in loader.asts.iter() {
        let scope = build_module_scope(located.file, &located.def, &loader.asts, &mut diagnostics);
        scopes.insert(mod_path.clone(), scope);
    }

    // Build FileId → scope lookup for cross-module resolution
    let scopes_by_file: HashMap<FileId, &ModuleScope> = loader
        .asts
        .iter()
        .filter_map(|(path, located)| scopes.get(path).map(|s| (located.file, s)))
        .collect();

    // Phase 3: For each test in root modules, build a Plan
    let mut plans = Vec::new();
    for mod_path in root_mod_paths {
        let located = match loader.asts.get(mod_path.as_str()) {
            Some(located) => located,
            None => continue,
        };
        let file_id = &located.file;
        let module = &located.def;
        let scope = match scopes.get(mod_path.as_str()) {
            Some(s) => s,
            None => continue,
        };

        for item in &module.items {
            if let parser::Item::Test(test_def) = &item.node {
                let plan = build_plan(
                    *file_id,
                    test_def,
                    &item.span,
                    scope,
                    &scopes_by_file,
                    multiplier,
                    &mut diagnostics,
                );
                plans.push(plan);
            }
        }
    }

    (plans, loader.source_map, diagnostics)
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct InMemoryLoader {
        modules: HashMap<String, String>,
    }

    impl InMemoryLoader {
        fn new() -> Self {
            Self {
                modules: HashMap::new(),
            }
        }

        fn add(&mut self, mod_path: &str, source: &str) {
            self.modules
                .insert(mod_path.to_string(), source.to_string());
        }

        fn resolve_one(&self, root: &str) -> (Vec<ir::Plan>, SourceMap, Vec<Diagnostic>) {
            resolve_with(&[root.to_string()], self, 1.0)
        }
    }

    impl SourceLoader for InMemoryLoader {
        fn load(&self, mod_path: &str) -> Option<(PathBuf, String)> {
            let source = self.modules.get(mod_path)?;
            let path = PathBuf::from(format!("{mod_path}.relux"));
            Some((path, source.clone()))
        }
    }

    fn diag_names(diags: &[Diagnostic]) -> Vec<&str> {
        diags
            .iter()
            .map(|d| match d {
                Diagnostic::Parse { .. } => "Parse",
                Diagnostic::ModuleNotFound { .. } => "ModuleNotFound",
                Diagnostic::RootNotFound { .. } => "RootNotFound",
                Diagnostic::CircularImport { .. } => "CircularImport",
                Diagnostic::UndefinedName { .. } => "UndefinedName",
                Diagnostic::DuplicateDefinition { .. } => "DuplicateDefinition",
                Diagnostic::UndefinedVariable { .. } => "UndefinedVariable",
                Diagnostic::CircularEffectDependency { .. } => "CircularEffectDependency",
                Diagnostic::InvalidTimeout { .. } => "InvalidTimeout",
                Diagnostic::ImportNotExported { .. } => "ImportNotExported",
                Diagnostic::InvalidRegex { .. } => "InvalidRegex",
                Diagnostic::ImpureInPureContext { .. } => "ImpureInPureContext",
            })
            .collect()
    }

    // ── Module Loading ──

    #[test]
    fn test_load_single_module() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "fn foo() {\n  > echo hello\n}\n\ntest \"basic\" {\n  shell s {\n    foo()\n  }\n}\n",
        );
        let (plans, source_map, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(source_map.files.len(), 1);
    }

    #[test]
    fn test_load_with_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { helper }\n\ntest \"t\" {\n  shell s {\n    helper()\n  }\n}\n",
        );
        let (plans, source_map, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(source_map.files.len(), 2);
    }

    #[test]
    fn test_diamond_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/shared", "fn shared_fn() {\n  > echo shared\n}\n");
        loader.add(
            "lib/a",
            "import lib/shared { shared_fn }\nfn a_fn() {\n  shared_fn()\n}\n",
        );
        loader.add(
            "lib/b",
            "import lib/shared { shared_fn }\nfn b_fn() {\n  shared_fn()\n}\n",
        );
        loader.add("main", "import lib/a { a_fn }\nimport lib/b { b_fn }\n\ntest \"t\" {\n  shell s {\n    a_fn()\n    b_fn()\n  }\n}\n");
        let (plans, source_map, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(source_map.files.len(), 4);
    }

    #[test]
    fn test_imported_function_calls_non_imported_sibling() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "lib/m",
            "fn helper() {\n  > echo help\n}\n\nfn caller() {\n  helper()\n}\n",
        );
        loader.add(
            "main",
            "import lib/m { caller }\n\ntest \"t\" {\n  shell s {\n    caller()\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].functions.len(),
            2,
            "both caller and its sibling helper should be reachable"
        );
    }

    #[test]
    fn test_imported_function_transitive_sibling_calls() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "lib/m",
            "fn deep() {\n  > echo deep\n}\n\nfn mid() {\n  deep()\n}\n\nfn top() {\n  mid()\n}\n",
        );
        loader.add(
            "main",
            "import lib/m { top }\n\ntest \"t\" {\n  shell s {\n    top()\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            plans[0].functions.len(),
            3,
            "top, mid, and deep should all be reachable"
        );
    }

    #[test]
    fn test_module_not_found() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "import lib/nonexistent { foo }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"ModuleNotFound"));
    }

    #[test]
    fn test_circular_import() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/a", "import lib/b\nfn a_fn() {\n  > echo a\n}\n");
        loader.add("lib/b", "import lib/a\nfn b_fn() {\n  > echo b\n}\n");
        loader.add(
            "main",
            "import lib/a { a_fn }\n\ntest \"t\" {\n  shell s {\n    a_fn()\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"CircularImport"));
    }

    // ── Name Resolution ──

    #[test]
    fn test_import_not_exported() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { nonexistent }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"ImportNotExported"));
        if let Diagnostic::ImportNotExported {
            name, module_path, ..
        } = &diags[0]
        {
            assert_eq!(name, "nonexistent");
            assert_eq!(module_path, "lib/utils");
        }
    }

    #[test]
    fn test_undefined_function_call() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    nonexistent_fn()\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"UndefinedName"));
    }

    #[test]
    fn test_function_arity_distinction() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "fn foo() {\n  > echo zero\n}\n\nfn foo(a) {\n  > echo one\n}\n\ntest \"t\" {\n  shell s {\n    foo()\n    foo(\"x\")\n  }\n}\n"
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].functions.len(),
            2,
            "foo/0 and foo/1 should both be resolved"
        );
    }

    #[test]
    fn test_wrong_arity_hints() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "fn foo(a) {\n  > echo one\n}\n\ntest \"t\" {\n  shell s {\n    foo(\"x\", \"y\")\n  }\n}\n"
        );
        let (_, _, diags) = loader.resolve_one("main");
        let undef = diags
            .iter()
            .find(|d| matches!(d, Diagnostic::UndefinedName { .. }));
        assert!(undef.is_some(), "expected UndefinedName diagnostic");
        if let Diagnostic::UndefinedName {
            available_arities, ..
        } = undef.unwrap()
        {
            assert_eq!(available_arities, &[1], "should hint that foo/1 exists");
        }
    }

    // ── Effect Graph ──

    #[test]
    fn test_effect_deduplication() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "effect StartDb -> db {\n  shell db {\n    > start db\n  }\n}\n\ntest \"t\" {\n  need StartDb as db1\n  need StartDb as db2\n  shell db1 {\n    > query 1\n  }\n}\n"
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].effect_graph.dag.node_count(), 1);
        assert_eq!(plans[0].test.needs.len(), 2);
        let inst0 = plans[0].test.needs[0].node.instance;
        let inst1 = plans[0].test.needs[1].node.instance;
        assert_eq!(
            inst0, inst1,
            "both needs should resolve to the same instance"
        );
    }

    #[test]
    fn test_need_without_alias_has_no_shell_binding() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "effect StartDb -> db {\n  shell db {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartDb\n  shell db {\n    > query\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert!(
            plans[0].test.needs[0].node.alias.is_none(),
            "bare need (no `as`) should have no alias"
        );
    }

    #[test]
    fn test_need_with_alias_has_shell_binding() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "effect StartDb -> db {\n  shell db {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartDb as mydb\n  shell mydb {\n    > query\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        let alias = plans[0].test.needs[0].node.alias.as_ref().expect("explicit alias should be Some");
        assert_eq!(alias.node, "mydb");
    }

    #[test]
    fn test_effect_different_overlay_different_instance() {
        let mut loader = InMemoryLoader::new();
        loader.add("main",
            "effect StartSvc -> svc {\n  shell svc {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartSvc as s1 {\n    PORT = \"8080\"\n  }\n  need StartSvc as s2 {\n    PORT = \"9090\"\n  }\n  shell s1 {\n    > query\n  }\n}\n"
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            plans[0].effect_graph.dag.node_count(),
            2,
            "different overlays → 2 instances"
        );
    }

    #[test]
    fn test_undefined_effect() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  need NonexistentEffect as x\n  shell x {\n    > echo\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"UndefinedName"));
    }

    // ── Lowering ──

    #[test]
    fn test_timeout_parsing() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~10s\n    > echo\n    ~500ms\n    > echo2\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => assert_eq!(*t, ir::Timeout::Tolerance { duration: Duration::from_secs(10), multiplier: 1.0 }),
            other => panic!("expected Timeout, got {other:?}"),
        }
        match &stmts[2].node {
            ir::ShellStmt::Timeout(t) => assert_eq!(*t, ir::Timeout::Tolerance { duration: Duration::from_millis(500), multiplier: 1.0 }),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~10xyz\n    > echo\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"InvalidTimeout"));
    }

    #[test]
    fn test_inline_test_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" ~5s {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            plans[0].test.timeout,
            Some(ir::TestTimeout::Explicit(ir::Timeout::Tolerance { duration: Duration::from_secs(5), multiplier: 1.0 }))
        );
    }

    #[test]
    fn test_no_inline_test_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(plans[0].test.timeout.is_none());
    }

    #[test]
    fn test_comments_stripped_from_ir() {
        let mut loader = InMemoryLoader::new();
        loader.add("main", "# top comment\ntest \"t\" {\n  # test comment\n  shell s {\n    # shell comment\n    > echo\n  }\n}\n");
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        for stmt in stmts {
            assert!(
                !matches!(&stmt.node, ir::ShellStmt::Expr(ir::Expr::Var(s)) if s.starts_with('#')),
                "comments should be stripped from IR"
            );
        }
    }

    // ── Error Spans ──

    #[test]
    fn test_error_span_points_to_correct_file() {
        let mut loader = InMemoryLoader::new();
        loader.add("lib/utils", "fn helper() {\n  > echo help\n}\n");
        loader.add(
            "main",
            "import lib/utils { nonexistent }\n\ntest \"t\" {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (_, source_map, diags) = loader.resolve_one("main");
        let import_err = diags
            .iter()
            .find(|d| matches!(d, Diagnostic::ImportNotExported { .. }));
        assert!(import_err.is_some());
        if let Diagnostic::ImportNotExported { span, .. } = import_err.unwrap() {
            let file = &source_map.files[span.file];
            assert!(
                file.path.to_string_lossy().contains("main"),
                "error should point to main module"
            );
            let text = &file.source[span.range.clone()];
            assert_eq!(text, "nonexistent", "span should cover the undefined name");
        }
    }

    #[test]
    fn test_undefined_name_span_accuracy() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    missing_fn(\"a\")\n  }\n}\n",
        );
        let (_, source_map, diags) = loader.resolve_one("main");
        let undef = diags
            .iter()
            .find(|d| matches!(d, Diagnostic::UndefinedName { .. }));
        assert!(undef.is_some());
        if let Diagnostic::UndefinedName { span, .. } = undef.unwrap() {
            let file = &source_map.files[span.file];
            let text = &file.source[span.range.clone()];
            assert_eq!(text, "missing_fn", "span should cover the function name");
        }
    }

    // ── Integration: syntax_demo-style ──

    #[test]
    fn test_multi_module_resolve() {
        let mut loader = InMemoryLoader::new();

        loader.add("lib/module1",
            "fn function1() {\n  > echo f1\n}\nfn function2() {\n  > echo f2\n}\nfn function3() {\n  > echo f3\n}\neffect Effect1 -> e1shell {\n  shell e1shell {\n    > start e1\n  }\n}\neffect Effect2 -> e2shell {\n  shell e2shell {\n    > start e2\n  }\n}\neffect Effect3 -> e3shell {\n  shell e3shell {\n    > start e3\n  }\n}\n"
        );
        loader.add("lib/module2", "fn mod2_fn() {\n  > echo mod2\n}\n");
        loader.add("main",
            "import lib/module1 {\n  function1, function2, function3 as f3,\n  Effect1, Effect2, Effect3 as E3,\n}\nimport lib/module2\n\nfn some_function(arg1, arg2) {\n  > echo ${arg1} ${arg2}\n}\n\nfn match_uuid() {\n  <? ([0-9a-f-]+)\n  ${1}\n}\n\neffect StartSomething -> something {\n  need Effect1 as e1\n  need Effect2 as e2\n  need E3 as e3 {\n    E3_VAR = \"value\"\n  }\n  let some_important_var\n  shell e3 {\n    > some command\n    <? match (\\d+)\n    some_important_var = ${1}\n  }\n  shell something {\n    some_function(\"a\", \"b\")\n  }\n  cleanup {\n    let flags = \"--graceful\"\n    > shutdown ${flags}\n  }\n}\n\ntest \"Some test\" {\n  \"\"\"\n  The test description\n  \"\"\"\n  need StartSomething as something_shell\n  need StartSomething as another_something_shell {\n    E3_VAR = \"another value\"\n  }\n  let global_test_var\n  shell myshell {\n    ~10s\n    !? [Ee]rror|FATAL|panic\n    let variable = \"always-string\"\n    let global_test_var = \"new value\"\n    > echo ${variable}\n    <? always-string\n    ~120s\n    > ./long_running_command\n    <? completed\n    != error\n  }\n  shell something_shell {\n    let result = some_function(\"arg1\", \"arg2\")\n    let id = match_uuid()\n    > curl localhost:8080/resource/${id}\n    <? 200\n  }\n  cleanup {\n    > rm -f /tmp/test_artifacts\n  }\n}\n"
        );

        let (plans, source_map, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1, "one test → one plan");
        assert!(source_map.files.len() >= 3, "main + 2 imported modules");

        let plan = &plans[0];
        assert_eq!(plan.test.name.node, "Some test");
        assert!(plan.test.doc.is_some());
        assert_eq!(plan.test.needs.len(), 2);
        assert!(plan.test.shells.len() >= 2);
        assert!(plan.test.cleanup.is_some());

        assert!(
            plan.effect_graph.dag.node_count() >= 2,
            "at least 2 distinct effect instances (different overlays on StartSomething)"
        );
        assert!(!plan.effects.is_empty());
        assert!(
            !plan.functions.is_empty(),
            "some_function and match_uuid should be reachable"
        );
    }

    #[test]
    fn test_timed_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~2s? regex\n    <~500ms= literal\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) => {
                assert_eq!(m.timeout_override, Some(ir::Timeout::Tolerance { duration: Duration::from_secs(2), multiplier: 1.0 }));
            }
            other => panic!("expected MatchRegex with timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) => {
                assert_eq!(m.timeout_override, Some(ir::Timeout::Tolerance { duration: Duration::from_millis(500), multiplier: 1.0 }));
            }
            other => panic!("expected MatchLiteral with timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_timed_match_invalid_duration() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~0xyz? regex\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"InvalidTimeout"));
    }

    // ── Assertion timeouts ──

    #[test]
    fn test_assertion_timeout_in_shell() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    @5s\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => assert_eq!(*t, ir::Timeout::Assertion(Duration::from_secs(5))),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_assertion_timeout_on_test() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" @3s {\n  shell s {\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            plans[0].test.timeout,
            Some(ir::TestTimeout::Explicit(ir::Timeout::Assertion(Duration::from_secs(3))))
        );
    }

    #[test]
    fn test_assertion_timed_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <@2s? regex\n    <@500ms= literal\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::MatchRegex(m)) => {
                assert_eq!(m.timeout_override, Some(ir::Timeout::Assertion(Duration::from_secs(2))));
            }
            other => panic!("expected MatchRegex with assertion timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) => {
                assert_eq!(m.timeout_override, Some(ir::Timeout::Assertion(Duration::from_millis(500))));
            }
            other => panic!("expected MatchLiteral with assertion timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_multiplier_does_not_affect_assertion_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    @5s\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = resolve_with(&["main".to_string()], &loader, 3.0);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => {
                assert_eq!(*t, ir::Timeout::Assertion(Duration::from_secs(5)));
                assert_eq!(t.resolve(), Duration::from_secs(5));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_multiplier_scales_tolerance_timeout() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    ~5s\n    > echo\n  }\n}\n",
        );
        let (plans, _, diags) = resolve_with(&["main".to_string()], &loader, 2.0);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Timeout(t) => {
                assert_eq!(*t, ir::Timeout::Tolerance { duration: Duration::from_secs(5), multiplier: 2.0 });
                assert_eq!(t.resolve(), Duration::from_secs(10));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    // ── Condition markers ──

    #[test]
    fn test_marker_lowering_test() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "[skip unless \"${CI}\"]\n[run if \"${OS}\" = \"linux\"]\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let plan = &plans[0];
        assert_eq!(plan.test.conditions.len(), 2);
        assert!(matches!(plan.test.conditions[0].node.kind, ir::CondKind::Skip));
        let c0 = plan.test.conditions[0].node.cond.as_ref().unwrap();
        assert!(matches!(c0.modifier, ir::CondModifier::Unless));
        assert!(matches!(c0.body, ir::CondBody::Bare(_)));
        assert!(matches!(plan.test.conditions[1].node.kind, ir::CondKind::Run));
        let c1 = plan.test.conditions[1].node.cond.as_ref().unwrap();
        assert!(matches!(c1.modifier, ir::CondModifier::If));
        assert!(matches!(c1.body, ir::CondBody::Eq(_, _)));
    }

    #[test]
    fn test_marker_lowering_effect() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            concat!(
                "[skip unless \"${PLATFORM}\" ? ^linux]\n",
                "effect E -> s {\n",
                "  shell s {\n    > start\n  }\n",
                "}\n",
                "test \"t\" {\n  need E as e\n  shell e {\n    > hi\n  }\n}\n",
            ),
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let plan = &plans[0];
        let effect = &plan.effects[0];
        assert_eq!(effect.conditions.len(), 1);
        assert!(matches!(effect.conditions[0].node.kind, ir::CondKind::Skip));
        let c0 = effect.conditions[0].node.cond.as_ref().unwrap();
        assert!(matches!(c0.body, ir::CondBody::Regex(_, _)));
    }

    #[test]
    fn test_bare_marker_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "[skip]\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let plan = &plans[0];
        assert_eq!(plan.test.conditions.len(), 1);
        assert!(matches!(plan.test.conditions[0].node.kind, ir::CondKind::Skip));
        assert!(plan.test.conditions[0].node.cond.is_none());
    }

    #[test]
    fn test_marker_invalid_regex() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "[skip unless \"${FOO}\" ? *invalid]\ntest \"t\" {\n  shell s {\n    > hi\n  }\n}\n",
        );
        let (_, _, diags) = loader.resolve_one("main");
        assert!(diag_names(&diags).contains(&"InvalidRegex"));
    }
}
