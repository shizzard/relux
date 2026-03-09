pub mod ir;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Spanned as AstSpanned;
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
    CircularImport {
        cycle: Vec<String>,
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
        cycle: Vec<String>,
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
}

// ─── Name Resolution Types ──────────────────────────────────

/// Function identity: (name, arity).
type FnKey = (String, usize);

/// What a module exports: all its fn and effect definitions.
#[derive(Debug, Clone)]
struct ModuleExports {
    functions: HashMap<FnKey, (FileId, parser::FnDef)>,
    effects: HashMap<String, (FileId, parser::EffectDef)>,
}

/// The resolved scope for a single module: own definitions + imports.
#[derive(Debug)]
struct ModuleScope {
    functions: HashMap<FnKey, (FileId, parser::FnDef)>,
    effects: HashMap<String, (FileId, parser::EffectDef)>,
    imported_module_exports: HashMap<String, ModuleExports>,
}

// ─── Loader ─────────────────────────────────────────────────

struct Loader<'a> {
    source_map: SourceMap,
    asts: HashMap<String, (FileId, parser::Module)>,
    loading_stack: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    source_loader: &'a dyn SourceLoader,
}

impl<'a> Loader<'a> {
    fn new(source_loader: &'a dyn SourceLoader) -> Self {
        Self {
            source_map: SourceMap::new(),
            asts: HashMap::new(),
            loading_stack: Vec::new(),
            diagnostics: Vec::new(),
            source_loader,
        }
    }

    fn load_module(&mut self, mod_path: &str, referenced_from: Option<Span>) {
        if self.asts.contains_key(mod_path) {
            return;
        }

        if let Some(pos) = self.loading_stack.iter().position(|p| p == mod_path) {
            let mut cycle: Vec<String> = self.loading_stack[pos..].to_vec();
            cycle.push(mod_path.to_string());
            self.diagnostics.push(Diagnostic::CircularImport { cycle });
            return;
        }

        let (file_path, source) = match self.source_loader.load(mod_path) {
            Some(pair) => pair,
            None => {
                let span = referenced_from.unwrap_or_else(|| Span::new(0, 0..0));
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

        self.loading_stack.push(mod_path.to_string());

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
        self.asts.insert(mod_path.to_string(), (file_id, module));
    }
}

// ─── Scope Builder ──────────────────────────────────────────

fn build_module_exports(file_id: FileId, module: &parser::Module) -> ModuleExports {
    let mut functions = HashMap::new();
    let mut effects = HashMap::new();

    for item in &module.items {
        match &item.node {
            parser::Item::Fn(f) => {
                let key = (f.name.node.clone(), f.params.len());
                functions.insert(key, (file_id, f.clone()));
            }
            parser::Item::Effect(e) => {
                effects.insert(e.name.node.clone(), (file_id, e.clone()));
            }
            _ => {}
        }
    }

    ModuleExports { functions, effects }
}

fn build_module_scope(
    _mod_path: &str,
    file_id: FileId,
    module: &parser::Module,
    all_asts: &HashMap<String, (FileId, parser::Module)>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ModuleScope {
    let own_exports = build_module_exports(file_id, module);
    let mut scope = ModuleScope {
        functions: own_exports.functions.clone(),
        effects: own_exports.effects.clone(),
        imported_module_exports: HashMap::new(),
    };

    for item in &module.items {
        let imp = match &item.node {
            parser::Item::Import(imp) => imp,
            _ => continue,
        };

        let target_path = &imp.path.node;
        let (target_file_id, target_module) = match all_asts.get(target_path.as_str()) {
            Some(pair) => pair,
            None => continue, // already reported as ModuleNotFound
        };

        let target_exports = build_module_exports(*target_file_id, target_module);
        scope
            .imported_module_exports
            .insert(target_path.clone(), target_exports.clone());

        match &imp.names {
            None => {
                // Wildcard import: bring everything in
                for (key, val) in &target_exports.functions {
                    if let Some(existing) = scope.functions.get(key) {
                        diagnostics.push(Diagnostic::DuplicateDefinition {
                            name: key.0.clone(),
                            arity: Some(key.1),
                            first: fn_def_span(existing.0, &existing.1),
                            second: fn_def_span(val.0, &val.1),
                        });
                    } else {
                        scope.functions.insert(key.clone(), val.clone());
                    }
                }
                for (name, val) in &target_exports.effects {
                    if let Some(existing) = scope.effects.get(name) {
                        diagnostics.push(Diagnostic::DuplicateDefinition {
                            name: name.clone(),
                            arity: None,
                            first: effect_def_span(existing.0, &existing.1),
                            second: effect_def_span(val.0, &val.1),
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
                        .filter(|((n, _), _)| n == raw_name)
                        .collect();
                    for ((_, arity), val) in &fn_matches {
                        found = true;
                        let key = (local_name.clone(), *arity);
                        if let Some(existing) = scope.functions.get(&key) {
                            diagnostics.push(Diagnostic::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: Some(*arity),
                                first: fn_def_span(existing.0, &existing.1),
                                second: fn_def_span(val.0, &val.1),
                            });
                        } else {
                            scope.functions.insert(key, (*val).clone());
                        }
                    }

                    // Try effects
                    if let Some(val) = target_exports.effects.get(raw_name) {
                        found = true;
                        if let Some(existing) = scope.effects.get(&local_name) {
                            diagnostics.push(Diagnostic::DuplicateDefinition {
                                name: local_name.clone(),
                                arity: None,
                                first: effect_def_span(existing.0, &existing.1),
                                second: effect_def_span(val.0, &val.1),
                            });
                        } else {
                            scope.effects.insert(local_name.clone(), val.clone());
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

fn fn_def_span(file_id: FileId, f: &parser::FnDef) -> Span {
    Span::new(file_id, f.name.span.clone())
}

fn effect_def_span(file_id: FileId, e: &parser::EffectDef) -> Span {
    Span::new(file_id, e.name.span.clone())
}

// ─── Effect Graph Builder ───────────────────────────────────

/// Identity key for deduplicating effect instances.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct EffectIdentity {
    name: String,
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
    effect_id_map: HashMap<String, ir::EffectId>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> EffectGraphBuilder<'a> {
    fn new(scope: &'a ModuleScope, scopes_by_file: &'a HashMap<FileId, &'a ModuleScope>) -> Self {
        Self {
            scope,
            scopes_by_file,
            dag: daggy::Dag::new(),
            identity_map: HashMap::new(),
            effects: Vec::new(),
            effect_id_map: HashMap::new(),
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

        let (effect_file_id, effect_def) = match self.scope.effects.get(effect_name) {
            Some(pair) => pair.clone(),
            None => {
                self.diagnostics.push(Diagnostic::UndefinedName {
                    name: effect_name.clone(),
                    span: Span::new(need_file_id, need.effect.span.clone()),
                    available_arities: vec![],
                });
                return None;
            }
        };

        let overlay_keys = overlay_identity(&need.overlay);
        let identity = EffectIdentity {
            name: effect_name.clone(),
            overlay_keys,
        };

        let node_idx = if let Some(&existing) = self.identity_map.get(&identity) {
            existing
        } else {
            let effect_id = self.ensure_effect_def(effect_name, effect_file_id, &effect_def);
            let overlay = lower_overlay(&need.overlay, need_file_id);
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
            let alias_name = need
                .alias
                .as_ref()
                .map(|a| a.node.clone())
                .unwrap_or_else(|| {
                    self.scope
                        .effects
                        .get(effect_name)
                        .map(|(_, def)| def.exported_shell.node.clone())
                        .unwrap_or_else(|| effect_name.clone())
                });
            let alias_span = need
                .alias
                .as_ref()
                .map(|a| Span::new(need_file_id, a.span.clone()))
                .unwrap_or_else(|| {
                    self.scope
                        .effects
                        .get(effect_name)
                        .map(|(fid, def)| sp(*fid, &def.exported_shell.span))
                        .unwrap_or_else(|| Span::new(need_file_id, need.effect.span.clone()))
                });

            let edge = ir::EffectEdge {
                alias: ir::Spanned::new(alias_name, alias_span),
            };
            if self.dag.add_edge(node_idx, dep_node, edge).is_err() {
                self.diagnostics.push(Diagnostic::CircularEffectDependency {
                    cycle: vec![effect_name.clone()],
                });
            }
        }

        Some(node_idx)
    }

    fn ensure_effect_def(
        &mut self,
        name: &str,
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
        let effect = lower_effect_def(file_id, def, effect_scope, &mut self.diagnostics);
        self.effects.push(effect);
        self.effect_id_map.insert(name.to_string(), id);
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
    raw: &str,
    file_id: FileId,
    span: &parser::Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> Duration {
    match humantime::parse_duration(raw.trim()) {
        Ok(d) => d,
        Err(_) => {
            // Point at the duration string (after the `~` prefix)
            let content_span = (span.start + 1)..span.end;
            diagnostics.push(Diagnostic::InvalidTimeout {
                raw: raw.to_string(),
                span: Span::new(file_id, content_span),
            });
            crate::config::DEFAULT_TIMEOUT
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Expr {
    match ast {
        parser::AstExpr::String(s) => {
            ir::Expr::String(lower_string_expr(file_id, s, expr_span, 0))
        }
        parser::AstExpr::Var(name) => ir::Expr::Var(name.clone()),
        parser::AstExpr::Call(call) => {
            let arity = call.args.len();
            let fn_key = (call.name.node.clone(), arity);
            if !scope.functions.contains_key(&fn_key)
                && crate::runtime::bifs::lookup(&call.name.node, arity).is_none()
            {
                let available: Vec<usize> = scope
                    .functions
                    .keys()
                    .filter(|(n, _)| n == &call.name.node)
                    .map(|(_, a)| *a)
                    .collect();
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
                        lower_expr(file_id, &a.node, &a.span, scope, diagnostics),
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
        parser::AstExpr::NegMatchRegex(s) => {
            ir::Expr::NegMatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: None,
            })
        }
        parser::AstExpr::NegMatchLiteral(s) => {
            ir::Expr::NegMatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: None,
            })
        }
        parser::AstExpr::TimedMatchRegex(dur, s) => {
            ir::Expr::MatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(dur, file_id, expr_span, diagnostics)),
            })
        }
        parser::AstExpr::TimedMatchLiteral(dur, s) => {
            ir::Expr::MatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(dur, file_id, expr_span, diagnostics)),
            })
        }
        parser::AstExpr::TimedNegMatchRegex(dur, s) => {
            ir::Expr::NegMatchRegex(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(dur, file_id, expr_span, diagnostics)),
            })
        }
        parser::AstExpr::TimedNegMatchLiteral(dur, s) => {
            ir::Expr::NegMatchLiteral(ir::MatchExpr {
                pattern: lower_string_expr(file_id, s, expr_span, 2),
                timeout_override: Some(parse_timeout(dur, file_id, expr_span, diagnostics)),
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
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ir::Spanned<ir::ShellStmt>> {
    let ir_stmt = match ast {
        parser::Stmt::Comment(_) => return None,
        parser::Stmt::Let(l) => {
            let value = l.value.as_ref().map(|v| {
                lower_spanned(
                    file_id,
                    lower_expr(file_id, &v.node, &v.span, scope, diagnostics),
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
                lower_expr(file_id, &a.value.node, &a.value.span, scope, diagnostics),
                &a.value.span,
            ),
        }),
        parser::Stmt::Timeout(raw) => {
            let dur = parse_timeout(raw, file_id, stmt_span, diagnostics);
            ir::ShellStmt::Timeout(dur)
        }
        parser::Stmt::FailRegex(s) => {
            ir::ShellStmt::FailRegex(lower_string_expr(file_id, s, stmt_span, 2))
        }
        parser::Stmt::FailLiteral(s) => {
            ir::ShellStmt::FailLiteral(lower_string_expr(file_id, s, stmt_span, 2))
        }
        parser::Stmt::ClearFailPattern => ir::ShellStmt::ClearFailPattern,
        parser::Stmt::Expr(e) => {
            ir::ShellStmt::Expr(lower_expr(file_id, e, stmt_span, scope, diagnostics))
        }
    };
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

fn lower_cleanup_stmt(
    file_id: FileId,
    ast: &parser::CleanupStmt,
    stmt_span: &parser::Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ir::Spanned<ir::CleanupStmt>> {
    let empty_scope = ModuleScope {
        functions: HashMap::new(),
        effects: HashMap::new(),
        imported_module_exports: HashMap::new(),
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
                    lower_expr(file_id, &v.node, &v.span, &empty_scope, diagnostics),
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
                lower_expr(file_id, &a.value.node, &a.value.span, &empty_scope, diagnostics),
                sp(file_id, &a.value.span),
            ),
        }),
    };
    Some(ir::Spanned::new(ir_stmt, sp(file_id, stmt_span)))
}

fn lower_shell_block(
    file_id: FileId,
    block: &parser::ShellBlock,
    block_span: &parser::Span,
    scope: &ModuleScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Spanned<ir::ShellBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_stmt(file_id, &s.node, &s.span, scope, diagnostics))
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Spanned<ir::CleanupBlock> {
    let stmts = block
        .stmts
        .iter()
        .filter_map(|s| lower_cleanup_stmt(file_id, &s.node, &s.span, diagnostics))
        .collect();
    ir::Spanned::new(ir::CleanupBlock { stmts }, sp(file_id, block_span))
}

fn lower_overlay(
    overlay: &[AstSpanned<parser::OverlayEntry>],
    file_id: FileId,
) -> Vec<ir::OverlayEntry> {
    overlay
        .iter()
        .map(|e| {
            let value_expr = match &e.node.value.node {
                parser::AstExpr::String(s) => {
                    lower_string_expr(file_id, s, &e.node.value.span, 0)
                }
                _ => ir::StringExpr {
                    parts: vec![],
                    span: Span::new(file_id, e.node.value.span.clone()),
                },
            };
            ir::OverlayEntry {
                key: lower_spanned(file_id, e.node.key.node.clone(), &e.node.key.span),
                value: ir::Spanned::new(value_expr, sp(file_id, &e.node.value.span)),
            }
        })
        .collect()
}

fn lower_marker_expr(
    file_id: FileId,
    expr: &parser::AstMarkerExpr,
    span: &parser::Span,
) -> ir::StringExpr {
    match expr {
        parser::AstMarkerExpr::String(s) => lower_string_expr(file_id, s, span, 0),
        parser::AstMarkerExpr::Number(n) => ir::StringExpr {
            parts: vec![ir::Spanned::new(
                ir::StringPart::Literal(n.clone()),
                sp(file_id, span),
            )],
            span: sp(file_id, span),
        },
    }
}

fn lower_marker(
    file_id: FileId,
    m: &parser::MarkerDecl,
    marker_span: &parser::Span,
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
                ir::CondBody::Bare(lower_marker_expr(file_id, expr, marker_span))
            }
            parser::AstMarkerCondBody::Eq(lhs, rhs) => ir::CondBody::Eq(
                lower_marker_expr(file_id, lhs, marker_span),
                lower_marker_expr(file_id, rhs, marker_span),
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
                    lower_marker_expr(file_id, lhs, marker_span),
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Effect {
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(file_id, &m.node, &m.span, diagnostics),
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
                        lower_expr(file_id, &v.node, &v.span, scope, diagnostics),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::VarDecl {
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
                    diagnostics,
                ));
            }
            parser::EffectItem::Cleanup(block) => {
                cleanup = Some(lower_cleanup_block(file_id, block, &item.span, diagnostics));
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Test {
    let mut doc = None;
    let conditions = def
        .markers
        .iter()
        .map(|m| {
            ir::Spanned::new(
                lower_marker(file_id, &m.node, &m.span, diagnostics),
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
                        lower_expr(file_id, &v.node, &v.span, scope, diagnostics),
                        &v.span,
                    )
                });
                vars.push(ir::Spanned::new(
                    ir::VarDecl {
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
                    diagnostics,
                ));
            }
            parser::TestItem::Cleanup(block) => {
                cleanup = Some(lower_cleanup_block(file_id, block, &item.span, diagnostics));
            }
        }
    }

    ir::Test {
        name: lower_spanned(file_id, def.name.node.clone(), &def.name.span),
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ir::Plan {
    let mut graph_builder = EffectGraphBuilder::new(scope, scopes_by_file);

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
            let alias_name = need
                .alias
                .as_ref()
                .map(|a| a.node.clone())
                .unwrap_or_else(|| {
                    scope
                        .effects
                        .get(&need.effect.node)
                        .map(|(_, def)| def.exported_shell.node.clone())
                        .unwrap_or_else(|| need.effect.node.clone())
                });
            let alias_span = need
                .alias
                .as_ref()
                .map(|a| sp(file_id, &a.span))
                .unwrap_or_else(|| {
                    scope
                        .effects
                        .get(&need.effect.node)
                        .map(|(fid, def)| sp(*fid, &def.exported_shell.span))
                        .unwrap_or_else(|| sp(file_id, &need.effect.span))
                });

            ir_needs.push(ir::Spanned::new(
                ir::TestNeed {
                    instance: node_idx,
                    alias: ir::Spanned::new(alias_name, alias_span),
                },
                sp(file_id, need_span),
            ));
        }
    }

    diagnostics.extend(graph_builder.diagnostics);

    // Collect reachable functions
    let mut reachable_fns = Vec::new();
    let mut seen_fns: HashMap<FnKey, ir::FnId> = HashMap::new();
    collect_reachable_functions(
        test_def,
        scope,
        scopes_by_file,
        &mut reachable_fns,
        &mut seen_fns,
        diagnostics,
    );

    let test = lower_test_def(file_id, test_def, test_span, scope, ir_needs, diagnostics);

    ir::Plan {
        functions: reachable_fns,
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Walk test body to find all function calls
    let mut call_keys: Vec<FnKey> = Vec::new();
    for item in &test_def.body {
        if let parser::TestItem::Shell(block) = &item.node {
            collect_calls_from_stmts(&block.stmts, &mut call_keys);
        }
    }

    // Also walk effect shells
    for (_, (_, effect_def)) in &scope.effects {
        for item in &effect_def.body {
            if let parser::EffectItem::Shell(block) = &item.node {
                collect_calls_from_stmts(&block.stmts, &mut call_keys);
            }
        }
    }

    // Resolve each call, then recursively resolve calls within function bodies
    let mut queue = call_keys;
    while let Some(key) = queue.pop() {
        if seen.contains_key(&key) {
            continue;
        }
        if let Some((fn_file_id, fn_def)) = scope.functions.get(&key) {
            let fn_id = functions.len();
            seen.insert(key.clone(), fn_id);

            collect_calls_from_stmts(&fn_def.body, &mut queue);

            let fn_scope = scopes_by_file.get(fn_file_id).copied().unwrap_or(scope);
            let body = fn_def
                .body
                .iter()
                .filter_map(|s| lower_stmt(*fn_file_id, &s.node, &s.span, fn_scope, diagnostics))
                .collect();

            functions.push(ir::Function {
                name: lower_spanned(*fn_file_id, key.0.clone(), &fn_def.name.span),
                params: fn_def
                    .params
                    .iter()
                    .map(|p| lower_spanned(*fn_file_id, p.node.clone(), &p.span))
                    .collect(),
                body,
                span: sp(*fn_file_id, &fn_def.name.span),
            });
        }
    }
}

fn collect_calls_from_stmts(stmts: &[AstSpanned<parser::Stmt>], keys: &mut Vec<FnKey>) {
    for stmt in stmts {
        match &stmt.node {
            parser::Stmt::Expr(parser::AstExpr::Call(call)) => {
                keys.push((call.name.node.clone(), call.args.len()));
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
            keys.push((call.name.node.clone(), call.args.len()));
            for arg in &call.args {
                collect_calls_from_expr(&arg.node, keys);
            }
        }
        _ => {}
    }
}

// ─── Public API ─────────────────────────────────────────────

pub fn resolve(
    roots: &[PathBuf],
    project_root: &Path,
    lib_dir: &Path,
) -> (Vec<ir::Plan>, SourceMap, Vec<Diagnostic>) {
    let loader = FsSourceLoader::new(project_root.to_path_buf(), vec![lib_dir.to_path_buf()]);
    let mod_paths: Vec<String> = roots
        .iter()
        .map(|root_path| {
            root_path
                .strip_prefix(project_root)
                .unwrap_or(root_path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    resolve_with(&mod_paths, &loader)
}

pub fn resolve_with(
    root_mod_paths: &[String],
    source_loader: &dyn SourceLoader,
) -> (Vec<ir::Plan>, SourceMap, Vec<Diagnostic>) {
    let mut loader = Loader::new(source_loader);

    // Phase 1: Load all modules
    for mod_path in root_mod_paths {
        loader.load_module(mod_path, None);
    }

    let mut diagnostics = loader.diagnostics;

    // Phase 2: Build scopes for all modules
    let mut scopes: HashMap<String, ModuleScope> = HashMap::new();
    for (mod_path, (file_id, module)) in &loader.asts {
        let scope = build_module_scope(mod_path, *file_id, module, &loader.asts, &mut diagnostics);
        scopes.insert(mod_path.clone(), scope);
    }

    // Build FileId → scope lookup for cross-module resolution
    let file_to_mod: HashMap<FileId, &str> = loader
        .asts
        .iter()
        .map(|(path, (fid, _))| (*fid, path.as_str()))
        .collect();
    let scopes_by_file: HashMap<FileId, &ModuleScope> = file_to_mod
        .iter()
        .filter_map(|(fid, path)| scopes.get(*path).map(|s| (*fid, s)))
        .collect();

    // Phase 3: For each test in root modules, build a Plan
    let mut plans = Vec::new();
    for mod_path in root_mod_paths {
        let (file_id, module) = match loader.asts.get(mod_path.as_str()) {
            Some(pair) => pair,
            None => continue,
        };
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
            resolve_with(&[root.to_string()], self)
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
                Diagnostic::CircularImport { .. } => "CircularImport",
                Diagnostic::UndefinedName { .. } => "UndefinedName",
                Diagnostic::DuplicateDefinition { .. } => "DuplicateDefinition",
                Diagnostic::UndefinedVariable { .. } => "UndefinedVariable",
                Diagnostic::CircularEffectDependency { .. } => "CircularEffectDependency",
                Diagnostic::InvalidTimeout { .. } => "InvalidTimeout",
                Diagnostic::ImportNotExported { .. } => "ImportNotExported",
                Diagnostic::InvalidRegex { .. } => "InvalidRegex",
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
    fn test_need_without_alias_defaults_to_exported_shell_name() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "effect StartDb -> db {\n  shell db {\n    > start\n  }\n}\n\ntest \"t\" {\n  need StartDb\n  shell db {\n    > query\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].test.needs[0].node.alias.node, "db",
            "default alias should be the exported shell name, not the effect name"
        );
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
            ir::ShellStmt::Timeout(d) => assert_eq!(*d, Duration::from_secs(10)),
            other => panic!("expected Timeout, got {other:?}"),
        }
        match &stmts[2].node {
            ir::ShellStmt::Timeout(d) => assert_eq!(*d, Duration::from_millis(500)),
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
    fn test_neg_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <!? error\n    <!= bad stuff\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::NegMatchRegex(m)) => {
                assert!(m.timeout_override.is_none());
            }
            other => panic!("expected NegMatchRegex, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::NegMatchLiteral(m)) => {
                assert!(m.timeout_override.is_none());
            }
            other => panic!("expected NegMatchLiteral, got {other:?}"),
        }
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
                assert_eq!(m.timeout_override, Some(Duration::from_secs(2)));
            }
            other => panic!("expected MatchRegex with timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::MatchLiteral(m)) => {
                assert_eq!(m.timeout_override, Some(Duration::from_millis(500)));
            }
            other => panic!("expected MatchLiteral with timeout, got {other:?}"),
        }
    }

    #[test]
    fn test_timed_neg_match_lowering() {
        let mut loader = InMemoryLoader::new();
        loader.add(
            "main",
            "test \"t\" {\n  shell s {\n    <~3s!? error\n    <~1s!= bad\n  }\n}\n",
        );
        let (plans, _, diags) = loader.resolve_one("main");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmts = &plans[0].test.shells[0].node.stmts;
        match &stmts[0].node {
            ir::ShellStmt::Expr(ir::Expr::NegMatchRegex(m)) => {
                assert_eq!(m.timeout_override, Some(Duration::from_secs(3)));
            }
            other => panic!("expected NegMatchRegex with timeout, got {other:?}"),
        }
        match &stmts[1].node {
            ir::ShellStmt::Expr(ir::Expr::NegMatchLiteral(m)) => {
                assert_eq!(m.timeout_override, Some(Duration::from_secs(1)));
            }
            other => panic!("expected NegMatchLiteral with timeout, got {other:?}"),
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
