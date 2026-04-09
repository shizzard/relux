use relux_ast::AstInterpolation;
use relux_ast::AstStringPart;
use relux_core::diagnostics::InvalidReport;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

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
        return Err(LoweringBail::invalid(InvalidReport::invalid_regex(
            pattern,
            e.to_string(),
            IrSpan::new(file.clone(), interp.span),
        )));
    }

    Ok(())
}
