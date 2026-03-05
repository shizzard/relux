use ariadne::{Config, IndexType, Label, Report, ReportKind, sources};

use crate::dsl::resolver::Diagnostic;
use crate::dsl::resolver::ir::{FileId, SourceMap, Span};
use crate::runtime::result::Failure;

type Src = String;

fn cfg() -> Config {
    Config::default().with_index_type(IndexType::Byte)
}

fn file_path(file: FileId, source_map: &SourceMap) -> Src {
    source_map
        .files
        .get(file)
        .map(|f| f.path.display().to_string())
        .unwrap_or_else(|| "<unknown>".into())
}

fn aspan(span: &Span, source_map: &SourceMap) -> (Src, std::ops::Range<usize>) {
    let path = file_path(span.file, source_map);
    let range = &span.range;
    if range.start < range.end {
        return (path, range.clone());
    }
    // Zero-width span: expand to the surrounding non-whitespace token on the same line
    if let Some(file) = source_map.files.get(span.file) {
        let src = file.source.as_bytes();
        let pos = range.start.min(src.len());
        let line_start = src[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let token_start = src[line_start..pos]
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .map_or(pos, |i| line_start + i);
        if token_start < pos {
            return (path, token_start..pos);
        }
    }
    (path, range.clone())
}

fn make_cache(source_map: &SourceMap) -> impl ariadne::Cache<Src> + '_ {
    let srcs: Vec<(Src, &str)> = source_map
        .files
        .iter()
        .map(|f| (f.path.display().to_string(), f.source.as_str()))
        .collect();
    sources(srcs)
}

pub fn print_diagnostics(diagnostics: &[Diagnostic], source_map: &SourceMap) {
    for diag in diagnostics {
        let mut cache = make_cache(source_map);
        render_diagnostic(diag, source_map, &mut cache);
    }
}

pub fn print_failure(failure: &Failure, source_map: &SourceMap) {
    let mut cache = make_cache(source_map);
    render_failure(failure, source_map, &mut cache);
}

fn render_diagnostic(
    diag: &Diagnostic,
    source_map: &SourceMap,
    cache: &mut impl ariadne::Cache<Src>,
) {
    let cfg = cfg();
    match diag {
        Diagnostic::Parse { file, error } => {
            let path = file_path(*file, source_map);
            let err_span = error.span();
            let span = (path.clone(), err_span.start..err_span.end);

            let mut builder = Report::build(ReportKind::Error, span.clone())
                .with_config(cfg)
                .with_message("unexpected token")
                .with_label(Label::new(span).with_message("this token is unexpected"));

            let expected: Vec<String> = error.expected().map(|e| format!("{e}")).collect();
            if !expected.is_empty() {
                builder = builder.with_note(format!("expected {}", expected.join(", ")));
            }

            let _ = builder.finish().eprint(cache);
        }

        Diagnostic::ModuleNotFound {
            path: mod_path,
            referenced_from,
        } => {
            let (path, range) = aspan(referenced_from, source_map);
            let span = (path, range);
            let _ = Report::build(ReportKind::Error, span.clone())
                .with_config(cfg)
                .with_message(format!("module not found: {mod_path}"))
                .with_label(Label::new(span).with_message("imported here"))
                .finish()
                .eprint(cache);
        }

        Diagnostic::CircularImport { cycle } => {
            eprintln!("Error: circular import detected");
            eprintln!("  = note: cycle: {}", cycle.join(" -> "));
        }

        Diagnostic::UndefinedName {
            name,
            span,
            available_arities,
        } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let mut builder = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("undefined name `{name}`"))
                .with_label(Label::new(s).with_message("not found"));

            if !available_arities.is_empty() {
                let alts: Vec<String> = available_arities.iter().map(|a| a.to_string()).collect();
                let base = name.split('/').next().unwrap_or(name);
                builder = builder.with_help(format!(
                    "`{base}` exists with {} {}",
                    if alts.len() == 1 { "arity" } else { "arities" },
                    alts.join(", ")
                ));
            }

            let _ = builder.finish().eprint(cache);
        }

        Diagnostic::DuplicateDefinition {
            name,
            arity,
            first,
            second,
        } => {
            let label = match arity {
                Some(a) => format!("duplicate definition of `{name}/{a}`"),
                None => format!("duplicate definition of `{name}`"),
            };
            let (path1, range1) = aspan(first, source_map);
            let (path2, range2) = aspan(second, source_map);
            let first_span = (path1, range1);
            let second_span = (path2, range2);

            let _ = Report::build(ReportKind::Error, second_span.clone())
                .with_config(cfg)
                .with_message(&label)
                .with_label(Label::new(second_span).with_message("redefined here"))
                .with_label(Label::new(first_span).with_message("first defined here"))
                .finish()
                .eprint(cache);
        }

        Diagnostic::UndefinedVariable { name, span } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("undefined variable `{name}`"))
                .with_label(Label::new(s).with_message("not found in scope"))
                .finish()
                .eprint(cache);
        }

        Diagnostic::CircularEffectDependency { cycle } => {
            eprintln!("Error: circular effect dependency");
            eprintln!("  = note: cycle: {}", cycle.join(" -> "));
        }

        Diagnostic::InvalidTimeout { raw, span } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("invalid timeout `{raw}`"))
                .with_label(Label::new(s).with_message("invalid duration"))
                .with_help("expected humantime format like `~10s`, `~500ms`, or `~2h 30m`")
                .finish()
                .eprint(cache);
        }

        Diagnostic::ImportNotExported {
            name,
            module_path,
            span,
        } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("`{name}` is not exported"))
                .with_label(Label::new(s).with_message("not found in module"))
                .with_note(format!(
                    "the module `{module_path}` does not export `{name}`"
                ))
                .finish()
                .eprint(cache);
        }
    }
}

fn render_failure(failure: &Failure, source_map: &SourceMap, cache: &mut impl ariadne::Cache<Src>) {
    let cfg = cfg();

    match failure {
        Failure::MatchTimeout {
            pattern,
            span,
            shell,
        } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("match timeout in shell `{shell}`"))
                .with_label(
                    Label::new(s).with_message(format!("timed out waiting for `{pattern}`")),
                )
                .finish()
                .eprint(cache);
        }

        Failure::FailPatternMatched {
            pattern,
            matched_line,
            span,
            shell,
        } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("fail pattern matched in shell `{shell}`"))
                .with_label(
                    Label::new(s).with_message(format!("pattern `{pattern}` triggered here")),
                )
                .with_note(format!("matched output: {matched_line}"))
                .finish()
                .eprint(cache);
        }

        Failure::ShellExited {
            shell,
            exit_code,
            span,
        } => {
            let (path, range) = aspan(span, source_map);
            let s = (path, range);
            let code_msg = match exit_code {
                Some(c) => format!("with exit code {c}"),
                None => "without an exit code".to_string(),
            };
            let _ = Report::build(ReportKind::Error, s.clone())
                .with_config(cfg)
                .with_message(format!("shell `{shell}` exited unexpectedly"))
                .with_label(Label::new(s).with_message(code_msg))
                .finish()
                .eprint(cache);
        }

        Failure::Runtime {
            message,
            span,
            shell,
        } => {
            let msg = match shell {
                Some(s) => format!("runtime error in shell `{s}`"),
                None => "runtime error".to_string(),
            };
            let first_line = message.lines().next().unwrap_or(message);
            let has_detail = message.contains('\n');
            if let Some(span) = span {
                let (path, range) = aspan(span, source_map);
                let s = (path, range);
                let mut builder = Report::build(ReportKind::Error, s.clone())
                    .with_config(cfg)
                    .with_message(&msg)
                    .with_label(Label::new(s).with_message(first_line));
                if has_detail {
                    builder = builder.with_note(message);
                }
                let _ = builder.finish().eprint(cache);
            } else {
                eprintln!("Error: {msg}: {first_line}");
                if has_detail {
                    eprintln!("{message}");
                }
            }
        }
    }
}
