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
pub mod match_context;
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
pub use event::MultiMatchPattern;
pub use failure::CancellationRecord;
pub use failure::FailureRecord;
pub use failure::StackFrame;
pub use match_context::MatchContext;
pub use shell::ShellRecord;
pub use skip::SkipRecord;
pub use span::FnCallKind;
pub use span::MarkerEvalDecision;
pub use span::MarkerEvalDetail;
pub use span::MarkerEvalKind;
pub use span::MarkerEvalModifier;
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

/// `events.json` schema version. Bump on any change to the on-disk
/// shape (fields added, removed, or renamed; new tagged-enum variants;
/// a narrowed field meaning). External consumers should verify this
/// matches the version they expect.
pub const SCHEMA_VERSION: u32 = 3;

/// Top-level structured log for a single test run. Produced by
/// `StructuredLogBuilder::build`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct StructuredLog {
    /// Schema version of this artifact. See `SCHEMA_VERSION`.
    pub schema_version: u32,
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

/// Serializable mirror of `relux_core::pure::LayeredEnvSource` for the
/// structured log. The core type carries a `PathBuf` and derives neither
/// `serde` nor `ts-rs`, so the schema keeps its own tagged mirror; a
/// `DotEnv` path is lossily stringified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EnvSourceRecord {
    Base,
    DotEnv { path: String },
    ReluxInternal,
    EffectOverlay { mnemonic: String },
}

impl From<&relux_core::pure::LayeredEnvSource> for EnvSourceRecord {
    fn from(s: &relux_core::pure::LayeredEnvSource) -> Self {
        use relux_core::pure::LayeredEnvSource as S;
        match s {
            S::Base => Self::Base,
            S::DotEnv(p) => Self::DotEnv {
                path: p.to_string_lossy().into_owned(),
            },
            S::ReluxInternal => Self::ReluxInternal,
            S::EffectOverlay(m) => Self::EffectOverlay {
                mnemonic: m.clone(),
            },
        }
    }
}

/// One resolved environment entry in the bootstrap dump, tagged with the
/// provenance of the layer that supplied the winning value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct EnvValue {
    pub key: String,
    pub value: String,
    pub source: EnvSourceRecord,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct EnvInfo {
    pub bootstrap: Vec<EnvValue>,
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

#[cfg(test)]
mod env_provenance_tests {
    use super::*;

    #[test]
    fn env_source_record_serialises_dot_env() {
        let r = EnvSourceRecord::DotEnv {
            path: "/p/.env".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "dot-env", "path": "/p/.env"}));
    }

    #[test]
    fn env_source_record_variants_serialise() {
        assert_eq!(
            serde_json::to_value(EnvSourceRecord::Base).unwrap(),
            serde_json::json!({"kind": "base"})
        );
        assert_eq!(
            serde_json::to_value(EnvSourceRecord::ReluxInternal).unwrap(),
            serde_json::json!({"kind": "relux-internal"})
        );
        assert_eq!(
            serde_json::to_value(EnvSourceRecord::EffectOverlay {
                mnemonic: "brave-yak-0001".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "effect-overlay", "mnemonic": "brave-yak-0001"})
        );
    }

    #[test]
    fn env_source_record_from_core() {
        use relux_core::pure::LayeredEnvSource;
        let r: EnvSourceRecord = (&LayeredEnvSource::ReluxInternal).into();
        assert_eq!(r, EnvSourceRecord::ReluxInternal);
        let r: EnvSourceRecord = (&LayeredEnvSource::DotEnv("/a/.env".into())).into();
        assert_eq!(
            r,
            EnvSourceRecord::DotEnv {
                path: "/a/.env".into()
            }
        );
        let r: EnvSourceRecord = (&LayeredEnvSource::EffectOverlay("m".into())).into();
        assert_eq!(
            r,
            EnvSourceRecord::EffectOverlay {
                mnemonic: "m".into()
            }
        );
    }

    #[test]
    fn env_value_serialises() {
        let ev = EnvValue {
            key: "PORT".into(),
            value: "5432".into(),
            source: EnvSourceRecord::Base,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["key"], "PORT");
        assert_eq!(v["value"], "5432");
        assert_eq!(v["source"]["kind"], "base");
    }

    #[test]
    fn schema_version_is_three() {
        assert_eq!(SCHEMA_VERSION, 3);
    }
}
