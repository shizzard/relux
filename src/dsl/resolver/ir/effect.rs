use crate::diagnostics::{
    EffectId as DiagEffectId, EffectName, InvalidReport, IrSpan, LoweringBail,
};
use crate::dsl::parser::ast::{AstEffectDef, AstEffectItem, AstNeedDecl, AstOverlayEntry};
use crate::table::FileId;

use super::block::{IrCleanupBlock, IrShellBlock};
use super::comment::IrComment;
use super::expr::IrExpr;
use super::ident::IrIdent;
use super::keys::LocalEffectKey;
use super::stmt::IrLetStmt;
use super::{IrNode, IrNodeLowering, LoweringContext};

// ─── IrOverlayEntry ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrOverlayEntry {
    key: IrIdent,
    value: IrExpr,
    span: IrSpan,
}

impl IrOverlayEntry {
    pub fn new(key: IrIdent, value: IrExpr, span: IrSpan) -> Self {
        Self { key, value, span }
    }

    pub fn key(&self) -> &IrIdent {
        &self.key
    }

    pub fn value(&self) -> &IrExpr {
        &self.value
    }
}

impl_ir_node_struct!(IrOverlayEntry);

// ─── IrEffectNeed ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrEffectNeed {
    effect: DiagEffectId,
    canonical_overlay: String,
    overlay: Vec<IrOverlayEntry>,
    alias: Option<String>,
    span: IrSpan,
}

impl IrEffectNeed {
    pub fn new(
        effect: DiagEffectId,
        canonical_overlay: String,
        overlay: Vec<IrOverlayEntry>,
        alias: Option<String>,
        span: IrSpan,
    ) -> Self {
        Self {
            effect,
            canonical_overlay,
            overlay,
            alias,
            span,
        }
    }

    pub fn effect(&self) -> &DiagEffectId {
        &self.effect
    }

    pub fn canonical_overlay(&self) -> &str {
        &self.canonical_overlay
    }

    pub fn overlay(&self) -> &[IrOverlayEntry] {
        &self.overlay
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

impl_ir_node_struct!(IrEffectNeed);

// ─── IrEffectItem ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrEffectItem {
    Comment { comment: IrComment, span: IrSpan },
    Need { need: IrEffectNeed, span: IrSpan },
    Let { stmt: IrLetStmt, span: IrSpan },
    Shell { block: IrShellBlock, span: IrSpan },
    Cleanup { block: IrCleanupBlock, span: IrSpan },
}

impl_ir_node_enum!(IrEffectItem {
    Comment,
    Need,
    Let,
    Shell,
    Cleanup
});

// ─── IrEffect ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrEffect {
    name: IrIdent,
    exported_shell: IrIdent,
    needs: Vec<IrEffectNeed>,
    body: Vec<IrEffectItem>,
    span: IrSpan,
}

impl IrEffect {
    pub fn new(
        name: IrIdent,
        exported_shell: IrIdent,
        needs: Vec<IrEffectNeed>,
        body: Vec<IrEffectItem>,
        span: IrSpan,
    ) -> Self {
        Self {
            name,
            exported_shell,
            needs,
            body,
            span,
        }
    }

    pub fn name(&self) -> &IrIdent {
        &self.name
    }

    pub fn exported_shell(&self) -> &IrIdent {
        &self.exported_shell
    }

    pub fn needs(&self) -> &[IrEffectNeed] {
        &self.needs
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
        let value = IrExpr::lower(&ast.value.node, file, ctx)?;
        Ok(IrOverlayEntry::new(
            key,
            value,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

impl IrNodeLowering for IrEffectNeed {
    type Ast = AstNeedDecl;
    fn lower(
        ast: &AstNeedDecl,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let effect_name = &ast.effect.node.name;
        let local_key = LocalEffectKey::new(EffectName(effect_name.clone()));

        // Look up in current scope's effect table
        let global_key = {
            let scope = ctx.current_scope();
            let et = scope
                .effect_table
                .as_ref()
                .expect("effect table must be in scope");
            et.get_global_key(&local_key).cloned()
        };

        let global_key = global_key.ok_or_else(|| {
            LoweringBail::Invalid(InvalidReport::UndefinedEffectNeed {
                name: effect_name.clone(),
                span: IrSpan::new(file.clone(), ast.effect.node.span),
            })
        })?;

        // Resolve the effect (ensures it's lowered and cached)
        ctx.resolve_effect(&global_key)?;

        // Lower overlay entries
        let overlay = ast
            .overlay
            .iter()
            .map(|e| IrOverlayEntry::lower(&e.node, file, ctx))
            .collect::<Result<Vec<_>, _>>()?;

        // Build canonical overlay string (sorted by key)
        let mut canonical_parts: Vec<String> = ast
            .overlay
            .iter()
            .map(|e| format!("{}={}", e.node.key.node.name, e.node.value.node.canonical()))
            .collect();
        canonical_parts.sort();
        let canonical_overlay = canonical_parts.join(",");

        let alias = ast.alias.as_ref().map(|a| a.node.name.clone());

        Ok(IrEffectNeed::new(
            global_key,
            canonical_overlay,
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
            AstEffectItem::Need { decl, span } => {
                let need = IrEffectNeed::lower(decl, file, ctx)?;
                Ok(IrEffectItem::Need {
                    need,
                    span: s(span),
                })
            }
            AstEffectItem::Let { stmt, span } => {
                let ir = IrLetStmt::lower(stmt, file, ctx)?;
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
        let exported_shell = IrIdent::lower(&ast.exported_shell.node, file, ctx)?;

        let mut needs = Vec::new();
        let mut body_items = Vec::new();

        for spanned_item in &ast.body {
            let ir_item = IrEffectItem::lower(&spanned_item.node, file, ctx)?;
            if let IrEffectItem::Need { ref need, .. } = ir_item {
                needs.push(need.clone());
            }
            body_items.push(ir_item);
        }

        Ok(IrEffect::new(
            name,
            exported_shell,
            needs,
            body_items,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::ModulePath;
    use crate::table::FileId;
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
    fn ir_effect_with_needs() {
        let s = test_span();
        let need = IrEffectNeed::new(test_effect_id(), "".into(), vec![], None, s.clone());
        let eff = IrEffect::new(test_ident("Db"), test_ident("db"), vec![need], vec![], s);
        assert_eq!(eff.needs().len(), 1);
    }

    #[test]
    fn ir_effect_empty_needs() {
        let eff = IrEffect::new(
            test_ident("Standalone"),
            test_ident("sh"),
            vec![],
            vec![],
            test_span(),
        );
        assert!(eff.needs().is_empty());
    }

    #[test]
    fn ir_effect_need_canonical_overlay() {
        let need = IrEffectNeed::new(
            test_effect_id(),
            "PORT=5432".into(),
            vec![],
            None,
            test_span(),
        );
        assert_eq!(need.canonical_overlay(), "PORT=5432");
    }

    #[test]
    fn ir_effect_need_no_overlay() {
        let need = IrEffectNeed::new(test_effect_id(), "".into(), vec![], None, test_span());
        assert!(need.overlay().is_empty());
        assert_eq!(need.canonical_overlay(), "");
    }

    #[test]
    fn ir_effect_need_with_alias() {
        let need = IrEffectNeed::new(
            test_effect_id(),
            "".into(),
            vec![],
            Some("my_db".into()),
            test_span(),
        );
        assert_eq!(need.alias(), Some("my_db"));
    }

    #[test]
    fn ir_effect_need_without_alias() {
        let need = IrEffectNeed::new(test_effect_id(), "".into(), vec![], None, test_span());
        assert_eq!(need.alias(), None);
    }

    #[test]
    fn ir_overlay_entry() {
        let s = test_span();
        let val = IrExpr::Var {
            name: "port_var".into(),
            span: s.clone(),
        };
        let entry = IrOverlayEntry::new(test_ident("PORT"), val, s);
        assert_eq!(entry.key().name(), "PORT");
        assert!(matches!(entry.value(), IrExpr::Var { .. }));
    }

    // ─── Effect lowering (cacheable) ──────────────────────────

    use crate::diagnostics::{CycleReport, EffectId, FnId, InvalidReport, LoweringBail};
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_effect_simple() {
        let source = r#"effect Db -> db {
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
    fn lower_effect_with_need() {
        let source = r#"effect Base -> base {
  shell base {
    > base
  }
}
effect App -> app {
  need Base
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
        assert!(!result.unwrap().needs().is_empty());
    }

    #[test]
    fn lower_effect_recursive_need() {
        let source = r#"effect A -> a {
  shell a {
    > a
  }
}
effect B -> b {
  need A
  shell b {
    > b
  }
}
effect C -> c {
  need B
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
        let source = r#"effect A -> a {
  need B
  shell a {
    > a
  }
}
effect B -> b {
  need A
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
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::Cycle(
                CycleReport::Effect { .. }
            )))
        ));
    }

    #[test]
    fn lower_effect_cycle_self() {
        let source = r#"effect A -> a {
  need A
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
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(InvalidReport::Cycle(
                CycleReport::Effect { .. }
            )))
        ));
    }

    #[test]
    fn lower_effect_cycle_deep() {
        let source = r#"effect A -> a {
  need B
  shell a {
    > a
  }
}
effect B -> b {
  need C
  shell b {
    > b
  }
}
effect C -> c {
  need A
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
        if let Err(LoweringBail::Invalid(InvalidReport::Cycle(CycleReport::Effect { chain }))) =
            &result
        {
            assert_eq!(chain.len(), 3);
        } else {
            panic!("expected effect cycle, got {:?}", result);
        }
    }

    #[test]
    fn lower_effect_memoized() {
        let source = r#"effect Shared -> sh {
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
        let source = r#"effect Db -> db {
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
    fn lower_effect_undefined_need() {
        let source = r#"effect A -> a {
  need Nonexistent
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
        assert!(matches!(
            result,
            Err(LoweringBail::Invalid(
                InvalidReport::UndefinedEffectNeed { .. }
            ))
        ));
    }

    #[test]
    fn lower_effect_with_overlay() {
        let source = r#"effect Db -> db {
  shell db {
    > start
  }
}
effect App -> app {
  need Db { PORT = "5432" }
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
        let need = &result.needs()[0];
        assert!(!need.overlay().is_empty());
    }

    #[test]
    fn lower_effect_no_overlay_canonical() {
        let source = r#"effect Db -> db {
  shell db {
    > start
  }
}
effect App -> app {
  need Db
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
        let need = &result.needs()[0];
        assert_eq!(need.canonical_overlay(), "");
    }

    #[test]
    fn lower_effect_with_let_vars() {
        let source = r#"effect Db -> db {
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
effect Db -> db {
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
    fn lower_effect_need_with_alias() {
        let source = r#"effect Db -> db {
  shell db {
    > start
  }
}
effect App -> app {
  need Db as mydb
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
        let need = &result.needs()[0];
        assert_eq!(need.alias(), Some("mydb"));
    }

    #[test]
    fn lower_effect_need_without_alias() {
        let source = r#"effect Db -> db {
  shell db {
    > start
  }
}
effect App -> app {
  need Db
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
        let need = &result.needs()[0];
        assert!(need.alias().is_none());
    }

    #[test]
    fn lower_effect_error_cached() {
        let source = r#"effect A -> a {
  need Nonexistent
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
}
