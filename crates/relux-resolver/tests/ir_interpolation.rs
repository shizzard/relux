// Tests extracted from relux-ir/src/interpolation.rs
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

fn test_file_id() -> FileId {
    FileId::new(PathBuf::from("test.relux"))
}

fn test_span() -> IrSpan {
    IrSpan::new(test_file_id(), relux_core::Span::new(0, 10))
}

#[test]
fn ir_interpolation_empty_parts() {
    let interp = IrInterpolation::new(vec![], test_span());
    assert!(interp.parts().is_empty());
}

#[test]
fn ir_interpolation_single_literal() {
    let s = test_span();
    let interp = IrInterpolation::new(
        vec![IrStringPart::Literal {
            value: "hello".into(),
            span: s.clone(),
        }],
        s,
    );
    assert_eq!(interp.parts().len(), 1);
}

#[test]
fn ir_interpolation_mixed_parts() {
    let s = test_span();
    let parts = vec![
        IrStringPart::Literal {
            value: "a".into(),
            span: s.clone(),
        },
        IrStringPart::Var {
            name: "x".into(),
            span: s.clone(),
        },
        IrStringPart::CaptureRef {
            index: 1,
            span: s.clone(),
        },
        IrStringPart::EscapedDollar { span: s.clone() },
    ];
    let interp = IrInterpolation::new(parts, s);
    assert_eq!(interp.parts().len(), 4);
}

#[test]
fn ir_string_part_all_variants() {
    let s = test_span();
    let _ = IrStringPart::Literal {
        value: "x".into(),
        span: s.clone(),
    };
    let _ = IrStringPart::Var {
        name: "v".into(),
        span: s.clone(),
    };
    let _ = IrStringPart::CaptureRef {
        index: 0,
        span: s.clone(),
    };
    let _ = IrStringPart::EscapedDollar { span: s };
}

// ─── Lowering tests (moved from lower.rs) ───────────────

use relux_ast::AstInterpolation;
use relux_ast::AstStringPart;

#[test]
fn lower_string_part_literal() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstStringPart::Literal {
        value: "hello".into(),
        span: Span::new(0, 5),
    };
    let ir = IrStringPart::lower(&ast, &file, &mut ctx).unwrap();
    if let IrStringPart::Literal { value, .. } = &ir {
        assert_eq!(value, "hello");
    } else {
        panic!("expected Literal");
    }
}

#[test]
fn lower_string_part_var() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstStringPart::VarRef {
        name: "name".into(),
        span: Span::new(0, 7),
    };
    let ir = IrStringPart::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrStringPart::Var { name, .. } if name == "name"));
}

#[test]
fn lower_string_part_capture_ref() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstStringPart::CaptureRef {
        index: 1,
        span: Span::new(0, 4),
    };
    let ir = IrStringPart::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrStringPart::CaptureRef { index: 1, .. }));
}

#[test]
fn lower_string_part_capture_ref_zero() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstStringPart::CaptureRef {
        index: 0,
        span: Span::new(0, 4),
    };
    let ir = IrStringPart::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrStringPart::CaptureRef { index: 0, .. }));
}

#[test]
fn lower_string_part_escaped_dollar() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstStringPart::EscapedDollar {
        span: Span::new(0, 2),
    };
    let ir = IrStringPart::lower(&ast, &file, &mut ctx).unwrap();
    assert!(matches!(ir, IrStringPart::EscapedDollar { .. }));
}

#[test]
fn lower_interpolation_single_part() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstInterpolation {
        parts: vec![AstStringPart::Literal {
            value: "hello".into(),
            span: Span::new(1, 6),
        }],
        span: Span::new(0, 7),
    };
    let ir = IrInterpolation::lower(&ast, &file, &mut ctx).unwrap();
    assert_eq!(ir.parts().len(), 1);
}

#[test]
fn lower_interpolation_mixed() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstInterpolation {
        parts: vec![
            AstStringPart::Literal {
                value: "hi ".into(),
                span: Span::new(1, 4),
            },
            AstStringPart::VarRef {
                name: "name".into(),
                span: Span::new(4, 11),
            },
            AstStringPart::CaptureRef {
                index: 1,
                span: Span::new(11, 15),
            },
        ],
        span: Span::new(0, 16),
    };
    let ir = IrInterpolation::lower(&ast, &file, &mut ctx).unwrap();
    assert_eq!(ir.parts().len(), 3);
    assert!(matches!(&ir.parts()[0], IrStringPart::Literal { .. }));
    assert!(matches!(&ir.parts()[1], IrStringPart::Var { .. }));
    assert!(matches!(&ir.parts()[2], IrStringPart::CaptureRef { .. }));
}

#[test]
fn lower_interpolation_empty() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstInterpolation {
        parts: vec![],
        span: Span::new(0, 2),
    };
    let ir = IrInterpolation::lower(&ast, &file, &mut ctx).unwrap();
    assert!(ir.parts().is_empty());
}

#[test]
fn lower_interpolation_adjacent_vars() {
    let file = test_file_id();
    let mut ctx = ctx_with_source("fn dummy() {}\n");
    let ast = AstInterpolation {
        parts: vec![
            AstStringPart::VarRef {
                name: "a".into(),
                span: Span::new(1, 5),
            },
            AstStringPart::VarRef {
                name: "b".into(),
                span: Span::new(5, 9),
            },
        ],
        span: Span::new(0, 10),
    };
    let ir = IrInterpolation::lower(&ast, &file, &mut ctx).unwrap();
    assert_eq!(ir.parts().len(), 2);
    assert!(matches!(&ir.parts()[0], IrStringPart::Var { name, .. } if name == "a"));
    assert!(matches!(&ir.parts()[1], IrStringPart::Var { name, .. } if name == "b"));
}
