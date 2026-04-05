use crate::core::table::FileId;
use crate::diagnostics::EffectId as DiagEffectId;
use crate::diagnostics::EffectName;
use crate::diagnostics::InvalidReport;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::dsl::parser::ast::AstEffectDef;
use crate::dsl::parser::ast::AstEffectItem;
use crate::dsl::parser::ast::AstOverlayEntry;
use crate::dsl::parser::ast::AstStartDecl;

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

#[derive(Debug, Clone)]
pub struct IrExposeDecl {
    qualifier: Option<String>,
    shell: String,
    alias: Option<String>,
    span: IrSpan,
}

impl IrExposeDecl {
    pub fn new(
        qualifier: Option<String>,
        shell: String,
        alias: Option<String>,
        span: IrSpan,
    ) -> Self {
        Self {
            qualifier,
            shell,
            alias,
            span,
        }
    }

    pub fn qualifier(&self) -> Option<&str> {
        self.qualifier.as_deref()
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// The name callers use to refer to this exposed shell.
    /// Falls back to the shell name if no alias is given.
    pub fn exposed_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.shell)
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
            let child =
                std::sync::Arc::new(crate::dsl::resolver::shallow_env::ShallowLayeredEnv::child(
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
        let s = |span: &crate::Span| IrSpan::new(file.clone(), *span);
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
                let qualifier = decl.qualifier.as_ref().map(|q| q.node.name.clone());
                let shell = decl.shell.node.name.clone();
                let alias = decl.alias.as_ref().map(|a| a.node.name.clone());
                let ir = IrExposeDecl::new(qualifier, shell, alias, s(span));
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
                        let updated = std::sync::Arc::new(
                            crate::dsl::resolver::shallow_env::ShallowLayeredEnv::child(
                                std::sync::Arc::clone(env),
                                names,
                            ),
                        );
                        ctx.set_shallow_env(updated);
                    }
                }
                IrEffectItem::Let { stmt, .. } => {
                    // Track let-bound names for inner start expect checking
                    if let Some(env) = ctx.shallow_env() {
                        let updated = std::sync::Arc::new(
                            crate::dsl::resolver::shallow_env::ShallowLayeredEnv::with_name(
                                env,
                                stmt.name().name().to_string(),
                            ),
                        );
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
                IrEffectItem::Shell { block, .. } => Some(block.name().name().to_string()),
                _ => None,
            })
            .collect();
        // Build map from alias → set of shells exposed by that dependency
        let mut dep_exposed: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for start in &starts {
            if let Some(alias) = start.alias() {
                let exposed_names: std::collections::HashSet<String> = ctx
                    .effects()
                    .get(start.effect())
                    .and_then(|r| r.as_ref().ok())
                    .map(|eff| {
                        eff.exposes()
                            .iter()
                            .map(|e| e.exposed_name().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                dep_exposed.insert(alias.to_string(), exposed_names);
            }
        }

        for expose in &exposes {
            let valid = if let Some(qualifier) = expose.qualifier() {
                // Qualified: `expose alias.shell as name` — alias must exist
                // and the dependency must actually expose that shell
                dep_exposed
                    .get(qualifier)
                    .is_some_and(|shells| shells.contains(expose.shell()))
            } else {
                // Simple: `expose shell` — shell must be a local shell
                shell_names.contains(&expose.shell().to_string())
            };
            if !valid {
                let label = if let Some(q) = expose.qualifier() {
                    format!("{}.{}", q, expose.shell())
                } else {
                    expose.shell().to_string()
                };
                return Err(LoweringBail::invalid(InvalidReport::invalid_expose(
                    name.name().to_string(),
                    label,
                    expose.span().clone(),
                )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use crate::diagnostics::ModulePath;
    use std::path::PathBuf;

    fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(0, 10))
    }

    fn test_ident(name: &str) -> IrIdent {
        IrIdent::new(name, test_span())
    }

    fn test_effect_id() -> DiagEffectId {
        DiagEffectId {
            module: ModulePath("test".into()),
            name: EffectName("Db".into()),
        }
    }

    #[test]
    fn ir_effect_with_starts() {
        let s = test_span();
        let start = IrEffectStart::new(test_effect_id(), vec![], None, s.clone());
        let eff = IrEffect::new(test_ident("Db"), vec![], vec![], vec![start], vec![], s);
        assert_eq!(eff.starts().len(), 1);
    }

    #[test]
    fn ir_effect_empty_starts() {
        let eff = IrEffect::new(
            test_ident("Standalone"),
            vec![],
            vec![],
            vec![],
            vec![],
            test_span(),
        );
        assert!(eff.starts().is_empty());
    }

    #[test]
    fn ir_effect_start_no_overlay() {
        let start = IrEffectStart::new(test_effect_id(), vec![], None, test_span());
        assert!(start.overlay().is_empty());
    }

    #[test]
    fn ir_effect_start_with_alias() {
        let start = IrEffectStart::new(test_effect_id(), vec![], Some("my_db".into()), test_span());
        assert_eq!(start.alias(), Some("my_db"));
    }

    #[test]
    fn ir_effect_start_without_alias() {
        let start = IrEffectStart::new(test_effect_id(), vec![], None, test_span());
        assert_eq!(start.alias(), None);
    }

    #[test]
    fn ir_overlay_entry() {
        let s = test_span();
        let val = IrPureExpr::Var {
            name: "port_var".into(),
            span: s.clone(),
        };
        let entry = IrOverlayEntry::new(test_ident("PORT"), val, s);
        assert_eq!(entry.key().name(), "PORT");
        assert!(matches!(entry.value(), IrPureExpr::Var { .. }));
    }

    // ─── Effect lowering (cacheable) ──────────────────────────

    use crate::diagnostics::CycleReport;
    use crate::diagnostics::EffectId;
    use crate::diagnostics::FnId;
    use crate::diagnostics::InvalidReport;
    use crate::diagnostics::LoweringBail;
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_effect_simple() {
        let source = r#"effect Db {
  shell db {
    > start_db
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        assert_eq!(eff.name().name(), "Db");
    }

    #[test]
    fn lower_effect_with_start() {
        let source = r#"effect Base {
  shell base {
    > base
  }
}
effect App {
  start Base
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        assert!(!result.unwrap().starts().is_empty());
    }

    #[test]
    fn lower_effect_recursive_start() {
        let source = r#"effect A {
  shell a {
    > a
  }
}
effect B {
  start A
  shell b {
    > b
  }
}
effect C {
  start B
  shell c {
    > c
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("C".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let a_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let b_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("B".into()),
        };
        assert!(ctx.effects().get(&a_id).is_some());
        assert!(ctx.effects().get(&b_id).is_some());
    }

    #[test]
    fn lower_effect_cycle_mutual() {
        let source = r#"effect A {
  start B
  shell a {
    > a
  }
}
effect B {
  start A
  shell b {
    > b
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_cycle_self() {
        let source = r#"effect A {
  start A
  shell a {
    > a
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_cycle_deep() {
        let source = r#"effect A {
  start B
  shell a {
    > a
  }
}
effect B {
  start C
  shell b {
    > b
  }
}
effect C {
  start A
  shell c {
    > c
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_err());
        if let Err(LoweringBail::Invalid(inner)) = &result {
            if let InvalidReport::Cycle(CycleReport::Effect { chain }) = inner.as_ref() {
                assert_eq!(chain.len(), 3);
            } else {
                panic!("expected effect cycle, got {:?}", result);
            }
        } else {
            panic!("expected effect cycle, got {:?}", result);
        }
    }

    #[test]
    fn lower_effect_memoized() {
        let source = r#"effect Shared {
  shell sh {
    > s
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Shared".into()),
        };
        ctx.resolve_effect(&effect_id).unwrap();
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_effect_with_cleanup() {
        let source = r#"effect Db {
  shell db {
    > start
  }
  cleanup {
    > stop
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id).unwrap();
        assert!(
            result
                .body()
                .iter()
                .any(|item| matches!(item, IrEffectItem::Cleanup { .. }))
        );
    }

    #[test]
    fn lower_effect_undefined_start() {
        let source = r#"effect A {
  start Nonexistent
  shell a {
    > a
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_with_overlay() {
        let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = "5432" }
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id).unwrap();
        let start = &result.starts()[0];
        assert!(!start.overlay().is_empty());
    }

    #[test]
    fn lower_effect_with_let_vars() {
        let source = r#"effect Db {
  let port = "5432"
  shell db {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id).unwrap();
        assert!(
            result
                .body()
                .iter()
                .any(|item| matches!(item, IrEffectItem::Let { .. }))
        );
    }

    #[test]
    fn lower_effect_with_fn_calls() {
        let source = r#"fn setup() {
  > setup
}
effect Db {
  shell db {
    setup()
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let setup_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "setup".into(),
            arity: 0,
        };
        assert!(ctx.functions().get(&setup_id).is_some());
    }

    #[test]
    fn lower_effect_start_with_alias() {
        let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db as mydb
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id).unwrap();
        let start = &result.starts()[0];
        assert_eq!(start.alias(), Some("mydb"));
    }

    #[test]
    fn lower_effect_start_without_alias() {
        let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id).unwrap();
        let start = &result.starts()[0];
        assert!(start.alias().is_none());
    }

    #[test]
    fn lower_effect_error_cached() {
        let source = r#"effect A {
  start Nonexistent
  shell a {
    > a
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("A".into()),
        };
        let result1 = ctx.resolve_effect(&effect_id);
        assert!(result1.is_err());
        let result2 = ctx.resolve_effect(&effect_id);
        assert!(result2.is_err());
    }

    // ─── Purity enforcement tests ────────────────────────────

    #[test]
    fn lower_effect_let_rejects_impure_fn_call() {
        let source = r#"fn impure_fn() {
  > cmd
}
effect E {
  let x = impure_fn()
  shell sh {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("E".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_let_accepts_pure_fn_call() {
        let source = r#"effect E {
  let x = trim("hi")
  shell sh {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("E".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        assert!(
            eff.body()
                .iter()
                .any(|item| matches!(item, IrEffectItem::Let { .. }))
        );
    }

    #[test]
    fn lower_effect_let_accepts_string_literal() {
        let source = r#"effect E {
  let x = "hello"
  shell sh {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("E".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_effect_let_accepts_var_ref() {
        let source = r#"effect E {
  let x = "val"
  let y = x
  shell sh {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("E".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
    }

    #[test]
    fn lower_overlay_accepts_pure_fn_call() {
        let source = r#"effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = available_port() }
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        let start = &eff.starts()[0];
        assert!(!start.overlay().is_empty());
    }

    #[test]
    fn lower_overlay_rejects_impure_fn_call() {
        let source = r#"fn impure_fn() {
  > cmd
}
effect Db {
  shell db {
    > start
  }
}
effect App {
  start Db { PORT = impure_fn() }
  shell app {
    > app
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("App".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    // ─── Expose validation ──────────────────────────────────

    #[test]
    fn lower_effect_expose_valid_local_shell() {
        let source = r#"effect Db {
  expose db
  shell db {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().exposes().len(), 1);
    }

    #[test]
    fn lower_effect_expose_invalid_shell() {
        let source = r#"effect Db {
  expose nonexistent
  shell db {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_expose_qualified_valid() {
        let source = r#"effect Base {
  expose sh
  shell sh {
    > base
  }
}
effect Wrapper {
  start Base as b
  expose b.sh as base_shell
  shell wrapper {
    > wrapper
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Wrapper".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        assert_eq!(eff.exposes().len(), 1);
        assert_eq!(eff.exposes()[0].exposed_name(), "base_shell");
    }

    #[test]
    fn lower_effect_expose_qualified_invalid_alias() {
        let source = r#"effect Base {
  expose sh
  shell sh {
    > base
  }
}
effect Wrapper {
  start Base as b
  expose nonexistent.sh
  shell wrapper {
    > wrapper
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Wrapper".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_effect_expose_qualified_unexposed_shell() {
        // Base exposes `sh` but NOT `internal`.
        // Wrapper tries to re-expose `b.internal` — this should fail
        // because Base does not expose `internal` to callers.
        let source = r#"effect Base {
  expose sh
  shell sh {
    > base
  }
  shell internal {
    > secret
  }
}
effect Wrapper {
  start Base as b
  expose b.internal as leaked
  shell wrapper {
    > wrapper
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Wrapper".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(
            matches!(result, Err(LoweringBail::Invalid(_))),
            "expose should reject referencing a shell not exposed by the dependency"
        );
    }

    #[test]
    fn lower_effect_expect_vars() {
        let source = r#"effect Db {
  expect DB_PORT, DB_NAME
  expose db
  shell db {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Db".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        assert_eq!(eff.expects().len(), 2);
        assert_eq!(eff.expects()[0].name(), "DB_PORT");
        assert_eq!(eff.expects()[1].name(), "DB_NAME");
    }

    #[test]
    fn lower_effect_no_expose_is_valid() {
        let source = r#"effect SideEffect {
  shell setup {
    > side effect
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("SideEffect".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        assert!(result.unwrap().exposes().is_empty());
    }

    #[test]
    fn lower_effect_expose_local_with_alias() {
        let source = r#"effect Auth {
  expose auth as svc
  shell auth {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Auth".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        let eff = result.unwrap();
        assert_eq!(eff.exposes().len(), 1);
        assert_eq!(eff.exposes()[0].exposed_name(), "svc");
    }

    #[test]
    fn lower_effect_no_expect_is_valid() {
        let source = r#"effect Simple {
  expose sh
  shell sh {
    > start
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let effect_id = EffectId {
            module: ModulePath("tests/a".into()),
            name: EffectName("Simple".into()),
        };
        let result = ctx.resolve_effect(&effect_id);
        assert!(result.is_ok());
        assert!(result.unwrap().expects().is_empty());
    }
}
