//! Structured logging schema and accumulator.
//!
//! The `StructuredLog` produced here is the canonical artifact of a test run:
//! a spans glossary, a flat list of execution events, a parallel list of
//! buffer events, a shells glossary, and an optional failure record. Each
//! type derives `serde` (JSON-on-disk) and `ts-rs` (TypeScript declarations
//! consumed by the SPA viewer).
//!
//! TypeScript bindings are produced by enabling the `ts-export` cargo
//! feature on this crate and running the auto-injected
//! `export_bindings_*` tests; `just viewer-types` drives both.

pub mod artifact;
pub mod buffer;
pub mod builder;
pub mod event;
pub mod failure;
pub mod log_sink;
pub mod shell;
pub mod skip;
pub mod span;
pub mod utf8_stream;

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

pub use artifact::ArtifactEntry;
pub use buffer::BufferEvent;
pub use buffer::BufferEventKind;
pub use builder::StructuredLogBuilder;
pub use event::CancelReasonRecord;
pub use event::Event;
pub use event::EventKind;
pub use event::EventSeq;
pub use failure::CancellationRecord;
pub use failure::FailureRecord;
pub use failure::StackFrame;
pub use shell::ShellRecord;
pub use skip::SkipRecord;
pub use span::FnCallKind;
pub use span::MarkerEvalDecision;
pub use span::MarkerEvalDetail;
pub use span::MarkerEvalKind;
pub use span::MarkerEvalModifier;
pub use span::MatchKind;
pub use span::Span;
pub use span::SpanId;
pub use span::SpanKind;
pub use utf8_stream::Utf8Stream;

/// Source-file location resolved from an `IrSpan`. Lives on spans and stack
/// frames; events resolve against their span if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// Top-level structured log for a single test run. Produced by
/// `StructuredLogBuilder::build`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct StructuredLog {
    pub info: TestInfo,
    pub outcome: TestOutcome,
    pub env: EnvInfo,
    pub shells: HashMap<String, ShellRecord>,
    /// JSON-serializes `SpanId` keys as strings (per JSON object-key rules),
    /// so the TS type uses a string-keyed record rather than `bigint`-keyed.
    #[ts(as = "HashMap<String, Span>")]
    pub spans: HashMap<SpanId, Span>,
    pub events: Vec<Event>,
    pub buffer_events: Vec<BufferEvent>,
    /// `.relux` file contents referenced by any span's `location` or any
    /// event's `source`. Keys are relative paths matching `SourceLocation.file`.
    pub sources: HashMap<String, String>,
    /// Files written under the test's artifacts directory, sorted with
    /// `cmp_artifact_paths` (files before subdirs within each directory).
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct TestInfo {
    pub name: String,
    pub path: String,
    pub duration_ms: u64,
}

/// Tagged verdict carried by `StructuredLog`. Replaces the older pair of
/// `TestInfo.outcome: String` + `StructuredLog.failure: Option<_>` so the
/// schema cannot represent contradictory states.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
// Tag is `kind` (not `type`) because `FailureRecord` is itself a tagged
// enum on `type`; flattening with `tag = "type"` here would collide and
// collapse the TS-side narrowing to `never`.
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TestOutcome {
    Pass,
    Fail(FailureRecord),
    Cancelled(CancellationRecord),
    Skip(SkipRecord),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct EnvInfo {
    pub bootstrap: Vec<(String, String)>,
}

/// Serde helper that encodes `Duration` as fractional milliseconds (`f64`).
/// Matches what the viewer expects (`number` of milliseconds since test start).
pub(crate) mod ts_duration_ms {
    use std::time::Duration;

    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64() * 1000.0)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = f64::deserialize(d)?;
        Ok(Duration::from_secs_f64(ms / 1000.0))
    }
}

/// Same as `ts_duration_ms` but for `Option<Duration>`.
pub(crate) mod ts_duration_ms_opt {
    use std::time::Duration;

    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&(d.as_secs_f64() * 1000.0)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let opt = Option::<f64>::deserialize(d)?;
        Ok(opt.map(|ms| Duration::from_secs_f64(ms / 1000.0)))
    }
}
