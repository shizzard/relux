use std::collections::HashMap;

use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

// ─── session/init ─────────────────────────────────────────

/// Incoming request for `session/init`.
#[derive(Debug, Deserialize, Serialize)]
pub struct SessionInitRequest {
    pub client: String,
    pub version: String,
}

/// Outgoing response for `session/init`.
#[derive(Debug, Builder, Serialize)]
pub struct SessionInitResponse {
    pub server: String,
    pub version: String,
    pub state: SessionState,
}

// ─── State snapshots ──────────────────────────────────────

/// Stage-specific state snapshot. The `stage` field is the enum
/// discriminant — serde embeds it as an internal tag, so the
/// stage and state are always in sync.
///
/// Used in `session/init` response and `stage/change` events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum SessionState {
    TestSelect(TestSelectState),
    PreRun(Box<PreRunState>),
}

/// State for the `test-select` stage.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct TestSelectState {
    pub project: String,
    pub files: Vec<SourceFileEntry>,
}

/// State for the `pre-run` stage. `frozen` is deferred and will be
/// added with the freeze-mode work item.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct PreRunState {
    pub source: PreRunSource,
    /// All env vars visible to the test: process env (`Env::capture`)
    /// plus the run-stable relux internals (`__RELUX_SHELL_PROMPT`,
    /// `__RELUX_SUITE_ROOT`, `__RELUX`). Per-run / per-test internals
    /// (`__RELUX_RUN_ID`, `__RELUX_RUN_ARTIFACTS`, `__RELUX_TEST_*`)
    /// materialize at the execution stage and are not in this map.
    pub env: HashMap<String, String>,
    /// Manifest-derived runtime configuration plus the effective debug
    /// timeout multiplier.
    pub config: PreRunConfig,
    /// Currently set breakpoints, keyed by suite-relative filename.
    /// Empty map if none. Populated by `breakpoint/set`; cleared by
    /// `breakpoint/unset`/`breakpoint/reset` and by re-selecting a
    /// different test.
    pub breakpoints: HashMap<String, Vec<Breakpoint>>,
}

/// Mirrors the `Config` block documented in `00-common.md`.
#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreRunConfig {
    pub shell: String,
    pub prompt: String,
    pub timeouts: PreRunTimeouts,
    pub timeout_multiplier: f64,
}

/// Timeout values, serialized as humantime-format strings (e.g. `"5s"`,
/// `"1m 40s"`, `"10m"`) — same shape the user wrote in `Relux.toml`.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct PreRunTimeouts {
    #[serde(rename = "match")]
    pub match_timeout: String,
    pub test: String,
    pub suite: String,
}

/// The resolved source graph for the selected test: the test's own file,
/// plus files containing reachable functions and effects. Files appear
/// in the bucket(s) matching the kinds of reachable definitions they
/// contain — a single file may appear in both `functions` and `effects`.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct PreRunSource {
    pub test: SourceFileEntry,
    pub functions: Vec<SourceFileEntry>,
    pub effects: Vec<SourceFileEntry>,
}

/// A loaded source file with its definitions. `content` is `None` in the
/// test-select stage — the client fetches it on demand via `source/get`.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct SourceFileEntry {
    pub filename: String,
    pub content: Option<String>,
    pub definitions: Vec<Definition>,
}

/// A typed, named span within a source file.
#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub kind: DefinitionKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    Test,
    Function,
    PureFunction,
    Effect,
}

/// A breakpoint at a specific line in a source file. Kept as a typed
/// struct (not a bare integer) so future fields like `condition` for
/// conditional breakpoints can be added without a wire-breaking change.
#[derive(Debug, Clone, Builder, Serialize)]
pub struct Breakpoint {
    pub line: usize,
}
