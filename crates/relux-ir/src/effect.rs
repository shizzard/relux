use relux_ast::AstEffectDef;
use relux_ast::AstEffectItem;
use relux_ast::AstOverlayEntry;
use relux_ast::AstStartDecl;
use relux_core::diagnostics::EffectId as DiagEffectId;
use relux_core::diagnostics::EffectName;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::block::IrCleanupBlock;
use super::block::IrShellBlock;
use super::comment::IrComment;
use super::expr::IrPureExpr;
use super::ident::IrIdent;
use super::stmt::IrPureLetStmt;
use super::tables::LocalEffectKey;

// ─── IrOverlayEntry ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrOverlayEntry {
    key: IrIdent,
    value: IrPureExpr,
    span: IrSpan,
}

impl IrOverlayEntry {
    pub fn new(key: IrIdent, value: IrPureExpr, span: IrSpan) -> Self {
        Self { key, value, span }
    }

    pub fn key(&self) -> &IrIdent {
        &self.key
    }

    pub fn value(&self) -> &IrPureExpr {
        &self.value
    }
}

impl_ir_node_struct!(IrOverlayEntry);

// ─── IrEffectStart ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrEffectStart {
    effect: DiagEffectId,
    overlay: Vec<IrOverlayEntry>,
    alias: Option<String>,
    span: IrSpan,
}

impl IrEffectStart {
    pub fn new(
        effect: DiagEffectId,
        overlay: Vec<IrOverlayEntry>,
        alias: Option<String>,
        span: IrSpan,
    ) -> Self {
        Self {
            effect,
            overlay,
            alias,
            span,
        }
    }

    pub fn effect(&self) -> &DiagEffectId {
        &self.effect
    }

    pub fn overlay(&self) -> &[IrOverlayEntry] {
        &self.overlay
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

impl_ir_node_struct!(IrEffectStart);

// ─── IrExposeDecl ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum IrExposeKind {
    Shell,
    Var,
}

#[derive(Debug, Clone)]
pub struct IrExposeDecl {
    kind: IrExposeKind,
    qualifier: Option<String>,
    target: String,
    alias: Option<String>,
    target_span: IrSpan,
    span: IrSpan,
}

impl IrExposeDecl {
    pub fn new(
        kind: IrExposeKind,
        qualifier: Option<String>,
        target: String,
        alias: Option<String>,
        target_span: IrSpan,
        span: IrSpan,
    ) -> Self {
        Self {
            kind,
            qualifier,
            target,
            alias,
            target_span,
            span,
        }
    }

    pub fn target_span(&self) -> &IrSpan {
        &self.target_span
    }

    pub fn kind(&self) -> &IrExposeKind {
        &self.kind
    }

    pub fn qualifier(&self) -> Option<&str> {
        self.qualifier.as_deref()
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    /// The name callers use to refer to this exposed item.
    /// Falls back to the target name if no alias is given.
    pub fn exposed_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.target)
    }
}

impl_ir_node_struct!(IrExposeDecl);

// ─── IrEffectItem ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrEffectItem {
    Comment { comment: IrComment, span: IrSpan },
    Expect { vars: Vec<IrIdent>, span: IrSpan },
    Start { start: IrEffectStart, span: IrSpan },
    Let { stmt: IrPureLetStmt, span: IrSpan },
    Expose { decl: IrExposeDecl, span: IrSpan },
    Shell { block: IrShellBlock, span: IrSpan },
    Cleanup { block: IrCleanupBlock, span: IrSpan },
}

impl_ir_node_enum!(IrEffectItem {
    Comment,
    Expect,
    Start,
    Let,
    Expose,
    Shell,
    Cleanup
});

// ─── IrEffect ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrEffect {
    name: IrIdent,
    expects: Vec<IrIdent>,
    exposes: Vec<IrExposeDecl>,
    starts: Vec<IrEffectStart>,
    body: Vec<IrEffectItem>,
    span: IrSpan,
}

impl IrEffect {
    pub fn new(
        name: IrIdent,
        expects: Vec<IrIdent>,
        exposes: Vec<IrExposeDecl>,
        starts: Vec<IrEffectStart>,
        body: Vec<IrEffectItem>,
        span: IrSpan,
    ) -> Self {
        Self {
            name,
            expects,
            exposes,
            starts,
            body,
            span,
        }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn expects(&self) -> &[IrIdent] {
        &self.expects
    }

    pub fn exposes(&self) -> &[IrExposeDecl] {
        &self.exposes
    }

    pub fn shell_exposes(&self) -> impl Iterator<Item = &IrExposeDecl> {
        self.exposes
            .iter()
            .filter(|e| *e.kind() == IrExposeKind::Shell)
    }

    pub fn var_exposes(&self) -> impl Iterator<Item = &IrExposeDecl> {
        self.exposes
            .iter()
            .filter(|e| *e.kind() == IrExposeKind::Var)
    }

    pub fn starts(&self) -> &[IrEffectStart] {
        &self.starts
    }

    pub fn body(&self) -> &[IrEffectItem] {
        &self.body
    }
}

impl_ir_node_struct!(IrEffect);

// ═══════════════════════════════════════════════════════════════
// IrNodeLowering implementations
// ═══════════════════════════════════════════════════════════════

impl IrNodeLowering for IrOverlayEntry {
    type Ast = AstOverlayEntry;
    fn lower(
        ast: &AstOverlayEntry,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let key = IrIdent::lower(&ast.key.node, file, ctx)?;
        let value = IrPureExpr::lower(&ast.value.node, file, ctx)?;
        Ok(IrOverlayEntry::new(
            key,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrEffectStart {
    type Ast = AstStartDecl;
    fn lower(
        ast: &AstStartDecl,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let effect_name = &ast.effect.node.name;
        let local_key = LocalEffectKey::new(EffectName(effect_name.clone()));

        // Look up in current scope's effect table
        let global_key = {
            let scope = ctx.current_scope();
            scope.tables.effects.get_global_key(&local_key).cloned()
        };

        let global_key = global_key.ok_or_else(|| {
            LoweringBail::invalid(InvalidReport::undefined_effect_start(
                effect_name.clone(),
                IrSpan::new(file.clone(), ast.effect.node.span),
            ))
        })?;

        // Lower overlay entries
        let overlay = ast
            .overlay
            .iter()
            .map(|e| IrOverlayEntry::lower(&e.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        // Build a child shallow env with overlay key names for nested effect lowering
        let saved_shallow = ctx.shallow_env().cloned();
        if let Some(caller_env) = &saved_shallow {
            let overlay_names = overlay.iter().map(|e| e.key().name().to_string());
            let child = std::sync::Arc::new(crate::shallow_env::ShallowLayeredEnv::child(
                std::sync::Arc::clone(caller_env),
                overlay_names,
            ));
            ctx.set_shallow_env(child);
        }

        // Resolve the effect (ensures it's lowered and cached; may recurse).
        // IrEffect::lower may mutate ctx.shallow_env (pushing expect/let names
        // for inner start validation), so we must restore the caller's env
        // afterward. We split resolve + restore + `?` to guarantee restoration
        // even on error — the IrNodeLowering trait signature prevents passing
        // the shallow env as a parameter, forcing us to thread it through ctx
        // with manual save/restore.
        let resolved = ctx.resolve_effect(&global_key);
        if let Some(env) = saved_shallow {
            ctx.set_shallow_env(env);
        }
        let resolved = resolved?;

        // Validate expect satisfiability: every expected var must be
        // in the overlay keys or reachable in the caller's shallow env.
        if let Some(caller_env) = ctx.shallow_env() {
            let overlay_names: std::collections::HashSet<String> =
                overlay.iter().map(|e| e.key().name().to_string()).collect();
            for expected in resolved.expects() {
                let name = expected.name();
                if !overlay_names.contains(name) && !caller_env.contains(name) {
                    return Err(LoweringBail::invalid(InvalidReport::unsatisfied_expect(
                        resolved.name().name().to_string(),
                        name.to_string(),
                        expected.span().clone(),
                        IrSpan::new(file.clone(), ast.span),
                    )));
                }
            }
        }

        let alias = ast.alias.as_ref().map(|a| a.node.name.clone());

        Ok(IrEffectStart::new(
            global_key,
            overlay,
            alias,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrEffectItem {
    type Ast = AstEffectItem;
    fn lower(
        ast: &AstEffectItem,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let s = |span: &relux_core::Span| IrSpan::new(file.clone(), *span);
        match ast {
            AstEffectItem::Comment { span, .. } => {
                let comment = IrComment::lower(span, file, ctx)?;
                Ok(IrEffectItem::Comment {
                    comment,
                    span: s(span),
                })
            }
            AstEffectItem::Start { decl, span } => {
                let start = IrEffectStart::lower(decl, file, ctx)?;
                Ok(IrEffectItem::Start {
                    start,
                    span: s(span),
                })
            }
            AstEffectItem::Let { stmt, span } => {
                let ir = IrPureLetStmt::lower(stmt, file, ctx)?;
                Ok(IrEffectItem::Let {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstEffectItem::Shell { block, span } => {
                let ir = IrShellBlock::lower(block, file, ctx)?;
                Ok(IrEffectItem::Shell {
                    block: ir,
                    span: s(span),
                })
            }
            AstEffectItem::Cleanup { block, span } => {
                let ir = IrCleanupBlock::lower(block, file, ctx)?;
                Ok(IrEffectItem::Cleanup {
                    block: ir,
                    span: s(span),
                })
            }
            AstEffectItem::Expect { decl, span } => {
                let vars = decl
                    .vars
                    .iter()
                    .map(|v| IrIdent::lower(&v.node, file, ctx))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(IrEffectItem::Expect {
                    vars,
                    span: s(span),
                })
            }
            AstEffectItem::Expose { decl, span } => {
                let kind = match &decl.kind {
                    relux_ast::AstExposeKind::Shell { .. } => IrExposeKind::Shell,
                    relux_ast::AstExposeKind::Var { .. } => IrExposeKind::Var,
                };
                let qualifier = decl.qualifier.as_ref().map(|q| q.node.name.clone());
                let target = decl.target.node.name.clone();
                let target_span = s(&decl.target.span);
                let alias = decl.alias.as_ref().map(|a| a.node.name.clone());
                let ir = IrExposeDecl::new(kind, qualifier, target, alias, target_span, s(span));
                Ok(IrEffectItem::Expose {
                    decl: ir,
                    span: s(span),
                })
            }
        }
    }
}

impl IrNodeLowering for IrEffect {
    type Ast = AstEffectDef;
    fn lower(
        ast: &AstEffectDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let name = IrIdent::lower(&ast.name.node, file, ctx)?;

        let mut expects = Vec::new();
        let mut exposes = Vec::new();
        let mut starts = Vec::new();
        let mut body_items = Vec::new();

        for spanned_item in &ast.body {
            let ir_item = IrEffectItem::lower(&spanned_item.node, file, ctx)?;
            match &ir_item {
                IrEffectItem::Start { start, .. } => starts.push(start.clone()),
                IrEffectItem::Expect { vars, .. } => {
                    expects.extend(vars.clone());
                    // Expected vars are guaranteed available (validated at start site);
                    // add them to the shallow env so inner starts can see them.
                    if let Some(env) = ctx.shallow_env() {
                        let names = vars.iter().map(|v| v.name().to_string());
                        let updated =
                            std::sync::Arc::new(crate::shallow_env::ShallowLayeredEnv::child(
                                std::sync::Arc::clone(env),
                                names,
                            ));
                        ctx.set_shallow_env(updated);
                    }
                }
                IrEffectItem::Let { stmt, .. } => {
                    // Track let-bound names for inner start expect checking
                    if let Some(env) = ctx.shallow_env() {
                        let updated =
                            std::sync::Arc::new(crate::shallow_env::ShallowLayeredEnv::with_name(
                                env,
                                stmt.name().name().to_string(),
                            ));
                        ctx.set_shallow_env(updated);
                    }
                }
                IrEffectItem::Expose { decl, .. } => exposes.push(decl.clone()),
                _ => {}
            }
            body_items.push(ir_item);
        }

        // Validate expose references
        let shell_names: Vec<String> = body_items
            .iter()
            .filter_map(|item| match item {
                IrEffectItem::Shell { block, .. } if block.qualifier().is_none() => {
                    Some(block.name().name().to_string())
                }
                _ => None,
            })
            .collect();
        let let_names: Vec<String> = body_items
            .iter()
            .filter_map(|item| match item {
                IrEffectItem::Let { stmt, .. } => Some(stmt.name().name().to_string()),
                _ => None,
            })
            .collect();
        // Build maps from alias → set of shells/vars exposed by that dependency
        let mut dep_exposed_shells: std::collections::HashMap<
            String,
            std::collections::HashSet<String>,
        > = std::collections::HashMap::new();
        let mut dep_exposed_vars: std::collections::HashMap<
            String,
            std::collections::HashSet<String>,
        > = std::collections::HashMap::new();
        for start in &starts {
            if let Some(alias) = start.alias()
                && let Some(Ok(eff)) = ctx.effects().get(start.effect()).map(|r| r.as_ref())
            {
                let shells: std::collections::HashSet<String> = eff
                    .shell_exposes()
                    .map(|e| e.exposed_name().to_string())
                    .collect();
                let vars: std::collections::HashSet<String> = eff
                    .var_exposes()
                    .map(|e| e.exposed_name().to_string())
                    .collect();
                dep_exposed_shells.insert(alias.to_string(), shells);
                dep_exposed_vars.insert(alias.to_string(), vars);
            }
        }

        for expose in &exposes {
            let valid = match expose.kind() {
                IrExposeKind::Shell => {
                    if let Some(qualifier) = expose.qualifier() {
                        dep_exposed_shells
                            .get(qualifier)
                            .is_some_and(|shells| shells.contains(expose.target()))
                    } else {
                        shell_names.contains(&expose.target().to_string())
                    }
                }
                IrExposeKind::Var => {
                    if let Some(qualifier) = expose.qualifier() {
                        dep_exposed_vars
                            .get(qualifier)
                            .is_some_and(|vars| vars.contains(expose.target()))
                    } else {
                        let_names.contains(&expose.target().to_string())
                    }
                }
            };
            if !valid {
                let label = if let Some(q) = expose.qualifier() {
                    format!("{}.{}", q, expose.target())
                } else {
                    expose.target().to_string()
                };
                let report = match expose.kind() {
                    IrExposeKind::Shell => InvalidReport::invalid_shell_expose(
                        name.name().to_string(),
                        label,
                        expose.target_span().clone(),
                    ),
                    IrExposeKind::Var => InvalidReport::invalid_var_expose(
                        name.name().to_string(),
                        label,
                        expose.target_span().clone(),
                    ),
                };
                return Err(LoweringBail::invalid(report));
            }
        }

        Ok(IrEffect::new(
            name,
            expects,
            exposes,
            starts,
            body_items,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

// ─── Tests ───────────────────────────────────────────────────
