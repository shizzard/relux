//! Cancellation primitives shared by the run scheduler, per-test watchdog,
//! VM, BIFs, and effect manager. Wraps `tokio_util::sync::CancellationToken`
//! with a per-token reason slot so observers can answer "why was I
//! cancelled?" without needing a separate side channel.

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum CancelReason {
    /// The per-test watchdog flipped this token because the test exceeded
    /// its `effective_timeout`.
    TestTimeout { duration: Duration },
    /// The suite-wide watchdog fired.
    SuiteTimeout { duration: Duration },
    /// A sibling test failed and `RunStrategy::FailFast` is active.
    FailFast { trigger_test: String },
    /// The CLI process received SIGINT.
    Sigint,
}

#[derive(Debug, Clone)]
pub struct CancelToken {
    inner: CancellationToken,
    reason: Arc<OnceLock<CancelReason>>,
    parent_reason: Option<Arc<OnceLock<CancelReason>>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            inner: CancellationToken::new(),
            reason: Arc::new(OnceLock::new()),
            parent_reason: None,
        }
    }

    /// Derive a child token. Observes the parent's cancellation via
    /// `tokio_util`'s `child_token`; falls back to the parent's reason slot
    /// when its own slot is empty. Setting the child's reason via
    /// `cancel_with` does not propagate to the parent.
    pub fn child(&self) -> Self {
        Self {
            inner: self.inner.child_token(),
            reason: Arc::new(OnceLock::new()),
            parent_reason: Some(self.reason.clone()),
        }
    }

    /// Set the local reason (first writer wins) and flip the cancel flag.
    pub fn cancel_with(&self, reason: CancelReason) {
        let _ = self.reason.set(reason);
        self.inner.cancel();
    }

    #[cfg(test)]
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    pub fn reason(&self) -> Option<CancelReason> {
        self.reason
            .get()
            .cloned()
            .or_else(|| self.parent_reason.as_ref().and_then(|p| p.get().cloned()))
    }

    pub async fn cancelled(&self) {
        self.inner.cancelled().await
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_with_sets_reason_and_flag() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        assert!(t.reason().is_none());
        t.cancel_with(CancelReason::Sigint);
        assert!(t.is_cancelled());
        assert!(matches!(t.reason(), Some(CancelReason::Sigint)));
    }

    #[test]
    fn cancel_with_first_writer_wins() {
        let t = CancelToken::new();
        t.cancel_with(CancelReason::Sigint);
        t.cancel_with(CancelReason::FailFast {
            trigger_test: "x".into(),
        });
        assert!(matches!(t.reason(), Some(CancelReason::Sigint)));
    }

    #[test]
    fn clone_shares_local_slot() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel_with(CancelReason::Sigint);
        assert!(b.is_cancelled());
        assert!(matches!(b.reason(), Some(CancelReason::Sigint)));
    }

    #[test]
    fn child_observes_parent_cancel_and_reason() {
        let parent = CancelToken::new();
        let child = parent.child();
        assert!(!child.is_cancelled());
        parent.cancel_with(CancelReason::SuiteTimeout {
            duration: Duration::from_secs(1),
        });
        assert!(child.is_cancelled());
        match child.reason() {
            Some(CancelReason::SuiteTimeout { duration }) => {
                assert_eq!(duration, Duration::from_secs(1));
            }
            other => panic!("unexpected reason: {other:?}"),
        }
    }

    #[test]
    fn child_local_reason_does_not_bubble_up() {
        let parent = CancelToken::new();
        let child = parent.child();
        child.cancel_with(CancelReason::TestTimeout {
            duration: Duration::from_millis(300),
        });
        assert!(child.is_cancelled());
        assert!(matches!(
            child.reason(),
            Some(CancelReason::TestTimeout { .. })
        ));
        assert!(!parent.is_cancelled());
        assert!(parent.reason().is_none());
    }

    #[test]
    fn child_local_reason_preferred_over_parent_fallback() {
        let parent = CancelToken::new();
        let child = parent.child();
        parent.cancel_with(CancelReason::FailFast {
            trigger_test: "p".into(),
        });
        child.cancel_with(CancelReason::TestTimeout {
            duration: Duration::from_millis(100),
        });
        assert!(matches!(
            child.reason(),
            Some(CancelReason::TestTimeout { .. })
        ));
        assert!(matches!(
            parent.reason(),
            Some(CancelReason::FailFast { .. })
        ));
    }

    #[test]
    fn cfg_test_cancel_flips_flag_without_reason() {
        let t = CancelToken::new();
        t.cancel();
        assert!(t.is_cancelled());
        assert!(t.reason().is_none());
    }
}
