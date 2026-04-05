use crate::core::table::FileId;
use crate::diagnostics::InvalidReport;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::dsl::parser::ast::AstTestDef;
use crate::dsl::parser::ast::AstTestItem;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::block::IrCleanupBlock;
use super::block::IrShellBlock;
use super::comment::IrComment;
use super::effect::IrEffectStart;
use super::stmt::IrPureLetStmt;

// ─── IrTestItem ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum IrTestItem {
    Comment { comment: IrComment, span: IrSpan },
    DocString { text: String, span: IrSpan },
    Start { start: IrEffectStart, span: IrSpan },
    Let { stmt: IrPureLetStmt, span: IrSpan },
    Shell { block: IrShellBlock, span: IrSpan },
    Cleanup { block: IrCleanupBlock, span: IrSpan },
}

impl_ir_node_enum!(IrTestItem {
    Comment,
    DocString,
    Start,
    Let,
    Shell,
    Cleanup
});

// ─── IrTest ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrTest {
    name: String,
    starts: Vec<IrEffectStart>,
    body: Vec<IrTestItem>,
    span: IrSpan,
}

impl IrTest {
    pub fn new(
        name: impl Into<String>,
        starts: Vec<IrEffectStart>,
        body: Vec<IrTestItem>,
        span: IrSpan,
    ) -> Self {
        Self {
            name: name.into(),
            starts,
            body,
            span,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn starts(&self) -> &[IrEffectStart] {
        &self.starts
    }

    pub fn body(&self) -> &[IrTestItem] {
        &self.body
    }
}

impl_ir_node_struct!(IrTest);

// ═══════════════════════════════════════════════════════════════
// IrNodeLowering implementations
// ═══════════════════════════════════════════════════════════════

impl IrNodeLowering for IrTestItem {
    type Ast = AstTestItem;
    fn lower(
        ast: &AstTestItem,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let s = |span: &crate::Span| IrSpan::new(file.clone(), *span);
        match ast {
            AstTestItem::Comment { span, .. } => {
                let comment = IrComment::lower(span, file, ctx)?;
                Ok(IrTestItem::Comment {
                    comment,
                    span: s(span),
                })
            }
            AstTestItem::DocString { text, span } => Ok(IrTestItem::DocString {
                text: text.clone(),
                span: s(span),
            }),
            AstTestItem::Start { decl, span } => {
                let start = IrEffectStart::lower(decl, file, ctx)?;
                Ok(IrTestItem::Start {
                    start,
                    span: s(span),
                })
            }
            AstTestItem::Let { stmt, span } => {
                let ir = IrPureLetStmt::lower(stmt, file, ctx)?;
                Ok(IrTestItem::Let {
                    stmt: ir,
                    span: s(span),
                })
            }
            AstTestItem::Shell { block, span } => {
                let ir = IrShellBlock::lower(block, file, ctx)?;
                Ok(IrTestItem::Shell {
                    block: ir,
                    span: s(span),
                })
            }
            AstTestItem::Cleanup { block, span } => {
                let ir = IrCleanupBlock::lower(block, file, ctx)?;
                Ok(IrTestItem::Cleanup {
                    block: ir,
                    span: s(span),
                })
            }
        }
    }
}

impl IrNodeLowering for IrTest {
    type Ast = AstTestDef;
    /// Lower a test body. Assumes scope is already pushed on ctx.
    fn lower(
        ast: &AstTestDef,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        let mut starts = Vec::new();
        let mut body_items = Vec::new();

        for spanned_item in &ast.body {
            let ir_item = IrTestItem::lower(&spanned_item.node, file, ctx)?;
            // Track let-bound names in the shallow env for expect checking
            if let IrTestItem::Let { ref stmt, .. } = ir_item
                && let Some(env) = ctx.shallow_env()
            {
                let updated = std::sync::Arc::new(
                    crate::dsl::resolver::shallow_env::ShallowLayeredEnv::with_name(
                        env,
                        stmt.name().name().to_string(),
                    ),
                );
                ctx.set_shallow_env(updated);
            }
            if let IrTestItem::Start { ref start, .. } = ir_item {
                starts.push(start.clone());
            }
            body_items.push(ir_item);
        }

        let has_nonempty_shell = body_items.iter().any(
            |item| matches!(item, IrTestItem::Shell { block, .. } if !block.body().is_empty()),
        );
        if !has_nonempty_shell {
            return Err(LoweringBail::invalid(InvalidReport::empty_test_body(
                ast.name.node.clone(),
                IrSpan::new(file.clone(), ast.span),
            )));
        }

        Ok(IrTest::new(
            &ast.name.node,
            starts,
            body_items,
            IrSpan::new(file.clone(), ast.span),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::FnId;
    use crate::diagnostics::ModulePath;
    use crate::dsl::resolver::lower::test_helpers::*;

    // ─── Test lowering ────────────────────────────────────────

    #[test]
    fn lower_test_simple() {
        let source = r#"test "basic" {
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "basic");
    }

    #[test]
    fn lower_test_with_starts() {
        let source = r#"effect Db {
  shell db {
    > start
  }
}
test "with needs" {
  start Db
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a").unwrap();
        assert!(!result.starts().is_empty());
    }

    #[test]
    fn lower_test_with_multiple_starts() {
        let source = r#"effect Db {
  shell db {
    > db
  }
}
effect Cache {
  shell cache {
    > cache
  }
}
test "multi" {
  start Db
  start Cache
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a").unwrap();
        assert_eq!(result.starts().len(), 2);
    }

    #[test]
    fn lower_test_no_timeout() {
        let source = r#"test "t" {
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a").unwrap();
        assert_eq!(result.name(), "t");
    }

    #[test]
    fn lower_test_calls_fn() {
        let source = r#"fn helper() {
  > help
}
test "t" {
  shell sh {
    helper()
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(result.is_ok());
        let helper_id = FnId {
            module: ModulePath("tests/a".into()),
            name: "helper".into(),
            arity: 0,
        };
        assert!(ctx.functions().get(&helper_id).is_some());
    }

    #[test]
    fn lower_test_calls_bif() {
        let source = r#"test "t" {
  shell sh {
    sleep("1")
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(result.is_ok());
    }

    #[test]
    fn lower_test_with_cleanup() {
        let source = r#"test "t" {
  shell sh {
    > cmd
  }
  cleanup {
    > clean
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a").unwrap();
        assert!(
            result
                .body()
                .iter()
                .any(|item| matches!(item, IrTestItem::Cleanup { .. }))
        );
    }

    #[test]
    fn lower_test_comments_stripped() {
        let source = r#"test "t" {
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a").unwrap();
        assert!(
            result
                .body()
                .iter()
                .all(|item| !matches!(item, IrTestItem::Start { .. }))
        );
        assert!(!result.body().is_empty());
    }

    // ─── Purity enforcement tests ────────────────────────────

    use crate::diagnostics::LoweringBail;

    #[test]
    fn lower_test_let_rejects_impure_fn_call() {
        let source = r#"fn impure_fn() {
  > cmd
}
test "t" {
  let x = impure_fn()
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(matches!(result, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_test_let_accepts_pure_fn_call() {
        let source = r#"test "t" {
  let x = trim("hi")
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(result.is_ok());
        let test = result.unwrap();
        assert!(
            test.body()
                .iter()
                .any(|item| matches!(item, IrTestItem::Let { .. }))
        );
    }

    #[test]
    fn lower_test_let_accepts_string_literal() {
        let source = r#"test "t" {
  let x = "hello"
  shell sh {
    > cmd
  }
}
"#;
        let mut ctx = ctx_with_source(source);
        let result = lower_first_test(&mut ctx, "tests/a");
        assert!(result.is_ok());
    }
}
