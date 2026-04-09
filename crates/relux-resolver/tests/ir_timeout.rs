// Tests extracted from relux-ir/src/timeout.rs
#![allow(unused_imports)]
use relux_ast::*;
use relux_core::Span;
use relux_core::Spanned;
use relux_core::diagnostics::*;
use relux_core::pure::*;
use relux_core::table::FileId;
use relux_core::table::SharedTable;
use relux_core::table::SourceTable;
use relux_ir::evaluator::*;
use relux_ir::lowering_context::*;
use relux_ir::marker::*;
use relux_ir::regex_validate::*;
use relux_ir::shallow_env::*;
use relux_ir::*;
use relux_resolver::lower::test_helpers::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use std::time::Duration;

#[test]
fn tolerance_default_multiplier() {
    let t = IrTimeout::tolerance(Duration::from_secs(5));
    assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
}

#[test]
fn tolerance_apply_multiplier() {
    let mut t = IrTimeout::tolerance(Duration::from_secs(5));
    t.apply_multiplier(2.0);
    assert_eq!(t.adjusted_duration(), Duration::from_secs(10));
}

#[test]
fn tolerance_apply_multiplier_fractional() {
    let mut t = IrTimeout::tolerance(Duration::from_secs(5));
    t.apply_multiplier(0.5);
    assert_eq!(t.adjusted_duration(), Duration::from_millis(2500));
}

#[test]
fn tolerance_apply_multiplier_zero() {
    let mut t = IrTimeout::tolerance(Duration::from_secs(5));
    t.apply_multiplier(0.0);
    assert_eq!(t.adjusted_duration(), Duration::ZERO);
}

#[test]
fn assertion_ignores_multiplier() {
    let mut t = IrTimeout::Assertion {
        duration: Duration::from_secs(5),
        span: IrSpan::synthetic(),
    };
    t.apply_multiplier(2.0);
    assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
}

#[test]
fn tolerance_raw_duration() {
    let mut t = IrTimeout::tolerance(Duration::from_secs(5));
    t.apply_multiplier(3.0);
    assert_eq!(t.raw_duration(), Duration::from_secs(5));
    assert_eq!(t.adjusted_duration(), Duration::from_secs(15));
}

#[test]
fn tolerance_constructor_synthetic_span() {
    let t = IrTimeout::tolerance(Duration::from_secs(1));
    assert_eq!(t.span().file().path().to_str().unwrap(), "<synthetic>");
}

#[test]
fn assertion_adjusted_is_raw() {
    let t = IrTimeout::Assertion {
        duration: Duration::from_secs(3),
        span: IrSpan::synthetic(),
    };
    assert_eq!(t.adjusted_duration(), Duration::from_secs(3));
    assert_eq!(t.raw_duration(), Duration::from_secs(3));
    assert_eq!(t.adjusted_duration(), t.raw_duration());
}

#[test]
fn tolerance_scaled_applies_multiplier() {
    let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 2.0);
    assert_eq!(t.raw_duration(), Duration::from_secs(5));
    assert_eq!(t.adjusted_duration(), Duration::from_secs(10));
}

#[test]
fn tolerance_scaled_with_unit_multiplier() {
    let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 1.0);
    assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
}

#[test]
fn tolerance_scaled_fractional() {
    let t = IrTimeout::tolerance_scaled(Duration::from_secs(10), 0.5);
    assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
}

#[test]
fn tolerance_flaky_multiplier() {
    let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 2.0);
    // base=5s, static_m=2.0, flaky_m=1.5 → 5*2.0*1.5 = 15s
    assert_eq!(t.adjusted_duration_with_flaky(1.5), Duration::from_secs(15));
}

#[test]
fn assertion_ignores_flaky_multiplier() {
    let t = IrTimeout::Assertion {
        duration: Duration::from_secs(5),
        span: IrSpan::synthetic(),
    };
    assert_eq!(t.adjusted_duration_with_flaky(2.0), Duration::from_secs(5));
}

#[test]
fn tolerance_flaky_multiplier_unit() {
    let t = IrTimeout::tolerance_scaled(Duration::from_secs(10), 1.0);
    assert_eq!(t.adjusted_duration_with_flaky(1.0), Duration::from_secs(10));
}
