use std::sync::Arc;

use crate::core::table::FileId;
use crate::diagnostics::Cause;
use crate::diagnostics::CauseId;
use crate::diagnostics::CauseTable;
use crate::diagnostics::DefinitionRef;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::diagnostics::ModulePath;
use crate::diagnostics::WarningId;
use crate::diagnostics::WarningTable;
use crate::dsl::parser::ast::AstItem;
use crate::dsl::parser::ast::AstTestDef;
use crate::dsl::parser::ast::AstTestItem;
use crate::pure::Env;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;
use super::tables::Tables;
use super::test_def::IrTest;
use super::timeout::IrTimeout;

// ─── TestMeta ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TestMeta {
    name: String,
    docstring: Option<String>,
    timeout: Option<IrTimeout>,
    flaky: bool,
    span: IrSpan,
}

impl TestMeta {
    pub fn new(
        name: impl Into<String>,
        docstring: Option<String>,
        timeout: Option<IrTimeout>,
        span: IrSpan,
    ) -> Self {
        Self {
            name: name.into(),
            docstring,
            timeout,
            flaky: false,
            span,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn docstring(&self) -> Option<&str> {
        self.docstring.as_deref()
    }

    pub fn timeout(&self) -> Option<&IrTimeout> {
        self.timeout.as_ref()
    }

    pub fn flaky(&self) -> bool {
        self.flaky
    }

    pub fn set_flaky(&mut self, flaky: bool) {
        self.flaky = flaky;
    }
}

impl_ir_node_struct!(TestMeta);

// ─── Plan ─────────────────────────────────────────────────

#[derive(Debug)]
pub enum Plan {
    Runnable {
        meta: TestMeta,
        test: IrTest,
        warnings: Vec<WarningId>,
    },
    Skipped {
        meta: TestMeta,
        causes: Vec<CauseId>,
        warnings: Vec<WarningId>,
    },
    Invalid {
        meta: TestMeta,
        causes: Vec<CauseId>,
        warnings: Vec<WarningId>,
    },
}

impl Plan {
    pub fn meta(&self) -> &TestMeta {
        match self {
            Plan::Runnable { meta, .. } => meta,
            Plan::Skipped { meta, .. } => meta,
            Plan::Invalid { meta, .. } => meta,
        }
    }
}

// ─── Suite ───────────────────────────────────────────────────

#[derive(Debug)]
pub struct Suite {
    pub plans: Vec<Plan>,
    pub env: Arc<Env>,
    pub causes: CauseTable,
    pub warnings: WarningTable,
    pub tables: Tables,
}

// ─── Plan Building ───────────────────────────────────────────

pub(crate) fn build_plan(
    def: &AstTestDef,
    module_path: &ModulePath,
    file_id: &FileId,
    ctx: &mut LoweringContext,
) -> Plan {
    // Extract TestMeta
    let docstring = def.body.iter().find_map(|item| {
        if let AstTestItem::DocString { text, .. } = &item.node {
            Some(text.clone())
        } else {
            None
        }
    });
    let timeout = def
        .timeout
        .as_ref()
        .map(|t| IrTimeout::lower(&t.node, file_id, ctx).unwrap());
    let mut meta = TestMeta::new(
        def.name.node.clone(),
        docstring,
        timeout,
        IrSpan::new(file_id.clone(), def.span),
    );

    // Create and populate local tables
    let mut tables = ctx.local_tables();
    if let Err(e) = ctx.populate_local_tables(module_path, file_id, &mut tables) {
        let bail = LoweringBail::invalid(e);
        let cause_id = bail.cause_id();
        ctx.register_cause(cause_id.clone(), Cause::from_bail(&bail));
        return Plan::Invalid {
            meta,
            causes: vec![cause_id],
            warnings: vec![],
        };
    }

    // Push scope
    use crate::dsl::resolver::lower::LoweringScope;
    ctx.push_scope(LoweringScope {
        module_path: module_path.clone(),
        tables,
    });

    // Evaluate markers
    let env = ctx.env().clone();
    let definition = DefinitionRef::Test {
        name: def.name.node.clone(),
        module: module_path.clone(),
    };
    match super::marker::eval_marker(&def.markers, definition, &env, file_id, ctx) {
        Ok(result) => {
            if let Some(skip) = result.skip {
                let cause_id = skip.cause_id();
                ctx.register_cause(cause_id.clone(), Cause::skip(skip));
                ctx.pop_scope();
                return Plan::Skipped {
                    meta,
                    causes: vec![cause_id],
                    warnings: vec![],
                };
            }
            meta.set_flaky(result.flaky);
        }
        Err(bail) => {
            let cause_id = bail.cause_id();
            ctx.register_cause(cause_id.clone(), Cause::from_bail(&bail));
            ctx.pop_scope();
            return Plan::Invalid {
                meta,
                causes: vec![cause_id],
                warnings: vec![],
            };
        }
    }

    // Set up shallow env for expect satisfiability checking
    let shallow = std::sync::Arc::new(crate::dsl::resolver::shallow_env::ShallowLayeredEnv::root(
        ctx.env(),
    ));
    ctx.set_shallow_env(shallow);

    // Lower test body
    let result = IrTest::lower(def, file_id, ctx);
    ctx.pop_scope();

    match result {
        Ok(ir_test) => Plan::Runnable {
            meta,
            test: ir_test,
            warnings: vec![],
        },
        Err(LoweringBail::Skip(skip)) => {
            let cause_id = skip.cause_id();
            ctx.register_cause(cause_id.clone(), Cause::Skip(skip));
            Plan::Skipped {
                meta,
                causes: vec![cause_id],
                warnings: vec![],
            }
        }
        Err(LoweringBail::Invalid(invalid)) => {
            let cause_id = invalid.cause_id();
            ctx.register_cause(cause_id.clone(), Cause::Invalid(invalid));
            Plan::Invalid {
                meta,
                causes: vec![cause_id],
                warnings: vec![],
            }
        }
    }
}

/// Build plans for all tests across all modules, sorted by module path.
pub fn build_all_plans(ctx: &mut LoweringContext) -> Vec<Plan> {
    // Collect (module_path, file_id, test_index) tuples
    let ast_table = ctx.ast_table().clone();
    let mut test_entries: Vec<(ModulePath, FileId, usize)> = Vec::new();

    for (module_path, (file_id, module)) in ast_table.as_vec() {
        for (idx, item) in module.items.iter().enumerate() {
            if matches!(&item.node, AstItem::Test { .. }) {
                test_entries.push((module_path.clone(), file_id.clone(), idx));
            }
        }
    }

    // Sort by module path, then by position within module
    test_entries.sort_by(|a, b| a.0.0.cmp(&b.0.0).then(a.2.cmp(&b.2)));

    let mut plans = Vec::new();
    for (module_path, file_id, idx) in test_entries {
        let entry = ast_table.get(&module_path).unwrap();
        let item = &entry.1.items[idx];
        if let AstItem::Test { def, .. } = &item.node {
            plans.push(build_plan(def, &module_path, &file_id, ctx));
        }
    }

    plans
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::table::FileId;
    use crate::dsl::resolver::ir::IrTestItem;
    use crate::dsl::resolver::lower::test_helpers::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::super::timeout::IrTimeout;

    fn test_file_id() -> FileId {
        FileId::new(PathBuf::from("test.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file_id(), crate::Span::new(0, 10))
    }

    #[test]
    fn plan_runnable_variant() {
        let s = test_span();
        let meta = TestMeta::new("test1", None, None, s.clone());
        let test = IrTest::new("test1", vec![], vec![], s);
        let plan = Plan::Runnable {
            meta,
            test,
            warnings: vec![],
        };
        assert!(matches!(plan, Plan::Runnable { .. }));
    }

    #[test]
    fn plan_runnable_with_warnings() {
        let s = test_span();
        let meta = TestMeta::new("test1", None, None, s.clone());
        let test = IrTest::new("test1", vec![], vec![], s);
        let w = WarningId {
            id: "test-warn-0001".into(),
        };
        let plan = Plan::Runnable {
            meta,
            test,
            warnings: vec![w],
        };
        if let Plan::Runnable { warnings, .. } = &plan {
            assert_eq!(warnings.len(), 1);
        }
    }

    #[test]
    fn plan_skipped_variant() {
        let meta = TestMeta::new("test1", None, None, test_span());
        let cause = CauseId::generate("test", "skip", 0, "skip");
        let plan = Plan::Skipped {
            meta,
            causes: vec![cause],
            warnings: vec![],
        };
        assert!(matches!(plan, Plan::Skipped { .. }));
    }

    #[test]
    fn plan_skipped_multiple_causes() {
        let meta = TestMeta::new("test1", None, None, test_span());
        let c1 = CauseId::generate("test", "a", 0, "skip");
        let c2 = CauseId::generate("test", "b", 1, "skip");
        let plan = Plan::Skipped {
            meta,
            causes: vec![c1, c2],
            warnings: vec![],
        };
        if let Plan::Skipped { causes, .. } = &plan {
            assert_eq!(causes.len(), 2);
        }
    }

    #[test]
    fn plan_invalid_variant() {
        let meta = TestMeta::new("test1", None, None, test_span());
        let cause = CauseId::generate("test", "err", 0, "invalid");
        let plan = Plan::Invalid {
            meta,
            causes: vec![cause],
            warnings: vec![],
        };
        assert!(matches!(plan, Plan::Invalid { .. }));
    }

    #[test]
    fn plan_invalid_multiple_causes() {
        let meta = TestMeta::new("test1", None, None, test_span());
        let c1 = CauseId::generate("test", "a", 0, "err1");
        let c2 = CauseId::generate("test", "b", 1, "err2");
        let plan = Plan::Invalid {
            meta,
            causes: vec![c1, c2],
            warnings: vec![],
        };
        if let Plan::Invalid { causes, .. } = &plan {
            assert_eq!(causes.len(), 2);
        }
    }

    #[test]
    fn test_meta_with_all_fields() {
        let s = test_span();
        let timeout = IrTimeout::Tolerance {
            duration: Duration::from_secs(5),
            multiplier: 1.0,
            span: s.clone(),
        };
        let meta = TestMeta::new("test1", Some("docs".into()), Some(timeout), s);
        assert_eq!(meta.name(), "test1");
        assert_eq!(meta.docstring(), Some("docs"));
        assert!(meta.timeout().is_some());
    }

    #[test]
    fn test_meta_minimal() {
        let meta = TestMeta::new("test1", None, None, test_span());
        assert_eq!(meta.name(), "test1");
        assert_eq!(meta.docstring(), None);
        assert!(meta.timeout().is_none());
    }

    // ─── Plan building: happy paths ────────────────────────────

    #[test]
    fn plan_simple_test() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "basic" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 1);
        assert!(is_runnable(&suite.plans[0]));
        assert_eq!(plan_name(&suite.plans[0]), "basic");
    }

    #[test]
    fn plan_test_with_fn_call() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"fn greet() {
  > echo hello
}

test "with fn" {
  shell sh {
    greet()
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn plan_test_with_pure_fn() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"pure fn greeting() {
  "hello"
}

test "with pure" {
  let g = greeting()
  shell sh {
    > echo ${g}
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn plan_test_with_bif() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "with bif" {
  let v = trim("  hello  ")
  shell sh {
    > echo ${v}
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn plan_test_with_effect() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"effect Setup {
  shell sh {
    > echo setup
  }
}

test "with effect" {
  start Setup
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn plan_test_with_docstring() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "documented" {
  """This test does things"""
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
        assert_eq!(
            suite.plans[0].meta().docstring(),
            Some("This test does things")
        );
    }

    #[test]
    fn plan_test_without_docstring() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "no doc" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
        assert_eq!(suite.plans[0].meta().docstring(), None);
    }

    #[test]
    fn plan_test_with_timeout() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "timed" ~10s {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
        assert!(suite.plans[0].meta().timeout().is_some());
    }

    #[test]
    fn plan_test_without_timeout() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "no timeout" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(suite.plans[0].meta().timeout().is_none());
    }

    #[test]
    fn plan_test_with_cleanup() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "with cleanup" {
  shell sh {
    > echo hello
  }
  cleanup {
    > echo bye
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
        if let Plan::Runnable { test, .. } = &suite.plans[0] {
            let has_cleanup = test
                .body()
                .iter()
                .any(|item| matches!(item, IrTestItem::Cleanup { .. }));
            assert!(has_cleanup);
        }
    }

    #[test]
    fn plan_test_with_let() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "with let" {
  let x = "hello"
  shell sh {
    > echo ${x}
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
    }

    #[test]
    fn plan_multiple_tests() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "first" {
  shell sh {
    > echo 1
  }
}

test "second" {
  shell sh {
    > echo 2
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 2);
        assert!(is_runnable(&suite.plans[0]));
        assert!(is_runnable(&suite.plans[1]));
        assert_eq!(plan_name(&suite.plans[0]), "first");
        assert_eq!(plan_name(&suite.plans[1]), "second");
    }

    #[test]
    fn plan_multiple_effects() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"effect Db {
  shell db_sh {
    > echo db setup
  }
}

effect Cache {
  shell cache_sh {
    > echo cache setup
  }
}

test "multi effects" {
  start Db
  start Cache
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_runnable(&suite.plans[0]));
        if let Plan::Runnable { test, .. } = &suite.plans[0] {
            assert_eq!(test.starts().len(), 2);
        }
    }

    // ─── Plan building: skip paths ─────────────────────────────

    #[test]
    fn plan_skip_unconditional() {
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
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn plan_skip_bare_condition() {
        let mut env = HashMap::new();
        env.insert("SKIP_ME".into(), "yes".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"# skip if SKIP_ME
test "skipped" {
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
    fn plan_skip_has_cause_id() {
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
        if let Plan::Skipped { causes, .. } = &suite.plans[0] {
            assert!(!causes.is_empty());
        } else {
            panic!("expected Skipped plan");
        }
    }

    #[test]
    fn plan_skip_fn_dep_propagates() {
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
        // Skipped fn dep → test is also skipped (propagation)
        assert!(is_skipped(&suite.plans[0]));
    }

    #[test]
    fn plan_skip_effect_dep_propagates() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
effect Setup {
  shell sh {
    > echo setup
  }
}

test "t" {
  start Setup
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        // Skipped effect dep → test is also skipped
        assert!(is_skipped(&suite.plans[0]));
    }

    // ─── Plan building: invalid paths ──────────────────────────

    #[test]
    fn plan_invalid_undefined_fn() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
        )]);
        assert!(is_invalid(&suite.plans[0]));
    }

    #[test]
    fn plan_invalid_undefined_effect() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t" {
  start NonExistent
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        assert!(is_invalid(&suite.plans[0]));
    }

    #[test]
    fn plan_invalid_fn_cycle() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"fn a() {
  b()
}

fn b() {
  a()
}

test "t" {
  shell sh {
    a()
  }
}
"#,
        )]);
        assert!(is_invalid(&suite.plans[0]));
    }

    #[test]
    fn plan_invalid_has_cause_id() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
        )]);
        if let Plan::Invalid { causes, .. } = &suite.plans[0] {
            assert!(!causes.is_empty());
        } else {
            panic!("expected Invalid plan");
        }
    }

    #[test]
    fn plan_invalid_purity_violation() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"pure fn bad() {
  > echo side-effect
}

test "t" {
  let v = bad()
  shell sh {
    > echo ${v}
  }
}
"#,
        )]);
        assert!(is_invalid(&suite.plans[0]));
    }

    // ─── Plan building: precedence ─────────────────────────────

    #[test]
    fn plan_own_skip_skips_body_lowering() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"# skip
test "t" {
  shell sh {
    nonexistent()
  }
}
"#,
        )]);
        assert!(is_skipped(&suite.plans[0]));
    }

    // ─── Suite assembly ────────────────────────────────────────

    #[test]
    fn suite_has_all_plans() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t1" {
  shell sh {
    > echo 1
  }
}

test "t2" {
  shell sh {
    > echo 2
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 2);
    }

    #[test]
    fn suite_has_source_table() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "t" {
  shell sh {
    > echo hello
  }
}
"#,
        )]);
        let has_entries = !suite.tables.sources.is_empty();
        assert!(has_entries);
    }

    #[test]
    fn suite_has_env() {
        let mut env = HashMap::new();
        env.insert("TEST_KEY".into(), "test_val".into());
        let suite = resolve_source(
            &[(
                "tests/a",
                r#"test "t" {
  shell sh {
    > echo hello
  }
}
"#,
            )],
            env,
        );
        assert_eq!(suite.env.get("TEST_KEY"), Some("test_val"));
    }

    #[test]
    fn suite_empty() {
        let suite = resolve_source_no_env(&[(
            "lib/helpers",
            r#"fn greet() {
  > echo hello
}
"#,
        )]);
        assert!(suite.plans.is_empty());
    }

    #[test]
    fn suite_mixed_variants() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "good" {
  shell sh {
    > echo hello
  }
}

# skip
test "skipped" {
  shell sh {
    > echo skip
  }
}

test "bad" {
  shell sh {
    nonexistent()
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 3);
        let good = suite.plans.iter().find(|p| plan_name(p) == "good").unwrap();
        let skipped = suite
            .plans
            .iter()
            .find(|p| plan_name(p) == "skipped")
            .unwrap();
        let bad = suite.plans.iter().find(|p| plan_name(p) == "bad").unwrap();
        assert!(is_runnable(good));
        assert!(is_skipped(skipped));
        assert!(is_invalid(bad));
    }

    // ─── Effect deduplication ──────────────────────────────────

    #[test]
    fn effect_start_no_overlay_same_as_empty_overlay() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"effect Db {
  shell db_sh {
    > echo db
  }
}

test "t1" {
  start Db
  shell sh {
    > echo 1
  }
}

test "t2" {
  start Db {}
  shell sh {
    > echo 2
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 2);
        assert!(suite.plans.iter().all(is_runnable));
    }

    // ─── build_all_plans ordering ──────────────────────────────

    #[test]
    fn build_all_plans_tests_within_module_in_order() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"test "first" {
  shell sh {
    > echo 1
  }
}

test "second" {
  shell sh {
    > echo 2
  }
}

test "third" {
  shell sh {
    > echo 3
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 3);
        assert_eq!(plan_name(&suite.plans[0]), "first");
        assert_eq!(plan_name(&suite.plans[1]), "second");
        assert_eq!(plan_name(&suite.plans[2]), "third");
    }

    // ─── Purity enforcement (end-to-end plan building) ───────

    #[test]
    fn plan_test_let_impure_fn_invalidates() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"fn impure_fn() {
  > cmd
}
test "t" {
  let x = impure_fn()
  shell sh {
    > cmd
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 1);
        assert!(is_invalid(&suite.plans[0]));
    }

    #[test]
    fn plan_effect_let_impure_fn_invalidates() {
        let suite = resolve_source_no_env(&[(
            "tests/a",
            r#"fn impure_fn() {
  > cmd
}
effect E {
  let x = impure_fn()
  shell sh {
    > start
  }
}
test "t" {
  start E
  shell sh {
    > cmd
  }
}
"#,
        )]);
        assert_eq!(suite.plans.len(), 1);
        assert!(is_invalid(&suite.plans[0]));
    }
}
