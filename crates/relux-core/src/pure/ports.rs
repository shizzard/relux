//! Collision-resistant port allocation backing the `available_port()` BIF.
//!
//! Ports are handed out from the window between the privileged interval and
//! the OS ephemeral interval, so the kernel can never assign one of our ports
//! as an outbound source port. Each port is probed by binding it and tracked
//! against an owner (one per test execution) until the owner is released.
//! Design: docs/superpowers/specs/2026-08-18-available-port-allocator-design.md

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Hard floor: never allocate below the historic privileged boundary.
pub const MIN_PORT: u16 = 1024;

/// Opaque allocation-scope token. The runtime mints one per test execution
/// and releases it after the test's cleanup completes.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct PortOwner(u64);

pub fn new_owner() -> PortOwner {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PortOwner(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Inclusive allocation window: both `start` and `end` are mintable.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum PortsError {
    #[error("available_ports window {start}-{end} is empty")]
    EmptyRange { start: u16, end: u16 },
    #[error("available_ports bounds must be at least 1024 (got {0})")]
    BelowMinimum(u16),
    #[error("port allocator already configured")]
    AlreadyConfigured,
}

/// Merge manifest overrides with detected defaults into the final inclusive
/// window. `detected_unprivileged` is the first non-privileged port;
/// `detected_ephemeral_start` is the first port of the OS ephemeral
/// interval (the window stays strictly below it).
pub fn resolve_range(
    start_override: Option<u16>,
    end_override: Option<u16>,
    detected_unprivileged: u16,
    detected_ephemeral_start: u16,
) -> Result<PortRange, PortsError> {
    let start = start_override.unwrap_or(detected_unprivileged.max(MIN_PORT));
    let end = end_override.unwrap_or(detected_ephemeral_start.saturating_sub(1));
    if start < MIN_PORT {
        return Err(PortsError::BelowMinimum(start));
    }
    if end < start {
        return Err(PortsError::EmptyRange { start, end });
    }
    Ok(PortRange { start, end })
}

/// First integer in a whitespace-separated string, as `u16`. Parses both
/// `/proc/sys/net/ipv4/ip_local_port_range` ("32768\t60999") and
/// `/proc/sys/net/ipv4/ip_unprivileged_port_start` ("1024").
// Gains production callers (OS detection) in a later change; only
// exercised by tests for now.
#[allow(dead_code)]
fn first_number(s: &str) -> Option<u16> {
    s.split_whitespace().next()?.parse().ok()
}

/// The allocator proper: a scanning cursor over the window plus the
/// `port -> owner` registry. Global wiring lives in `configure`/`allocate`/
/// `release`; this struct is directly constructible for tests.
///
/// Gains a production caller (global wiring) in a later change; only
/// exercised via `with_start` by tests for now.
#[allow(dead_code)]
struct Allocator {
    range: PortRange,
    /// Next candidate to try; wraps within `range`.
    next: u16,
    owned: HashMap<u16, PortOwner>,
}

#[allow(dead_code)]
impl Allocator {
    /// Deterministic constructor for tests.
    fn with_start(range: PortRange, start_at: u16) -> Self {
        Self {
            range,
            next: start_at,
            owned: HashMap::new(),
        }
    }

    fn allocate(&mut self, owner: PortOwner) -> Option<u16> {
        let width = u32::from(self.range.end) - u32::from(self.range.start) + 1;
        for _ in 0..width {
            let candidate = self.next;
            self.next = if candidate == self.range.end {
                self.range.start
            } else {
                candidate + 1
            };
            if self.owned.contains_key(&candidate) {
                continue;
            }
            // Probe with default socket options (no SO_REUSEADDR): a live
            // listener or TIME_WAIT residue both disqualify the candidate.
            if TcpListener::bind(("127.0.0.1", candidate)).is_err() {
                continue;
            }
            self.owned.insert(candidate, owner);
            return Some(candidate);
        }
        None
    }

    fn release(&mut self, owner: PortOwner) {
        self.owned.retain(|_, o| *o != owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_range_defaults_to_detected_window() {
        let r = resolve_range(None, None, 1024, 32768).unwrap();
        assert_eq!(
            r,
            PortRange {
                start: 1024,
                end: 32767
            }
        );
    }

    #[test]
    fn resolve_range_start_override_keeps_detected_end() {
        let r = resolve_range(Some(20000), None, 1024, 32768).unwrap();
        assert_eq!(
            r,
            PortRange {
                start: 20000,
                end: 32767
            }
        );
    }

    #[test]
    fn resolve_range_end_override_keeps_detected_start() {
        let r = resolve_range(None, Some(29999), 1024, 32768).unwrap();
        assert_eq!(
            r,
            PortRange {
                start: 1024,
                end: 29999
            }
        );
    }

    #[test]
    fn resolve_range_clamps_detected_unprivileged_to_min() {
        // A container with ip_unprivileged_port_start=0 must not push the
        // window below 1024.
        let r = resolve_range(None, None, 0, 32768).unwrap();
        assert_eq!(r.start, 1024);
    }

    #[test]
    fn resolve_range_empty_window_is_an_error() {
        // range_start above the detected ephemeral boundary.
        let err = resolve_range(Some(40000), None, 1024, 32768).unwrap_err();
        assert!(matches!(
            err,
            PortsError::EmptyRange {
                start: 40000,
                end: 32767
            }
        ));
    }

    #[test]
    fn resolve_range_width_one_window_is_valid() {
        let r = resolve_range(Some(21700), Some(21700), 1024, 32768).unwrap();
        assert_eq!(
            r,
            PortRange {
                start: 21700,
                end: 21700
            }
        );
    }

    #[test]
    fn first_number_parses_pair_and_single() {
        assert_eq!(first_number("32768\t60999\n"), Some(32768));
        assert_eq!(first_number("1024\n"), Some(1024));
        assert_eq!(first_number(""), None);
        assert_eq!(first_number("garbage"), None);
    }

    #[test]
    fn owners_are_unique() {
        assert_ne!(new_owner(), new_owner());
    }

    #[test]
    fn allocator_never_repeats_within_owner() {
        let range = PortRange {
            start: 21710,
            end: 21714,
        };
        let mut a = Allocator::with_start(range, 21710);
        let owner = new_owner();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = a.allocate(owner) {
            assert!(p >= range.start && p <= range.end);
            assert!(seen.insert(p), "port {p} returned twice");
        }
        // Window of 5 fully consumable (assuming the fixed test range is free).
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn allocator_skips_bound_port() {
        let range = PortRange {
            start: 21720,
            end: 21722,
        };
        let _busy = TcpListener::bind(("127.0.0.1", 21721)).expect("bind fixture port");
        let mut a = Allocator::with_start(range, 21721);
        let owner = new_owner();
        let got: Vec<u16> = std::iter::from_fn(|| a.allocate(owner)).collect();
        assert!(!got.contains(&21721), "bound port must be skipped: {got:?}");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn allocator_release_makes_ports_reusable() {
        let range = PortRange {
            start: 21730,
            end: 21730,
        };
        let mut a = Allocator::with_start(range, 21730);
        let first = new_owner();
        assert_eq!(a.allocate(first), Some(21730));
        assert_eq!(a.allocate(first), None, "width-1 window exhausted");
        a.release(first);
        let second = new_owner();
        assert_eq!(a.allocate(second), Some(21730), "released port reusable");
    }

    #[test]
    fn allocator_release_keeps_other_owners_ports() {
        let range = PortRange {
            start: 21740,
            end: 21741,
        };
        let mut a = Allocator::with_start(range, 21740);
        let keep = new_owner();
        let drop_ = new_owner();
        let kept = a.allocate(keep).unwrap();
        let dropped = a.allocate(drop_).unwrap();
        a.release(drop_);
        // The kept port is still owned; only the dropped one came back.
        assert_eq!(a.allocate(new_owner()), Some(dropped));
        assert_eq!(a.allocate(new_owner()), None);
        let _ = kept;
    }

    #[test]
    fn allocator_wraps_around_the_window() {
        let range = PortRange {
            start: 21750,
            end: 21752,
        };
        // Start scanning at the last port: wraparound must reach the first.
        let mut a = Allocator::with_start(range, 21752);
        let owner = new_owner();
        assert_eq!(a.allocate(owner), Some(21752));
        assert_eq!(a.allocate(owner), Some(21750));
        assert_eq!(a.allocate(owner), Some(21751));
    }
}
