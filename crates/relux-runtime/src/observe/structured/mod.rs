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

pub mod buffer;
pub mod builder;
pub mod event;
pub mod failure;
pub mod shell;
pub mod span;
pub mod utf8_stream;

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

pub use buffer::BufferEvent;
pub use buffer::BufferEventKind;
pub use builder::StructuredLogBuilder;
pub use event::Event;
pub use event::EventKind;
pub use event::EventSeq;
pub use failure::FailureRecord;
pub use failure::StackFrame;
pub use shell::ShellRecord;
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
    pub test: TestInfo,
    pub env: EnvInfo,
    pub shells: HashMap<String, ShellRecord>,
    pub spans: HashMap<SpanId, Span>,
    pub events: Vec<Event>,
    pub buffer_events: Vec<BufferEvent>,
    pub failure: Option<FailureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct TestInfo {
    pub name: String,
    pub path: String,
    pub outcome: String,
    pub duration_ms: u64,
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
