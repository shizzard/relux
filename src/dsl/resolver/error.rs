use crate::dsl::parser;
use crate::error::{DiagnosticReport, ReportLabel, Severity};

use super::ir::{FileId, Span};

// ─── Warnings ──────────────────────────────────────────────

#[derive(Debug)]
pub enum DiagnosticWarning {
    DuplicateDefinition {
        name: String,
        arity: Option<usize>,
        first: Span,
        second: Span,
    },
    ImportNotExported {
        name: String,
        module_path: String,
        span: Span,
    },
}

impl DiagnosticWarning {
    pub fn name(&self) -> &'static str {
        match self {
            DiagnosticWarning::DuplicateDefinition { .. } => "DuplicateDefinition",
            DiagnosticWarning::ImportNotExported { .. } => "ImportNotExported",
        }
    }
}

impl From<&DiagnosticWarning> for DiagnosticReport {
    fn from(diag: &DiagnosticWarning) -> Self {
        match diag {
            DiagnosticWarning::DuplicateDefinition {
                name,
                arity,
                first,
                second,
            } => {
                let message = match arity {
                    Some(a) => format!("duplicate definition of `{name}/{a}`"),
                    None => format!("duplicate definition of `{name}`"),
                };
                DiagnosticReport {
                    severity: Severity::Warning,
                    message,
                    labels: vec![
                        (second.clone(), "redefined here").into(),
                        (first.clone(), "first defined here").into(),
                    ],
                    help: None,
                    note: None,
                }
            }
            DiagnosticWarning::ImportNotExported {
                name,
                module_path,
                span,
            } => DiagnosticReport {
                severity: Severity::Warning,
                message: format!("`{name}` is not exported"),
                labels: vec![(span.clone(), "not found in module").into()],
                help: None,
                note: Some(format!(
                    "the module `{module_path}` does not export `{name}`"
                )),
            },
        }
    }
}

// ─── Errors ────────────────────────────────────────────────

#[derive(Debug)]
pub enum DiagnosticError {
    Parse {
        file: FileId,
        error: parser::ParseError,
    },
    ModuleNotFound {
        path: String,
        referenced_from: Span,
    },
    RootNotFound {
        path: String,
    },
    CircularImport {
        cycle: Vec<(String, Option<Span>)>,
    },
    UndefinedName {
        name: String,
        span: Span,
        available_arities: Vec<usize>,
    },
    DuplicateDefinition {
        name: String,
        arity: Option<usize>,
        first: Span,
        second: Span,
    },
    UndefinedVariable {
        name: String,
        span: Span,
    },
    CircularEffectDependency {
        cycle: Vec<(String, Span)>,
    },
    InvalidTimeout {
        raw: String,
        span: Span,
    },
    InvalidRegex {
        pattern: String,
        message: String,
        span: Span,
    },
    ImpureInPureContext {
        what: String,
        span: Span,
    },
    InvalidCleanupStatement {
        span: Span,
    },
    ImportNotExported {
        name: String,
        module_path: String,
        span: Span,
    },
}

impl DiagnosticError {
    pub fn name(&self) -> &'static str {
        match self {
            DiagnosticError::Parse { .. } => "Parse",
            DiagnosticError::ModuleNotFound { .. } => "ModuleNotFound",
            DiagnosticError::RootNotFound { .. } => "RootNotFound",
            DiagnosticError::CircularImport { .. } => "CircularImport",
            DiagnosticError::UndefinedName { .. } => "UndefinedName",
            DiagnosticError::DuplicateDefinition { .. } => "DuplicateDefinition",
            DiagnosticError::UndefinedVariable { .. } => "UndefinedVariable",
            DiagnosticError::CircularEffectDependency { .. } => "CircularEffectDependency",
            DiagnosticError::InvalidTimeout { .. } => "InvalidTimeout",
            DiagnosticError::InvalidRegex { .. } => "InvalidRegex",
            DiagnosticError::ImpureInPureContext { .. } => "ImpureInPureContext",
            DiagnosticError::InvalidCleanupStatement { .. } => "InvalidCleanupStatement",
            DiagnosticError::ImportNotExported { .. } => "ImportNotExported",
        }
    }
}

impl From<&DiagnosticError> for DiagnosticReport {
    fn from(diag: &DiagnosticError) -> Self {
        match diag {
            DiagnosticError::Parse { file, error } => {
                let err_span = error.span();
                let span = Span::new(*file, err_span.start..err_span.end);
                match error {
                    parser::ParseError::Syntax(syntax_err) => {
                        let expected: Vec<String> =
                            syntax_err.expected().map(|e| format!("{e}")).collect();
                        let note = if expected.is_empty() {
                            None
                        } else {
                            Some(format!("expected {}", expected.join(", ")))
                        };
                        DiagnosticReport {
                            severity: Severity::Error,
                            message: "unexpected token".into(),
                            labels: vec![(span, "this token is unexpected").into()],
                            help: None,
                            note,
                        }
                    }
                    parser::ParseError::InvalidEscape { sequence, .. } => DiagnosticReport {
                        severity: Severity::Error,
                        message: format!("unknown escape sequence `{sequence}`"),
                        labels: vec![(span, "invalid escape").into()],
                        help: Some("supported escapes: \\n, \\t, \\r, \\\\, \\\", \\0, \\a, \\b, \\f, \\v, \\e".into()),
                        note: None,
                    },
                    parser::ParseError::OrphanMarker { .. } => DiagnosticReport {
                        severity: Severity::Error,
                        message: "orphan marker not attached to any test or effect".into(),
                        labels: vec![(span, "this marker has no target").into()],
                        help: Some("markers must appear directly before a test or effect definition".into()),
                        note: None,
                    },
                    parser::ParseError::Multiple(msg) => DiagnosticReport {
                        severity: Severity::Error,
                        message: msg.clone(),
                        labels: vec![(span, "parse errors").into()],
                        help: None,
                        note: None,
                    },
                }
            }
            DiagnosticError::ModuleNotFound {
                path: mod_path,
                referenced_from,
            } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("module not found: {mod_path}"),
                labels: vec![(referenced_from.clone(), "imported here").into()],
                help: None,
                note: None,
            },
            DiagnosticError::RootNotFound { path } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("test file not found: {path}"),
                labels: vec![],
                help: None,
                note: None,
            },
            DiagnosticError::CircularImport { cycle } => {
                let cycle_names: Vec<&str> = cycle.iter().map(|(name, _)| name.as_str()).collect();
                let labels: Vec<ReportLabel> = cycle
                    .iter()
                    .enumerate()
                    .take(cycle.len().saturating_sub(1))
                    .filter_map(|(i, (name, span))| {
                        span.as_ref().map(|s| {
                            let importer = if i > 0 {
                                cycle[i - 1].0.as_str()
                            } else {
                                cycle
                                    .get(cycle.len().saturating_sub(2))
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("?")
                            };
                            (s.clone(), format!("{importer} imports {name}")).into()
                        })
                    })
                    .collect();
                DiagnosticReport {
                    severity: Severity::Error,
                    message: "circular import detected".into(),
                    labels,
                    help: None,
                    note: Some(format!("cycle: {}", cycle_names.join(" -> "))),
                }
            }
            DiagnosticError::UndefinedName {
                name,
                span,
                available_arities,
            } => {
                let help = if available_arities.is_empty() {
                    None
                } else {
                    let alts: Vec<String> =
                        available_arities.iter().map(|a| a.to_string()).collect();
                    let base = name.split('/').next().unwrap_or(name);
                    Some(format!(
                        "`{base}` exists with {} {}",
                        if alts.len() == 1 { "arity" } else { "arities" },
                        alts.join(", ")
                    ))
                };
                DiagnosticReport {
                    severity: Severity::Error,
                    message: format!("undefined name `{name}`"),
                    labels: vec![(span.clone(), "not found").into()],
                    help,
                    note: None,
                }
            }
            DiagnosticError::DuplicateDefinition {
                name,
                arity,
                first,
                second,
            } => {
                let message = match arity {
                    Some(a) => format!("duplicate definition of `{name}/{a}`"),
                    None => format!("duplicate definition of `{name}`"),
                };
                DiagnosticReport {
                    severity: Severity::Error,
                    message,
                    labels: vec![
                        (second.clone(), "redefined here").into(),
                        (first.clone(), "first defined here").into(),
                    ],
                    help: None,
                    note: None,
                }
            }
            DiagnosticError::UndefinedVariable { name, span } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("undefined variable `{name}`"),
                labels: vec![(span.clone(), "not found in scope").into()],
                help: None,
                note: None,
            },
            DiagnosticError::CircularEffectDependency { cycle } => {
                let mut cycle_names: Vec<&str> =
                    cycle.iter().map(|(name, _)| name.as_str()).collect();
                if let Some(first) = cycle.first() {
                    cycle_names.push(first.0.as_str());
                }
                let labels: Vec<ReportLabel> = cycle
                    .iter()
                    .enumerate()
                    .map(|(i, (name, span))| {
                        let next_name =
                            cycle
                                .get(i + 1)
                                .map(|(n, _)| n.as_str())
                                .unwrap_or_else(|| {
                                    cycle.first().map(|(n, _)| n.as_str()).unwrap_or("?")
                                });
                        (span.clone(), format!("{name} needs {next_name}")).into()
                    })
                    .collect();
                DiagnosticReport {
                    severity: Severity::Error,
                    message: "circular effect dependency".into(),
                    labels,
                    help: None,
                    note: Some(format!("cycle: {}", cycle_names.join(" -> "))),
                }
            }
            DiagnosticError::InvalidTimeout { raw, span } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("invalid timeout `{raw}`"),
                labels: vec![(span.clone(), "invalid duration").into()],
                help: Some(
                    "expected compact humantime format like `~10s`, `~500ms`, or `~2h30m`".into(),
                ),
                note: None,
            },
            DiagnosticError::InvalidRegex {
                pattern: _,
                message,
                span,
            } => DiagnosticReport {
                severity: Severity::Error,
                message: "invalid regex in condition marker".into(),
                labels: vec![(span.clone(), message.as_str()).into()],
                help: None,
                note: None,
            },
            DiagnosticError::ImpureInPureContext { what, span } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("{what} cannot be used in a pure function"),
                labels: vec![(span.clone(), "not allowed in pure context").into()],
                help: None,
                note: None,
            },
            DiagnosticError::InvalidCleanupStatement { span } => DiagnosticReport {
                severity: Severity::Error,
                message: "statement not allowed in cleanup block".into(),
                labels: vec![
                    (
                        span.clone(),
                        "only send, let, and assign are allowed in cleanup blocks",
                    )
                        .into(),
                ],
                help: None,
                note: None,
            },
            DiagnosticError::ImportNotExported {
                name,
                module_path,
                span,
            } => DiagnosticReport {
                severity: Severity::Error,
                message: format!("`{name}` is not exported by `{module_path}`"),
                labels: vec![(span.clone(), "not exported").into()],
                help: None,
                note: None,
            },
        }
    }
}
