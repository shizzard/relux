//! Thin re-export: the reachability walk that transitively collects a test's
//! marker decisions lives in `relux-ir` (it shares the decision table that the
//! resolver's per-test decision pass populates). The runtime re-walks it at
//! replay time to recover a test's marker recordings. See
//! `relux_ir::reachability` for the walk itself.

pub use relux_ir::reachability::collect_test_decision;
