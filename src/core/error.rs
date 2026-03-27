use std::path::Path;

use ariadne::CharSet;
use ariadne::Color;
use ariadne::Config;
use ariadne::IndexType;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use ariadne::sources;

use crate::diagnostics::IrSpan;
use crate::dsl::resolver::ir::SourceTable;

// ─── Severity ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

// ─── Diagnostic Report ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReportLabel {
    pub span: IrSpan,
    pub message: String,
}

impl From<(IrSpan, String)> for ReportLabel {
    fn from((span, message): (IrSpan, String)) -> Self {
        Self { span, message }
    }
}

impl From<(IrSpan, &str)> for ReportLabel {
    fn from((span, message): (IrSpan, &str)) -> Self {
        Self {
            span,
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<ReportLabel>,
    pub help: Option<String>,
    pub note: Option<String>,
}

// ─── Diagnostic Reports (batch with source table) ───────────

pub struct DiagnosticReports {
    pub errors: Vec<DiagnosticReport>,
    pub warnings: Vec<DiagnosticReport>,
    pub source_table: SourceTable,
    pub project_root: Option<std::path::PathBuf>,
}

impl DiagnosticReports {
    pub fn eprint(&self) {
        let root = self.project_root.as_deref();
        let mut cache = make_cache(&self.source_table, root);
        for warning in &self.warnings {
            warning.render(&mut cache, &self.source_table, root);
        }
        for error in &self.errors {
            error.render(&mut cache, &self.source_table, root);
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ─── Rendering ──────────────────────────────────────────────

type Src = String;

fn cfg() -> Config {
    Config::default()
        .with_index_type(IndexType::Byte)
        .with_char_set(CharSet::Unicode)
}

fn display_path(abs: &Path, root: Option<&Path>) -> String {
    if let Some(root) = root
        && let Ok(rel) = abs.strip_prefix(root)
    {
        return rel.display().to_string();
    }
    abs.display().to_string()
}

fn aspan(
    span: &IrSpan,
    source_table: &SourceTable,
    root: Option<&Path>,
) -> (Src, std::ops::Range<usize>) {
    let abs = source_table
        .get(span.file())
        .map(|sf| sf.path.clone())
        .unwrap_or_else(|| span.file().path().clone());
    let path = display_path(&abs, root);
    let range = span.span().start()..span.span().end();
    if range.start < range.end {
        return (path, range);
    }
    // Zero-width span: expand to the surrounding non-whitespace token on the same line
    if let Some(sf) = source_table.get(span.file()) {
        let src = sf.source.as_bytes();
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
    (path, range)
}

fn make_cache(source_table: &SourceTable, root: Option<&Path>) -> impl ariadne::Cache<Src> {
    let srcs: Vec<(Src, String)> = source_table
        .as_vec()
        .into_iter()
        .map(|(_file_id, sf)| (display_path(&sf.path, root), sf.source.clone()))
        .collect();
    sources(srcs)
}

impl DiagnosticReport {
    pub fn eprint(&self, source_table: &SourceTable, project_root: Option<&Path>) {
        let mut cache = make_cache(source_table, project_root);
        self.render(&mut cache, source_table, project_root);
    }

    fn render(
        &self,
        cache: &mut impl ariadne::Cache<Src>,
        source_table: &SourceTable,
        root: Option<&Path>,
    ) {
        let kind = match self.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
        };
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        if self.labels.is_empty() {
            eprintln!("{prefix}: {}", self.message);
            if let Some(note) = &self.note {
                eprintln!("  = note: {note}");
            }
            return;
        }

        let label_color = match self.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
        };
        let (path, range) = aspan(&self.labels[0].span, source_table, root);
        let mut builder = Report::build(kind, (path, range))
            .with_config(cfg())
            .with_message(&self.message);

        for label in &self.labels {
            let (path, range) = aspan(&label.span, source_table, root);
            builder = builder.with_label(
                Label::new((path, range))
                    .with_message(&label.message)
                    .with_color(label_color),
            );
        }

        if let Some(help) = &self.help {
            builder = builder.with_help(help);
        }
        if let Some(note) = &self.note {
            builder = builder.with_note(note);
        }

        let _ = builder.finish().eprint(cache);
    }
}
