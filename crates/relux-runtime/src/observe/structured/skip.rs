use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use super::event::EventSeq;
use super::span::MarkerEvalDetail;
use super::span::MarkerEvalKind;
use super::span::SpanId;

/// Self-contained pointer used by `TestOutcome::Skip` to identify which
/// marker triggered the skip. `span` is the `marker-eval` span; `event_seq`
/// is the `BoolCheck` event under it. The viewer focuses these at open
/// time and expands ancestors so the markers tree is unfolded.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
pub struct SkipRecord {
    pub span: SpanId,
    pub event_seq: EventSeq,
    pub marker_kind: MarkerEvalKind,
    pub evaluation: MarkerEvalDetail,
}
