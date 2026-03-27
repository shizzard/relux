use crate::core::table::FileId;
use crate::diagnostics::InvalidReport;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::dsl::parser::ast::AstInterpolation;
use crate::dsl::parser::ast::AstStringPart;

/// Validate a static regex pattern (no interpolation variables).
/// If the pattern contains variables, skip validation (runtime-only).
pub(crate) fn validate_static_regex(
    interp: &AstInterpolation,
    file: &FileId,
) -> Result<(), LoweringBail> {
    // If any part is a variable reference or capture ref, skip validation
    let has_dynamic = interp.parts.iter().any(|p| {
        matches!(
            p,
            AstStringPart::VarRef { .. } | AstStringPart::CaptureRef { .. }
        )
    });
    if has_dynamic {
        return Ok(());
    }

    // Collect static pattern
    let pattern: String = interp
        .parts
        .iter()
        .map(|p| match p {
            AstStringPart::Literal { value, .. } => value.as_str(),
            AstStringPart::EscapedDollar { .. } => "$",
            _ => "",
        })
        .collect();

    if pattern.is_empty() {
        return Ok(());
    }

    if let Err(e) = regex::Regex::new(&pattern) {
        return Err(LoweringBail::invalid(InvalidReport::InvalidRegex {
            pattern,
            error: e.to_string(),
            span: IrSpan::new(file.clone(), interp.span),
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::diagnostics::InvalidReport;
    use crate::diagnostics::LoweringBail;
    use crate::dsl::resolver::lower::test_helpers::*;

    #[test]
    fn lower_valid_regex() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? hello\\s+world\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        assert!(ir.is_ok());
    }

    #[test]
    fn lower_invalid_regex_match() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_invalid_regex_fail() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  !? [unclosed\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_invalid_regex_timed_match() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <~5s? [unclosed\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        assert!(matches!(ir, Err(LoweringBail::Invalid(_))));
    }

    #[test]
    fn lower_invalid_regex_includes_pattern() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        if let Err(LoweringBail::Invalid(inner)) = &ir {
            if let InvalidReport::InvalidRegex { pattern, .. } = inner.as_ref() {
                assert!(pattern.contains("[unclosed"));
            } else {
                panic!("expected InvalidRegex, got {:?}", ir);
            }
        } else {
            panic!("expected InvalidRegex, got {:?}", ir);
        }
    }

    #[test]
    fn lower_invalid_regex_includes_error_message() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? [unclosed\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        if let Err(LoweringBail::Invalid(inner)) = &ir {
            if let InvalidReport::InvalidRegex { error, .. } = inner.as_ref() {
                assert!(!error.is_empty());
            } else {
                panic!("expected InvalidRegex, got {:?}", ir);
            }
        } else {
            panic!("expected InvalidRegex, got {:?}", ir);
        }
    }

    #[test]
    fn lower_regex_with_interpolation_not_validated() {
        let mut ctx = ctx_with_source("fn dummy() {}\n");
        push_test_scope(&mut ctx, "tests/a");
        let file = file_id_for(&ctx, "tests/a");
        let stmt = extract_first_stmt("fn t() {\n  <? ^${prefix}\n}\n");
        let ir = IrShellStmt::lower(&stmt, &file, &mut ctx);
        assert!(ir.is_ok());
    }
}
