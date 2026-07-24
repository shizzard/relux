//! Thin re-export: the reachability walk that transitively collects a test's
//! marker decisions now lives in `relux-ir` (it needs the decision table that
//! lowering populates, and 4c will run it per-test from `resolve()`). See
//! `relux_ir::reachability` for the walk itself.

pub use relux_ir::reachability::collect_test_decision;
