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
#[derive(Debug, Serialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum SessionState {
    TestSelect(TestSelectState),
}

/// State for the `test-select` stage.
#[derive(Debug, Builder, Serialize)]
pub struct TestSelectState {
    pub project: String,
    pub files: Vec<SourceFileEntry>,
}

/// A loaded source file with its definitions. `content` is `None` in the
/// test-select stage — the client fetches it on demand via `source/get`.
#[derive(Debug, Builder, Serialize)]
pub struct SourceFileEntry {
    pub filename: String,
    pub content: Option<String>,
    pub definitions: Vec<Definition>,
}

/// A typed, named span within a source file.
#[derive(Debug, Builder, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub kind: DefinitionKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    Test,
    Function,
    PureFunction,
    Effect,
}
