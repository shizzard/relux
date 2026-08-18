//! Collision-resistant port allocation backing the `available_port()` BIF.
//!
//! Ports are handed out from the window between the privileged interval and
//! the OS ephemeral interval, so the kernel can never assign one of our ports
//! as an outbound source port. Each port is probed by binding it and tracked
//! against an owner (one per test execution) until the owner is released.
//! Design: docs/superpowers/specs/2026-08-18-available-port-allocator-design.md

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
}
