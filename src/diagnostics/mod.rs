use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use crate::Span;
use crate::core::table::FileId;
use crate::core::table::SharedTable;

// ─── ModulePath / EffectName ────────────────────────────────

/// A module path like `"tests/login"` or `"lib/helpers"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub String);

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An effect name (CamelCase by convention).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectName(pub String);

impl fmt::Display for EffectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ─── IrSpan ─────────────────────────────────────────────────

/// Byte range within a specific source file. Used by all IR nodes
/// and diagnostic labels for cross-file source annotations.
#[derive(Debug, Clone)]
pub struct IrSpan {
    file: FileId,
    span: Span,
}

impl IrSpan {
    /// Sentinel span for config-derived or synthetic values not tied to source.
    pub fn synthetic() -> Self {
        Self {
            file: FileId::new(std::path::PathBuf::from("<synthetic>")),
            span: Span::new(0, 0),
        }
    }

    pub fn new(file: FileId, span: Span) -> Self {
        Self { file, span }
    }

    pub fn file(&self) -> &FileId {
        &self.file
    }

    pub fn span(&self) -> &Span {
        &self.span
    }
}

// ─── FnId ───────────────────────────────────────────────────

/// Uniquely identifies a function (pure or impure) across the suite.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnId {
    pub module: ModulePath,
    pub name: String,
    pub arity: usize,
}

impl fmt::Display for FnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}/{}", self.module, self.name, self.arity)
    }
}

// ─── EffectId ───────────────────────────────────────────────

/// Uniquely identifies an effect definition across the suite.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectId {
    pub module: ModulePath,
    pub name: EffectName,
}

impl fmt::Display for EffectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.name)
    }
}

// ─── Severity ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

// ─── ReportLabel ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReportLabel {
    pub span: IrSpan,
    pub message: String,
}

// ─── Diagnostic ─────────────────────────────────────────────

#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<ReportLabel>,
    pub help: Option<String>,
    pub note: Option<String>,
}

impl Diagnostic {
    fn new(severity: Severity, message: String) -> Self {
        Self {
            severity,
            message,
            labels: Vec::new(),
            help: None,
            note: None,
        }
    }

    fn with_label(mut self, span: IrSpan, message: impl Into<String>) -> Self {
        self.labels.push(ReportLabel {
            span,
            message: message.into(),
        });
        self
    }

    #[allow(dead_code)]
    fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    #[allow(dead_code)]
    fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Render this diagnostic to stderr using ariadne.
    pub fn eprint(
        &self,
        source_table: &crate::core::table::SharedTable<FileId, crate::core::table::SourceFile>,
        project_root: Option<&std::path::Path>,
    ) {
        self.eprint_inner(None, source_table, project_root);
    }

    /// Render this diagnostic to stderr with a mnemonic cause/warning ID prefix.
    ///
    /// Output: `[cause-id] Error: message` instead of `Error: message`.
    pub fn eprint_with_id(
        &self,
        id: &impl fmt::Display,
        source_table: &crate::core::table::SharedTable<FileId, crate::core::table::SourceFile>,
        project_root: Option<&std::path::Path>,
    ) {
        self.eprint_inner(Some(&id.to_string()), source_table, project_root);
    }

    fn eprint_inner(
        &self,
        id: Option<&str>,
        source_table: &crate::core::table::SharedTable<FileId, crate::core::table::SourceFile>,
        project_root: Option<&std::path::Path>,
    ) {
        use ariadne::CharSet;
        use ariadne::Color;
        use ariadne::Config;
        use ariadne::IndexType;
        use ariadne::Label;
        use ariadne::Report;
        use ariadne::ReportKind;
        use ariadne::sources;

        let cfg = Config::default()
            .with_index_type(IndexType::Byte)
            .with_char_set(CharSet::Unicode);

        let kind = match (&self.severity, id) {
            (Severity::Error, Some(id)) => ReportKind::Custom(&format!("[{id}] Error"), Color::Red),
            (Severity::Error, None) => ReportKind::Error,
            (Severity::Warning, Some(id)) => {
                ReportKind::Custom(&format!("[{id}] Warning"), Color::Yellow)
            }
            (Severity::Warning, None) => ReportKind::Warning,
        };

        if self.labels.is_empty() {
            let prefix = match (&self.severity, id) {
                (Severity::Error, Some(id)) => format!("[{id}] error"),
                (Severity::Error, None) => "error".to_string(),
                (Severity::Warning, Some(id)) => format!("[{id}] warning"),
                (Severity::Warning, None) => "warning".to_string(),
            };
            eprintln!("{prefix}: {}", self.message);
            if let Some(note) = &self.note {
                eprintln!("  = note: {note}");
            }
            return;
        }

        // Build source cache from referenced files
        let display = |p: &std::path::Path| -> String {
            if let Some(root) = project_root
                && let Ok(rel) = p.strip_prefix(root)
            {
                return rel.display().to_string();
            }
            p.display().to_string()
        };

        let mut src_entries: Vec<(String, String)> = Vec::new();
        for label in &self.labels {
            let path_str = display(label.span.file().path());
            if !src_entries.iter().any(|(p, _)| p == &path_str)
                && let Some(sf) = source_table.get(label.span.file())
            {
                src_entries.push((path_str, sf.source.clone()));
            }
        }
        let mut cache = sources(src_entries);

        type Src = String;

        let first = &self.labels[0];
        let path: Src = display(first.span.file().path());
        let range: std::ops::Range<usize> = (*first.span.span()).into();

        let label_color = match self.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
        };
        let mut builder = Report::<(Src, std::ops::Range<usize>)>::build(kind, (path, range))
            .with_config(cfg)
            .with_message(&self.message);

        for label in &self.labels {
            let lpath: Src = display(label.span.file().path());
            let lrange: std::ops::Range<usize> = (*label.span.span()).into();
            builder = builder.with_label(
                Label::new((lpath, lrange))
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

        let _ = builder.finish().eprint(&mut cache);
    }
}

// ─── LoweringBail ───────────────────────────────────────────

/// Error type for lowering failures — either a skip or an invalid definition.
#[derive(Debug, Clone)]
pub enum LoweringBail {
    Skip(Arc<SkipReport>),
    Invalid(Arc<InvalidReport>),
}

impl LoweringBail {
    pub fn skip(report: SkipReport) -> Self {
        Self::Skip(Arc::new(report))
    }

    pub fn invalid(report: InvalidReport) -> Self {
        Self::Invalid(Arc::new(report))
    }
}

impl fmt::Display for LoweringBail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoweringBail::Skip(r) => write!(f, "skipped: {r}"),
            LoweringBail::Invalid(r) => write!(f, "invalid: {r}"),
        }
    }
}

impl LoweringBail {
    pub fn cause_id(&self) -> CauseId {
        match self {
            LoweringBail::Skip(skip) => skip.cause_id(),
            LoweringBail::Invalid(invalid) => invalid.cause_id(),
        }
    }
}

impl std::error::Error for LoweringBail {}

// ─── InvalidReport ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum InvalidReport {
    Cycle(CycleReport),
    PurityViolation {
        span: IrSpan,
    },
    UndefinedFunctionCall {
        name: String,
        arity: usize,
        span: IrSpan,
    },
    UndefinedEffectNeed {
        name: String,
        span: IrSpan,
    },
    UndefinedFunctionImport {
        name: String,
        module_path: ModulePath,
        span: IrSpan,
    },
    UndefinedEffectImport {
        name: String,
        module_path: ModulePath,
        span: IrSpan,
    },
    UndefinedModuleImport {
        module_path: ModulePath,
        span: IrSpan,
    },
    NameConflict {
        name: String,
        first: IrSpan,
        second: IrSpan,
    },
    InvalidRegex {
        pattern: String,
        error: String,
        span: IrSpan,
    },
    ParseError {
        module_path: ModulePath,
        message: String,
        span: IrSpan,
    },
    EmptyTestBody {
        name: String,
        span: IrSpan,
    },
}

impl fmt::Display for InvalidReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidReport::Cycle(c) => write!(f, "{c}"),
            InvalidReport::PurityViolation { .. } => write!(f, "shell operation in pure context"),
            InvalidReport::UndefinedFunctionCall { name, arity, .. } => {
                write!(f, "undefined function `{name}/{arity}`")
            }
            InvalidReport::UndefinedEffectNeed { name, .. } => {
                write!(f, "undefined effect `{name}`")
            }
            InvalidReport::UndefinedFunctionImport {
                name, module_path, ..
            } => {
                write!(f, "function `{name}` not exported by `{module_path}`")
            }
            InvalidReport::UndefinedEffectImport {
                name, module_path, ..
            } => {
                write!(f, "effect `{name}` not exported by `{module_path}`")
            }
            InvalidReport::UndefinedModuleImport { module_path, .. } => {
                write!(f, "module `{module_path}` not found")
            }
            InvalidReport::NameConflict { name, .. } => {
                write!(f, "name conflict: `{name}` defined twice")
            }
            InvalidReport::InvalidRegex { pattern, error, .. } => {
                write!(f, "invalid regex `{pattern}`: {error}")
            }
            InvalidReport::ParseError {
                module_path,
                message,
                ..
            } => {
                write!(f, "parse error in `{module_path}`: {message}")
            }
            InvalidReport::EmptyTestBody { name, .. } => {
                write!(f, "test `{name}` has no shell blocks")
            }
        }
    }
}

impl InvalidReport {
    pub fn cause_id(&self) -> CauseId {
        match self {
            InvalidReport::Cycle(cycle) => match cycle {
                CycleReport::Function { chain } => {
                    let first = &chain[0].id;
                    CauseId::generate(&first.module.0, &first.name, first.arity, "cycle")
                }
                CycleReport::Effect { chain } => {
                    let first = &chain[0].id;
                    CauseId::generate(&first.module.0, &first.name.0, 0, "cycle")
                }
            },
            InvalidReport::PurityViolation { .. } => {
                CauseId::generate("", "", 0, "purity_violation")
            }
            InvalidReport::UndefinedFunctionCall { name, arity, .. } => {
                CauseId::generate("", name, *arity, "undefined_fn_call")
            }
            InvalidReport::UndefinedEffectNeed { name, .. } => {
                CauseId::generate("", name, 0, "undefined_effect_need")
            }
            InvalidReport::UndefinedFunctionImport {
                name, module_path, ..
            } => CauseId::generate(&module_path.0, name, 0, "undefined_fn_import"),
            InvalidReport::UndefinedEffectImport {
                name, module_path, ..
            } => CauseId::generate(&module_path.0, name, 0, "undefined_effect_import"),
            InvalidReport::UndefinedModuleImport { module_path, .. } => {
                CauseId::generate(&module_path.0, "", 0, "undefined_module_import")
            }
            InvalidReport::NameConflict { name, .. } => {
                CauseId::generate("", name, 0, "name_conflict")
            }
            InvalidReport::InvalidRegex { pattern, .. } => {
                CauseId::generate("", pattern, 0, "invalid_regex")
            }
            InvalidReport::ParseError { module_path, .. } => {
                CauseId::generate(&module_path.0, "", 0, "parse_error")
            }
            InvalidReport::EmptyTestBody { name, .. } => {
                CauseId::generate("", name, 0, "empty_test_body")
            }
        }
    }
}

impl std::error::Error for InvalidReport {}

// ─── SkipReport ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SkipReport {
    pub definition: DefinitionRef,
    pub marker_span: IrSpan,
    pub evaluation: SkipEvaluation,
}

impl fmt::Display for SkipReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} skipped: {}", self.definition, self.evaluation)
    }
}

impl SkipReport {
    pub fn cause_id(&self) -> CauseId {
        match &self.definition {
            DefinitionRef::Fn(fn_id) => {
                CauseId::generate(&fn_id.module.0, &fn_id.name, fn_id.arity, "skip")
            }
            DefinitionRef::Effect(eff_id) => {
                CauseId::generate(&eff_id.module.0, &eff_id.name.0, 0, "skip")
            }
            DefinitionRef::Test { name, module } => CauseId::generate(&module.0, name, 0, "skip"),
        }
    }
}

impl std::error::Error for SkipReport {}

#[derive(Debug, Clone)]
pub enum DefinitionRef {
    Fn(FnId),
    Effect(EffectId),
    Test { name: String, module: ModulePath },
}

impl fmt::Display for DefinitionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefinitionRef::Fn(id) => write!(f, "function `{id}`"),
            DefinitionRef::Effect(id) => write!(f, "effect `{id}`"),
            DefinitionRef::Test { name, module } => write!(f, "test `{name}` in `{module}`"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SkipEvaluation {
    Unconditional,
    Bare {
        value: String,
        met: bool,
    },
    Eq {
        lhs: String,
        rhs: String,
        met: bool,
    },
    Regex {
        value: String,
        pattern: String,
        met: bool,
    },
}

impl fmt::Display for SkipEvaluation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipEvaluation::Unconditional => write!(f, "unconditional skip"),
            SkipEvaluation::Bare { value, .. } => {
                if value.is_empty() {
                    write!(f, "evaluated to empty")
                } else {
                    write!(f, "evaluated to non-empty: {value:?}")
                }
            }
            SkipEvaluation::Eq { lhs, rhs, .. } => {
                if lhs == rhs {
                    write!(f, "{lhs:?} == {rhs:?}")
                } else {
                    write!(f, "{lhs:?} != {rhs:?}")
                }
            }
            SkipEvaluation::Regex {
                value,
                pattern,
                met,
            } => {
                if *met {
                    write!(f, "{value:?} matched /{pattern}/")
                } else {
                    write!(f, "{value:?} did not match /{pattern}/")
                }
            }
        }
    }
}

// ─── CycleReport ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CycleReport {
    Function { chain: Vec<FnCycleEntry> },
    Effect { chain: Vec<EffectCycleEntry> },
}

impl fmt::Display for CycleReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CycleReport::Function { chain } => {
                let names: Vec<String> = chain.iter().map(|e| e.id.to_string()).collect();
                write!(f, "function cycle: {}", names.join(" -> "))
            }
            CycleReport::Effect { chain } => {
                let names: Vec<String> = chain.iter().map(|e| e.id.to_string()).collect();
                write!(f, "effect cycle: {}", names.join(" -> "))
            }
        }
    }
}

impl std::error::Error for CycleReport {}

#[derive(Debug, Clone)]
pub struct FnCycleEntry {
    pub id: FnId,
    pub call_span: IrSpan,
}

#[derive(Debug, Clone)]
pub struct EffectCycleEntry {
    pub id: EffectId,
    pub need_span: IrSpan,
}

// ─── CauseId ────────────────────────────────────────────────

/// Stable mnemonic identifier, e.g. `"broken-walrus-0042"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CauseId {
    pub id: String,
}

impl CauseId {
    /// Generate a deterministic mnemonic from hashed inputs.
    pub fn generate(module: &str, name: &str, arity: usize, error_kind: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        module.hash(&mut hasher);
        name.hash(&mut hasher);
        arity.hash(&mut hasher);
        error_kind.hash(&mut hasher);
        let hash = hasher.finish();

        let adj_idx = (hash & 0xFF) as usize;
        let noun_idx = ((hash >> 8) & 0xFF) as usize;
        let suffix = ((hash >> 16) % 10000) as u16;

        let adj = ADJECTIVES[adj_idx];
        let noun = NOUNS[noun_idx];
        Self {
            id: format!("{adj}-{noun}-{suffix:04}"),
        }
    }
}

impl fmt::Display for CauseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

// ─── Cause / Warning ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Cause {
    Skip(Arc<SkipReport>),
    Invalid(Arc<InvalidReport>),
}

impl Cause {
    pub fn skip(report: SkipReport) -> Self {
        Self::Skip(Arc::new(report))
    }

    pub fn invalid(report: InvalidReport) -> Self {
        Self::Invalid(Arc::new(report))
    }

    pub fn from_bail(bail: &LoweringBail) -> Self {
        match bail {
            LoweringBail::Skip(s) => Cause::Skip(s.clone()),
            LoweringBail::Invalid(i) => Cause::Invalid(i.clone()),
        }
    }
}

pub type CauseTable = SharedTable<CauseId, Cause>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WarningId {
    pub id: String,
}

impl fmt::Display for WarningId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

#[derive(Debug, Clone)]
pub enum Warning {
    // No variants yet — placeholder for future use.
}

pub type WarningTable = SharedTable<WarningId, Warning>;

// ─── From<&InvalidReport> for Diagnostic ────────────────────

impl From<&InvalidReport> for Diagnostic {
    fn from(report: &InvalidReport) -> Self {
        match report {
            InvalidReport::Cycle(cycle) => {
                let msg = cycle.to_string();
                let mut diag = Diagnostic::new(Severity::Error, msg);
                match cycle {
                    CycleReport::Function { chain } => {
                        for (i, entry) in chain.iter().enumerate() {
                            let next = &chain[(i + 1) % chain.len()];
                            diag.labels.push(ReportLabel {
                                span: entry.call_span.clone(),
                                message: format!("{} calls {}", entry.id, next.id),
                            });
                        }
                    }
                    CycleReport::Effect { chain } => {
                        for (i, entry) in chain.iter().enumerate() {
                            let next = &chain[(i + 1) % chain.len()];
                            diag.labels.push(ReportLabel {
                                span: entry.need_span.clone(),
                                message: format!("{} needs {}", entry.id, next.id),
                            });
                        }
                    }
                }
                diag
            }
            InvalidReport::PurityViolation { span } => {
                Diagnostic::new(Severity::Error, "shell operation in pure context".into())
                    .with_label(span.clone(), "not allowed in pure function")
            }
            InvalidReport::UndefinedFunctionCall { name, arity, span } => Diagnostic::new(
                Severity::Error,
                format!("undefined function `{name}/{arity}`"),
            )
            .with_label(span.clone(), "not found"),
            InvalidReport::UndefinedEffectNeed { name, span } => {
                Diagnostic::new(Severity::Error, format!("undefined effect `{name}`"))
                    .with_label(span.clone(), "not found")
            }
            InvalidReport::UndefinedFunctionImport {
                name,
                module_path,
                span,
            } => Diagnostic::new(
                Severity::Error,
                format!("function `{name}` not exported by `{module_path}`"),
            )
            .with_label(span.clone(), "not exported"),
            InvalidReport::UndefinedEffectImport {
                name,
                module_path,
                span,
            } => Diagnostic::new(
                Severity::Error,
                format!("effect `{name}` not exported by `{module_path}`"),
            )
            .with_label(span.clone(), "not exported"),
            InvalidReport::UndefinedModuleImport { module_path, span } => {
                Diagnostic::new(Severity::Error, format!("module `{module_path}` not found"))
                    .with_label(span.clone(), "imported here")
            }
            InvalidReport::NameConflict {
                name,
                first,
                second,
            } => Diagnostic::new(
                Severity::Error,
                format!("name conflict: `{name}` defined twice"),
            )
            .with_label(second.clone(), "conflicts here")
            .with_label(first.clone(), "first defined here"),
            InvalidReport::InvalidRegex {
                pattern,
                error,
                span,
            } => Diagnostic::new(Severity::Error, format!("invalid regex `{pattern}`"))
                .with_label(span.clone(), error.as_str()),
            InvalidReport::ParseError {
                module_path,
                message,
                span,
            } => Diagnostic::new(Severity::Error, format!("parse error in `{module_path}`"))
                .with_label(span.clone(), message.as_str()),
            InvalidReport::EmptyTestBody { name, span } => Diagnostic::new(
                Severity::Error,
                format!("test `{name}` has no shell blocks"),
            )
            .with_label(
                span.clone(),
                "test body must contain at least one shell block",
            ),
        }
    }
}

// ─── From<&SkipReport> for Diagnostic ───────────────────────

impl From<&SkipReport> for Diagnostic {
    fn from(report: &SkipReport) -> Self {
        let msg = format!("{} skipped", report.definition);
        let eval_msg = report.evaluation.to_string();
        Diagnostic::new(Severity::Warning, msg).with_label(report.marker_span.clone(), eval_msg)
    }
}

// ─── From<&Cause> for Diagnostic ────────────────────────────

impl From<&Cause> for Diagnostic {
    fn from(cause: &Cause) -> Self {
        match cause {
            Cause::Skip(skip) => Diagnostic::from(skip.as_ref()),
            Cause::Invalid(invalid) => Diagnostic::from(invalid.as_ref()),
        }
    }
}

// ─── From<&Warning> for Diagnostic ──────────────────────────

impl From<&Warning> for Diagnostic {
    fn from(warning: &Warning) -> Self {
        match *warning {}
    }
}

// ─── Word lists for CauseId ────────────────────────────────

/// 256 adjectives evoking brokenness, damage, trouble, and disrepair.
const ADJECTIVES: [&str; 256] = [
    // physical damage
    "bent", "blown", "broke", "burnt", "burst", "cheap", "cracked", "crashed", "crazed", "crisp",
    "cross", "crude", "crushed", "cursed", "cut", "damp", "dead", "deaf", "dim", "dingy", "dirty",
    "dizzy", "drafty", "drained", "dreary", "dried", "dull", "dusty", "empty", "erased", "eroded",
    "failed", "faint", "faulty", "feeble", "fierce", "filthy", "flaky", "flat", "flawed", "foggy",
    "forlorn", "foul", "frail", "frantic", "frayed", "frozen", "fudged",
    // decay and wear
    "funky", "fuzzy", "gashed", "gaunt", "glum", "gnarly", "goofy", "grave", "grim", "grimy",
    "gritty", "gross", "grouchy", "grubby", "guilty", "gummy", "hairy", "harsh", "hazy", "hoarse",
    "hollow", "horrid", "humid", "hurt", "icy", "iffy", "inert", "inky", "itchy", "jaded",
    "jagged", "janky", "jarred", "jerky", "jilted", "jittery", "jolted", "jumbled", "junky",
    "lame", "leaky", "limp", "listless", "livid", "lonely", "loose", "lost", "lousy",
    // absence and confusion
    "lumpy", "mad", "mangled", "marred", "matted", "meager", "measly", "messy", "milky", "misled",
    "missing", "misty", "mixed", "moody", "mopey", "mossy", "mousy", "muddy", "muggy", "murky",
    "mushy", "musty", "muted", "nicked", "noisy", "numb", "odd", "oily", "opaque", "ornery",
    "pale", "parched", "patchy", "peaked", "pesky", "phony", "picky", "pitchy", "plain", "poor",
    "puffy", "pulpy", "punky", "queasy", "ratty", "raw", "rickety", "rigid",
    // emotional distress
    "rocky", "rotten", "rough", "rugged", "rusty", "sad", "sandy", "scabby", "scarred", "scraped",
    "scratchy", "seedy", "shady", "shaky", "shallow", "sharp", "shifty", "shoddy", "shrill",
    "sick", "singed", "sketchy", "slack", "slimy", "sloppy", "slow", "sluggish", "smelly", "smoky",
    "snaggy", "soggy", "sooty", "sore", "sorry", "sour", "spent", "spiny", "split", "spotty",
    "stale", "stark", "steep", "sticky", "stiff", "stingy", "stormy", "stray", "stubby",
    // chaos and wrongness
    "stuck", "stuffy", "stunted", "sulky", "sunken", "swampy", "tacky", "tainted", "tangled",
    "tart", "tense", "thorny", "tired", "torn", "toxic", "trashy", "tricky", "troubled", "turbid",
    "ugly", "uncut", "undone", "uneven", "unfit", "unkempt", "unruly", "unset", "untidy", "upset",
    "vacant", "vague", "void", "warped", "wasted", "watery", "weak", "weary", "weedy", "weird",
    "wilted", "wiry", "wobbly", "wonky", "wooden", "worn", "wrecked", "wrong", "zapped",
    // stragglers
    "ashen", "balky", "botched", "busted", "clammy", "clunky", "corroded", "crumbly", "dented",
    "dismal", "frazzled", "garbled", "ghastly", "gouged", "manky", "pitted",
];

/// 256 animal and creature nouns — the charismatic megafauna of your errors.
const NOUNS: [&str; 256] = [
    // insects and arachnids
    "ant",
    "aphid",
    "bee",
    "beetle",
    "bug",
    "cicada",
    "cricket",
    "drone",
    "earwig",
    "firefly",
    "flea",
    "fly",
    "gnat",
    "grub",
    "hornet",
    "larva",
    "locust",
    "mantis",
    "mayfly",
    "midge",
    "mite",
    "moth",
    "nymph",
    "roach",
    "scarab",
    "slug",
    "snail",
    "spider",
    "termite",
    "tick",
    "wasp",
    "weevil",
    // water creatures
    "bass",
    "betta",
    "clam",
    "cod",
    "coral",
    "crab",
    "dace",
    "eel",
    "guppy",
    "hake",
    "koi",
    "leech",
    "limpet",
    "mussel",
    "newt",
    "octopus",
    "orca",
    "otter",
    "perch",
    "pike",
    "prawn",
    "puffer",
    "ray",
    "salmon",
    "seal",
    "shark",
    "shrimp",
    "squid",
    "trout",
    "tuna",
    "turtle",
    "walrus",
    // birds
    "avocet",
    "bittern",
    "canary",
    "condor",
    "crane",
    "crow",
    "cuckoo",
    "curlew",
    "dodo",
    "dove",
    "eagle",
    "egret",
    "falcon",
    "finch",
    "goose",
    "grouse",
    "gull",
    "hawk",
    "heron",
    "ibis",
    "jay",
    "kite",
    "lark",
    "loon",
    "magpie",
    "martin",
    "osprey",
    "owl",
    "parrot",
    "pelican",
    "pigeon",
    "plover",
    // mammals — small
    "badger",
    "bat",
    "beaver",
    "chipmunk",
    "coypu",
    "desman",
    "dormouse",
    "ermine",
    "ferret",
    "fox",
    "gerbil",
    "gopher",
    "hamster",
    "hare",
    "hedgehog",
    "lemming",
    "marmot",
    "mink",
    "mole",
    "mouse",
    "opossum",
    "pika",
    "possum",
    "rabbit",
    "raccoon",
    "rat",
    "shrew",
    "skunk",
    "squirrel",
    "stoat",
    "vole",
    "weasel",
    // mammals — large
    "alpaca",
    "bison",
    "boar",
    "buffalo",
    "camel",
    "cougar",
    "coyote",
    "deer",
    "dingo",
    "donkey",
    "elk",
    "gazelle",
    "gnu",
    "gorilla",
    "horse",
    "hyena",
    "ibex",
    "impala",
    "jackal",
    "jaguar",
    "kudu",
    "lemur",
    "leopard",
    "lion",
    "llama",
    "lynx",
    "moose",
    "mule",
    "ox",
    "panda",
    "panther",
    "puma",
    // reptiles and amphibians
    "adder",
    "asp",
    "axolotl",
    "boa",
    "bullfrog",
    "chameleon",
    "cobra",
    "dragon",
    "frog",
    "gecko",
    "iguana",
    "komodo",
    "lizard",
    "mamba",
    "python",
    "rattler",
    "skink",
    "taipan",
    "toad",
    "tortoise",
    "viper",
    "worm",
    "caiman",
    "anole",
    // mythical and miscellaneous
    "beast",
    "blob",
    "boggart",
    "chimera",
    "drake",
    "gargoyle",
    "ghost",
    "goblin",
    "golem",
    "gremlin",
    "griffin",
    "imp",
    "kraken",
    "minotaur",
    "ogre",
    "phantom",
    "pixie",
    "roc",
    "serpent",
    "shade",
    "sphinx",
    "sprite",
    "troll",
    "whelk",
    "wombat",
    "wren",
    "yak",
    "yeti",
    "zebu",
    "civet",
    "dhole",
    "raven",
    "bunting",
    "jacana",
    "murre",
    "quail",
    "robin",
    "stork",
    "swift",
    "tern",
    "vireo",
    "thrush",
    "nutria",
    "pangolin",
    "tapir",
    "auk",
    "booby",
    "corgi",
    "darter",
    "emu",
    "flounder",
    "gannet",
    "haddock",
    "jellyfish",
    "kinglet",
    "lamprey",
    "macaw",
    "narwhal",
    "oriole",
    "penguin",
    "quokka",
    "rooster",
    "starling",
    "tadpole",
    "urial",
    "vulture",
    "warbler",
    "xerus",
    "yapok",
    "zorilla",
    "barnacle",
    "capybara",
];

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    use std::path::PathBuf;

    fn test_file() -> FileId {
        FileId::new(PathBuf::from("/test/file.relux"))
    }

    fn test_span() -> IrSpan {
        IrSpan::new(test_file(), Span::new(0, 10))
    }

    fn test_span_at(start: usize, end: usize) -> IrSpan {
        IrSpan::new(test_file(), Span::new(start, end))
    }

    fn test_module() -> ModulePath {
        ModulePath("tests/login".into())
    }

    fn other_module() -> ModulePath {
        ModulePath("lib/helpers".into())
    }

    fn test_fn_id() -> FnId {
        FnId {
            module: test_module(),
            name: "my_fn".into(),
            arity: 2,
        }
    }

    fn test_effect_id() -> EffectId {
        EffectId {
            module: test_module(),
            name: EffectName("MyEffect".into()),
        }
    }

    // ── IrSpan ──────────────────────────────────────────────

    #[test]
    fn ir_span_accessors() {
        let s = test_span();
        assert_eq!(s.file(), &test_file());
        assert_eq!(s.span().start(), 0);
        assert_eq!(s.span().end(), 10);
    }

    #[test]
    fn ir_span_clone() {
        let s = test_span();
        let s2 = s.clone();
        assert_eq!(s.file(), s2.file());
        assert_eq!(s.span().start(), s2.span().start());
        assert_eq!(s.span().end(), s2.span().end());
    }

    #[test]
    fn ir_span_different_files() {
        let a = IrSpan::new(FileId::new(PathBuf::from("/a.relux")), Span::new(0, 5));
        let b = IrSpan::new(FileId::new(PathBuf::from("/b.relux")), Span::new(0, 5));
        assert_ne!(a.file(), b.file());
    }

    // ── FnId ────────────────────────────────────────────────

    #[test]
    fn fn_id_equality() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let b = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn fn_id_inequality_arity() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let b = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 2,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fn_id_inequality_module() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let b = FnId {
            module: other_module(),
            name: "f".into(),
            arity: 1,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fn_id_inequality_name() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let b = FnId {
            module: test_module(),
            name: "g".into(),
            arity: 1,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fn_id_hash_consistency() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let b = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn fn_id_zero_arity() {
        let a = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 0,
        };
        let b = FnId {
            module: test_module(),
            name: "f".into(),
            arity: 1,
        };
        assert_ne!(a, b);
    }

    // ── EffectId ────────────────────────────────────────────

    #[test]
    fn effect_id_equality() {
        let a = test_effect_id();
        let b = test_effect_id();
        assert_eq!(a, b);
    }

    #[test]
    fn effect_id_inequality_name() {
        let a = EffectId {
            module: test_module(),
            name: EffectName("A".into()),
        };
        let b = EffectId {
            module: test_module(),
            name: EffectName("B".into()),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn effect_id_inequality_module() {
        let a = EffectId {
            module: test_module(),
            name: EffectName("E".into()),
        };
        let b = EffectId {
            module: other_module(),
            name: EffectName("E".into()),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn effect_id_hash_consistency() {
        let a = test_effect_id();
        let b = test_effect_id();
        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    // ── CauseId ─────────────────────────────────────────────

    #[test]
    fn cause_id_format() {
        let id = CauseId::generate("mod", "func", 2, "undefined");
        let re = regex::Regex::new(r"^[a-z]+-[a-z]+-\d{4}$").unwrap();
        assert!(re.is_match(&id.id), "bad format: {}", id.id);
    }

    #[test]
    fn cause_id_deterministic() {
        let a = CauseId::generate("mod", "func", 2, "undefined");
        let b = CauseId::generate("mod", "func", 2, "undefined");
        assert_eq!(a, b);
    }

    #[test]
    fn cause_id_differs_for_different_error_kind() {
        let a = CauseId::generate("mod", "func", 2, "undefined");
        let b = CauseId::generate("mod", "func", 2, "cycle");
        assert_ne!(a, b);
    }

    #[test]
    fn cause_id_differs_for_different_module() {
        let a = CauseId::generate("mod_a", "func", 2, "undefined");
        let b = CauseId::generate("mod_b", "func", 2, "undefined");
        assert_ne!(a, b);
    }

    #[test]
    fn cause_id_differs_for_different_name() {
        let a = CauseId::generate("mod", "alpha", 2, "undefined");
        let b = CauseId::generate("mod", "beta", 2, "undefined");
        assert_ne!(a, b);
    }

    #[test]
    fn cause_id_differs_for_different_arity() {
        let a = CauseId::generate("mod", "func", 1, "undefined");
        let b = CauseId::generate("mod", "func", 2, "undefined");
        assert_ne!(a, b);
    }

    #[test]
    fn cause_id_equality_and_hash() {
        let a = CauseId::generate("mod", "func", 2, "undefined");
        let b = CauseId::generate("mod", "func", 2, "undefined");
        assert_eq!(a, b);
        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    // ── Diagnostic from InvalidReport ───────────────────────

    #[test]
    fn diagnostic_from_undefined_function_call() {
        let r = InvalidReport::UndefinedFunctionCall {
            name: "foo".into(),
            arity: 3,
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert_eq!(d.severity, Severity::Error);
        assert!(d.message.contains("foo"));
        assert!(d.message.contains("3"));
        assert_eq!(d.labels.len(), 1);
    }

    #[test]
    fn diagnostic_from_undefined_function_call_arity_zero() {
        let r = InvalidReport::UndefinedFunctionCall {
            name: "bar".into(),
            arity: 0,
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("0"));
    }

    #[test]
    fn diagnostic_from_purity_violation() {
        let r = InvalidReport::PurityViolation { span: test_span() };
        let d = Diagnostic::from(&r);
        assert_eq!(d.severity, Severity::Error);
        assert!(d.message.contains("pure"));
        assert_eq!(d.labels.len(), 1);
    }

    #[test]
    fn diagnostic_from_name_conflict() {
        let r = InvalidReport::NameConflict {
            name: "dup".into(),
            first: test_span_at(0, 5),
            second: test_span_at(20, 25),
        };
        let d = Diagnostic::from(&r);
        assert_eq!(d.labels.len(), 2);
        assert!(d.labels[0].message.contains("conflicts"));
        assert!(d.labels[1].message.contains("first defined"));
    }

    #[test]
    fn diagnostic_from_invalid_regex() {
        let r = InvalidReport::InvalidRegex {
            pattern: "[bad".into(),
            error: "unclosed bracket".into(),
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("[bad"));
        assert!(d.labels[0].message.contains("unclosed bracket"));
    }

    #[test]
    fn diagnostic_from_cycle_function_two_entries() {
        let r = InvalidReport::Cycle(CycleReport::Function {
            chain: vec![
                FnCycleEntry {
                    id: test_fn_id(),
                    call_span: test_span_at(0, 5),
                },
                FnCycleEntry {
                    id: FnId {
                        module: test_module(),
                        name: "other".into(),
                        arity: 1,
                    },
                    call_span: test_span_at(10, 15),
                },
            ],
        });
        let d = Diagnostic::from(&r);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.labels.len(), 2);
    }

    #[test]
    fn diagnostic_from_cycle_function_self() {
        let r = InvalidReport::Cycle(CycleReport::Function {
            chain: vec![FnCycleEntry {
                id: test_fn_id(),
                call_span: test_span(),
            }],
        });
        let d = Diagnostic::from(&r);
        assert_eq!(d.labels.len(), 1);
    }

    #[test]
    fn diagnostic_from_cycle_effect() {
        let r = InvalidReport::Cycle(CycleReport::Effect {
            chain: vec![EffectCycleEntry {
                id: test_effect_id(),
                need_span: test_span(),
            }],
        });
        let d = Diagnostic::from(&r);
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn diagnostic_from_undefined_module_import() {
        let r = InvalidReport::UndefinedModuleImport {
            module_path: ModulePath("lib/missing".into()),
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("lib/missing"));
    }

    #[test]
    fn diagnostic_from_undefined_function_import() {
        let r = InvalidReport::UndefinedFunctionImport {
            name: "helper".into(),
            module_path: other_module(),
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("helper"));
        assert!(d.message.contains("lib/helpers"));
    }

    #[test]
    fn diagnostic_from_undefined_effect_import() {
        let r = InvalidReport::UndefinedEffectImport {
            name: "Db".into(),
            module_path: other_module(),
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("Db"));
        assert!(d.message.contains("lib/helpers"));
    }

    #[test]
    fn diagnostic_from_undefined_effect_need() {
        let r = InvalidReport::UndefinedEffectNeed {
            name: "Missing".into(),
            span: test_span(),
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("Missing"));
    }

    // ── Diagnostic from SkipReport ──────────────────────────

    fn make_skip(eval: SkipEvaluation) -> SkipReport {
        SkipReport {
            definition: DefinitionRef::Fn(test_fn_id()),
            marker_span: test_span(),
            evaluation: eval,
        }
    }

    #[test]
    fn diagnostic_from_skip_unconditional() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Unconditional));
        assert_eq!(d.severity, Severity::Warning);
        assert!(d.labels[0].message.contains("unconditional"));
    }

    #[test]
    fn diagnostic_from_skip_bare_met() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Bare {
            value: "yes".into(),
            met: true,
        }));
        assert!(d.labels[0].message.contains("non-empty"));
    }

    #[test]
    fn diagnostic_from_skip_bare_unmet() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Bare {
            value: String::new(),
            met: false,
        }));
        assert!(d.labels[0].message.contains("empty"));
    }

    #[test]
    fn diagnostic_from_skip_eq_match() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Eq {
            lhs: "a".into(),
            rhs: "a".into(),
            met: true,
        }));
        assert!(d.labels[0].message.contains("=="));
    }

    #[test]
    fn diagnostic_from_skip_eq_no_match() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Eq {
            lhs: "a".into(),
            rhs: "b".into(),
            met: false,
        }));
        assert!(d.labels[0].message.contains("!="));
    }

    #[test]
    fn diagnostic_from_skip_regex_match() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Regex {
            value: "hello".into(),
            pattern: "h.*".into(),
            met: true,
        }));
        assert!(d.labels[0].message.contains("matched"));
    }

    #[test]
    fn diagnostic_from_skip_regex_no_match() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Regex {
            value: "hello".into(),
            pattern: "^x".into(),
            met: false,
        }));
        assert!(d.labels[0].message.contains("did not match"));
    }

    #[test]
    fn diagnostic_from_skip_fn_definition() {
        let r = SkipReport {
            definition: DefinitionRef::Fn(test_fn_id()),
            marker_span: test_span(),
            evaluation: SkipEvaluation::Unconditional,
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("function"));
    }

    #[test]
    fn diagnostic_from_skip_effect_definition() {
        let r = SkipReport {
            definition: DefinitionRef::Effect(test_effect_id()),
            marker_span: test_span(),
            evaluation: SkipEvaluation::Unconditional,
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("effect"));
    }

    #[test]
    fn diagnostic_from_skip_test_definition() {
        let r = SkipReport {
            definition: DefinitionRef::Test {
                name: "my test".into(),
                module: test_module(),
            },
            marker_span: test_span(),
            evaluation: SkipEvaluation::Unconditional,
        };
        let d = Diagnostic::from(&r);
        assert!(d.message.contains("test"));
    }

    // ── Severity ────────────────────────────────────────────

    #[test]
    fn invalid_report_produces_error_severity() {
        let variants: Vec<InvalidReport> = vec![
            InvalidReport::PurityViolation { span: test_span() },
            InvalidReport::UndefinedFunctionCall {
                name: "f".into(),
                arity: 0,
                span: test_span(),
            },
            InvalidReport::UndefinedEffectNeed {
                name: "E".into(),
                span: test_span(),
            },
            InvalidReport::NameConflict {
                name: "n".into(),
                first: test_span(),
                second: test_span(),
            },
            InvalidReport::InvalidRegex {
                pattern: "(".into(),
                error: "err".into(),
                span: test_span(),
            },
        ];
        for v in &variants {
            assert_eq!(Diagnostic::from(v).severity, Severity::Error);
        }
    }

    #[test]
    fn skip_report_produces_warning_severity() {
        let d = Diagnostic::from(&make_skip(SkipEvaluation::Unconditional));
        assert_eq!(d.severity, Severity::Warning);
    }

    // ── LoweringBail ────────────────────────────────────────

    #[test]
    fn lowering_bail_skip_variant() {
        let bail = LoweringBail::skip(make_skip(SkipEvaluation::Unconditional));
        assert!(matches!(bail, LoweringBail::Skip(_)));
    }

    #[test]
    fn lowering_bail_invalid_variant() {
        let bail = LoweringBail::invalid(InvalidReport::PurityViolation { span: test_span() });
        assert!(matches!(bail, LoweringBail::Invalid(_)));
    }

    #[test]
    fn lowering_bail_clone() {
        let bail = LoweringBail::skip(make_skip(SkipEvaluation::Unconditional));
        let _cloned = bail.clone();
    }

    // ── CycleReport ─────────────────────────────────────────

    #[test]
    fn cycle_report_function_single() {
        let r = CycleReport::Function {
            chain: vec![FnCycleEntry {
                id: test_fn_id(),
                call_span: test_span(),
            }],
        };
        if let CycleReport::Function { chain } = &r {
            assert_eq!(chain.len(), 1);
        }
    }

    #[test]
    fn cycle_report_function_chain() {
        let r = CycleReport::Function {
            chain: vec![
                FnCycleEntry {
                    id: test_fn_id(),
                    call_span: test_span(),
                },
                FnCycleEntry {
                    id: FnId {
                        module: test_module(),
                        name: "b".into(),
                        arity: 0,
                    },
                    call_span: test_span(),
                },
            ],
        };
        if let CycleReport::Function { chain } = &r {
            assert_eq!(chain.len(), 2);
            assert_eq!(chain[0].id.name, "my_fn");
            assert_eq!(chain[1].id.name, "b");
        }
    }

    #[test]
    fn cycle_report_effect_chain() {
        let r = CycleReport::Effect {
            chain: vec![EffectCycleEntry {
                id: test_effect_id(),
                need_span: test_span(),
            }],
        };
        if let CycleReport::Effect { chain } = &r {
            assert_eq!(chain.len(), 1);
        }
    }

    // ── Cause / Warning tables ──────────────────────────────

    #[test]
    fn cause_table_insert_and_retrieve() {
        let table: CauseTable = SharedTable::new();
        let id = CauseId::generate("m", "f", 0, "invalid");
        table.insert(
            id.clone(),
            Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        let cause = table.get(&id);
        assert!(cause.is_some());
        assert!(matches!(cause.unwrap(), Cause::Invalid(_)));
    }

    #[test]
    fn cause_table_skip_variant() {
        let table: CauseTable = SharedTable::new();
        let id = CauseId {
            id: "test-skip-0001".into(),
        };
        table.insert(
            id.clone(),
            Cause::skip(make_skip(SkipEvaluation::Unconditional)),
        );
        assert!(matches!(table.get(&id).unwrap(), Cause::Skip(_)));
    }

    #[test]
    fn cause_table_multiple_causes() {
        let table: CauseTable = SharedTable::new();
        let id1 = CauseId {
            id: "a-b-0001".into(),
        };
        let id2 = CauseId {
            id: "c-d-0002".into(),
        };
        table.insert(
            id1.clone(),
            Cause::skip(make_skip(SkipEvaluation::Unconditional)),
        );
        table.insert(
            id2.clone(),
            Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        assert!(table.get(&id1).is_some());
        assert!(table.get(&id2).is_some());
    }

    #[test]
    fn cause_table_duplicate_id_first_insert_wins() {
        let table: CauseTable = SharedTable::new();
        let id = CauseId {
            id: "x-y-0000".into(),
        };
        table.insert(
            id.clone(),
            Cause::skip(make_skip(SkipEvaluation::Unconditional)),
        );
        table.insert(
            id.clone(),
            Cause::invalid(InvalidReport::PurityViolation { span: test_span() }),
        );
        // FrozenMap: first insert wins
        assert!(matches!(table.get(&id).unwrap(), Cause::Skip(_)));
    }

    #[test]
    fn warning_table_is_empty_initially() {
        let table: WarningTable = SharedTable::new();
        let id = WarningId {
            id: "test-0000".into(),
        };
        assert!(table.get(&id).is_none());
    }

    // ── Display formats ─────────────────────────────────────

    #[test]
    fn fn_id_display() {
        let id = FnId {
            module: ModulePath("lib/helpers".into()),
            name: "greet".into(),
            arity: 2,
        };
        assert_eq!(id.to_string(), "lib/helpers::greet/2");
    }

    #[test]
    fn effect_id_display() {
        let id = EffectId {
            module: ModulePath("lib/effects".into()),
            name: EffectName("StartDb".into()),
        };
        assert_eq!(id.to_string(), "lib/effects::StartDb");
    }
}
