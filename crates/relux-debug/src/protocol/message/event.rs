use serde::Serialize;

use super::SessionState;

/// Server-pushed events delivered on the single `events/subscribe`
/// subscription. The `event` tag distinguishes event kinds.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// The session has transitioned to a new stage. Carries the full
    /// state snapshot for the new stage.
    StageChange { state: SessionState },
}
