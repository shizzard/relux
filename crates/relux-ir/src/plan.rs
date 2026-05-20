use std::sync::Arc;

use relux_ast::AstItem;
use relux_ast::AstTestDef;
use relux_ast::AstTestItem;
use relux_core::diagnostics::Cause;
use relux_core::diagnostics::CauseId;
use relux_core::diagnostics::CauseTable;
use relux_core::diagnostics::DefinitionRef;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::diagnostics::ModulePath;
use relux_core::diagnostics::WarningId;
use relux_core::diagnostics::WarningTable;
use relux_core::pure::LayeredEnv;
use relux_core::table::FileId;

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
    definition: DefinitionRef,
    span: IrSpan,
}

impl TestMeta {
    pub fn new(
        name: impl Into<String>,
        docstring: Option<String>,
        timeout: Option<IrTimeout>,
        definition: DefinitionRef,
        span: IrSpan,
    ) -> Self {
        Self {
            name: name.into(),
            docstring,
            timeout,
            flaky: false,
            definition,
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

    pub fn definition(&self) -> &DefinitionRef {
        &self.definition
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
    pub env: Arc<LayeredEnv>,
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
    let definition = DefinitionRef::Test {
        name: def.name.node.clone(),
        module: module_path.clone(),
    };
    let mut meta = TestMeta::new(
        def.name.node.clone(),
        docstring,
        timeout,
        definition.clone(),
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
    use crate::lowering_context::LoweringScope;
    ctx.push_scope(LoweringScope {
        module_path: module_path.clone(),
        tables,
    });

    // Evaluate markers
    let env = ctx.env().clone();
    match super::marker::eval_marker(&def.markers, definition.clone(), &env, file_id, ctx) {
        Ok(result) => {
            ctx.tables()
                .marker_recordings
                .insert(definition.clone(), result.recordings);
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
    let shallow = std::sync::Arc::new(crate::shallow_env::ShallowLayeredEnv::root(ctx.env()));
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
