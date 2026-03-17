use ariadne::{CharSet, Config, IndexType, Label, Report, ReportKind, sources};

use crate::dsl::resolver::ir::{self, SourceMap, Span};

// ─── Severity ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

// ─── Diagnostic Report ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReportLabel {
    pub span: Span,
    pub message: String,
}

impl From<(Span, String)> for ReportLabel {
    fn from((span, message): (Span, String)) -> Self {
        Self { span, message }
    }
}

impl From<(Span, &str)> for ReportLabel {
    fn from((span, message): (Span, &str)) -> Self {
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

// ─── Diagnostic Reports (batch with source map) ─────────────

pub struct DiagnosticReports {
    pub errors: Vec<DiagnosticReport>,
    pub warnings: Vec<DiagnosticReport>,
    pub source_map: SourceMap,
}

impl DiagnosticReports {
    pub fn eprint(&self) {
        let mut cache = make_cache(&self.source_map);
        for warning in &self.warnings {
            warning.render(&mut cache, &self.source_map);
        }
        for error in &self.errors {
            error.render(&mut cache, &self.source_map);
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
        .with_char_set(CharSet::Ascii)
}

fn aspan(span: &Span, source_map: &SourceMap) -> (Src, std::ops::Range<usize>) {
    let path = source_map.display_path(span.file);
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
        .enumerate()
        .map(|(id, f)| {
            (
                source_map.display_path(ir::FileId::from(id)),
                f.source.as_str(),
            )
        })
        .collect();
    sources(srcs)
}

impl DiagnosticReport {
    pub fn eprint(&self, source_map: &SourceMap) {
        let mut cache = make_cache(source_map);
        self.render(&mut cache, source_map);
    }

    fn render(&self, cache: &mut impl ariadne::Cache<Src>, source_map: &SourceMap) {
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

        let (path, range) = aspan(&self.labels[0].span, source_map);
        let mut builder = Report::build(kind, (path, range))
            .with_config(cfg())
            .with_message(&self.message);

        for label in &self.labels {
            let (path, range) = aspan(&label.span, source_map);
            builder = builder.with_label(Label::new((path, range)).with_message(&label.message));
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
