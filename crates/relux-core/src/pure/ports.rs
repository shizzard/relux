//! Collision-resistant port allocation backing the `available_port()` BIF.
//!
//! Ports are handed out from the window between the privileged interval and
//! the OS ephemeral interval, so the kernel can never assign one of our ports
//! as an outbound source port. Each port is probed by binding it and tracked
//! against an owner (one per test execution) until the owner is released.
//! The probe binds `127.0.0.1` only, so the collision guarantee is
//! loopback-scoped: a service that binds a specific non-loopback interface
//! can still conflict with an allocated port.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Hard floor: never allocate below the historic privileged boundary.
pub const MIN_PORT: u16 = 1024;

/// Ephemeral-range start assumed when detection fails. Conservative:
/// at or below both the Linux default (32768) and macOS default (49152).
const FALLBACK_EPHEMERAL_START: u16 = 32768;

/// Opaque allocation-scope token. The runtime mints one per test execution
/// and releases it after the test's cleanup completes.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct PortOwner(u64);

/// Owner for allocations made outside any test execution (lowering-time
/// marker eval, `relux check`). Never released.
const PROCESS_OWNER: PortOwner = PortOwner(0);

/// Mint a fresh owner token. Callers use one owner per test execution: every
/// port allocated under it is freed together by a single `release` call
/// after that test's cleanup completes.
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

/// Failure modes for resolving or installing the allocation window.
#[derive(Debug, thiserror::Error)]
pub enum PortsError {
    #[error(
        "[available_ports] window {start}-{end} is empty; set range_start/range_end in Relux.toml to override"
    )]
    EmptyRange { start: u16, end: u16 },
    #[error("[available_ports] bounds must be at least 1024 (got {0})")]
    BelowMinimum(u16),
    #[error("port allocator already configured")]
    AlreadyConfigured,
}

/// Merge manifest overrides with detected defaults into the final inclusive
/// window. `detected_unprivileged` is the first non-privileged port;
/// `detected_ephemeral_start` is the first port of the OS ephemeral
/// interval (the window stays strictly below it).
/// The friendly manifest-time twin of these checks lives at
/// `config::AvailablePortsConfig::validate`.
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
fn first_number(s: &str) -> Option<u16> {
    s.split_whitespace().next()?.parse().ok()
}

/// Bind-probe a candidate port with default socket options (no
/// `SO_REUSEADDR`): a live listener or TIME_WAIT residue both disqualify
/// the candidate. Production probe used by `Allocator::new`; tests inject
/// a cheaper, deterministic substitute so they never touch a real socket.
fn probe_bind(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// The allocator proper: a scanning cursor over the window plus the
/// `port -> owner` registry. Global wiring lives in `configure`/`allocate`/
/// `release`; this struct is directly constructible for tests.
struct Allocator {
    range: PortRange,
    /// Next candidate to try; wraps within `range`.
    next: u16,
    owned: HashMap<u16, PortOwner>,
    /// Availability probe: `true` means the candidate is free to take.
    /// Production uses `probe_bind`; tests inject a fake so bookkeeping
    /// tests never touch a real socket.
    probe: fn(u16) -> bool,
}

impl Allocator {
    /// Production constructor: random start offset, so concurrent relux
    /// processes scan different parts of the window.
    fn new(range: PortRange) -> Self {
        use rand::RngExt;
        let width = u32::from(range.end) - u32::from(range.start) + 1;
        let offset = rand::rng().random_range(0..width) as u16;
        Self::with_start(range, range.start + offset, probe_bind)
    }

    /// Deterministic constructor for tests: fixed start position plus an
    /// injected probe, so tests control availability without real sockets.
    fn with_start(range: PortRange, start_at: u16, probe: fn(u16) -> bool) -> Self {
        debug_assert!((range.start..=range.end).contains(&start_at));
        Self {
            range,
            next: start_at,
            owned: HashMap::new(),
            probe,
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
            // The global mutex is held across this probe (a syscall in
            // production). A near-exhausted window means up to O(width)
            // binds under the lock in the worst case - an accepted bound,
            // not a bug, since the window is small (thousands of ports).
            if !(self.probe)(candidate) {
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

/// Floor a detected ephemeral-range start at `FALLBACK_EPHEMERAL_START`
/// whenever the reading is missing or claims the ephemeral range starts at
/// or below `MIN_PORT`. A kernel tuned to e.g.
/// `net.ipv4.ip_local_port_range = "1024 65535"` (real container/proxy
/// tuning) claims the whole port space as ephemeral, leaving no safe
/// default window; falling back to the conventional boundary beats
/// refusing to start - the bind-probe and owner registry still protect
/// against real collisions within whatever window results.
fn sane_ephemeral_start(reading: Option<u16>) -> u16 {
    match reading {
        Some(p) if p > MIN_PORT => p,
        _ => FALLBACK_EPHEMERAL_START,
    }
}

fn detect_ephemeral_start() -> u16 {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
        && let Some(p) = first_number(&s)
    {
        return sane_ephemeral_start(Some(p));
    }
    #[cfg(target_os = "macos")]
    if let Some(p) = sysctl_u16("net.inet.ip.portrange.first") {
        return sane_ephemeral_start(Some(p));
    }
    FALLBACK_EPHEMERAL_START
}

fn detect_unprivileged_start() -> u16 {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/sys/net/ipv4/ip_unprivileged_port_start")
        && let Some(p) = first_number(&s)
    {
        return p.max(MIN_PORT);
    }
    MIN_PORT
}

/// One-shot `sysctl -n <name>` read. Runs at most twice per process (both
/// detection calls happen once, at allocator initialization).
#[cfg(target_os = "macos")]
fn sysctl_u16(name: &str) -> Option<u16> {
    let out = std::process::Command::new("sysctl")
        .arg("-n")
        .arg(name)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    first_number(&String::from_utf8_lossy(&out.stdout))
}

static GLOBAL: OnceLock<Mutex<Allocator>> = OnceLock::new();

/// Install the allocation window from manifest overrides. Called once by the
/// CLI after manifest load, before any allocation. Errors if the resulting
/// window is empty or the allocator was already configured.
///
/// Must run before the first `allocate`/`release` call - the CLI calls this
/// first thing, before dispatching any BIF that could touch the allocator.
/// If `AlreadyConfigured` fires, it means lazy init (`global()`) already ran
/// ahead of `configure`, which indicates an internal call-ordering bug, not
/// a condition callers should recover from.
pub fn configure(start_override: Option<u16>, end_override: Option<u16>) -> Result<(), PortsError> {
    let range = resolve_range(
        start_override,
        end_override,
        detect_unprivileged_start(),
        detect_ephemeral_start(),
    )?;
    GLOBAL
        .set(Mutex::new(Allocator::new(range)))
        .map_err(|_| PortsError::AlreadyConfigured)
}

fn global() -> &'static Mutex<Allocator> {
    GLOBAL.get_or_init(|| {
        // Library/embedded use without configure(): detected defaults, and on
        // pathological sysctls fall back to the widest sane window.
        let range = resolve_range(
            None,
            None,
            detect_unprivileged_start(),
            detect_ephemeral_start(),
        )
        .unwrap_or(PortRange {
            start: MIN_PORT,
            end: FALLBACK_EPHEMERAL_START - 1,
        });
        Mutex::new(Allocator::new(range))
    })
}

/// Allocate a port for `owner` (`None` = process-lifetime, never released).
/// `None` result means the window is exhausted right now.
pub fn allocate(owner: Option<PortOwner>) -> Option<u16> {
    global()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .allocate(owner.unwrap_or(PROCESS_OWNER))
}

/// Free every port held by `owner`. Call only after the owning test's
/// cleanup has completed (the services bound to those ports are down).
pub fn release(owner: PortOwner) {
    if let Some(m) = GLOBAL.get() {
        m.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release(owner);
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
    fn sane_ephemeral_start_floors_when_no_safe_window_remains() {
        // Whole-port-space tuning (e.g. "1024 65535") reads back as 1024:
        // must not be trusted, or the default window collapses to empty.
        assert_eq!(sane_ephemeral_start(Some(1024)), FALLBACK_EPHEMERAL_START);
        assert_eq!(sane_ephemeral_start(Some(1023)), FALLBACK_EPHEMERAL_START);
        assert_eq!(sane_ephemeral_start(Some(20000)), 20000);
        assert_eq!(sane_ephemeral_start(None), FALLBACK_EPHEMERAL_START);
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

    /// Availability probe that treats every candidate as free. Used by
    /// bookkeeping tests that exercise cursor/owner logic, not real binding.
    fn always_free(_: u16) -> bool {
        true
    }

    #[test]
    fn allocator_never_repeats_within_owner() {
        let range = PortRange {
            start: 21710,
            end: 21714,
        };
        let mut a = Allocator::with_start(range, 21710, always_free);
        let owner = new_owner();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = a.allocate(owner) {
            assert!(p >= range.start && p <= range.end);
            assert!(seen.insert(p), "port {p} returned twice");
        }
        // Window of 5 fully consumable (probe always reports free).
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn allocator_skips_bound_port() {
        // Fake probe: every port free except 21721, which stands in for a
        // live listener or TIME_WAIT residue - no real socket involved.
        fn busy(p: u16) -> bool {
            p != 21721
        }
        let range = PortRange {
            start: 21720,
            end: 21722,
        };
        let mut a = Allocator::with_start(range, 21721, busy);
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
        let mut a = Allocator::with_start(range, 21730, always_free);
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
        let mut a = Allocator::with_start(range, 21740, always_free);
        let keep = new_owner();
        let drop_ = new_owner();
        let kept = a.allocate(keep).unwrap();
        let dropped = a.allocate(drop_).unwrap();
        assert_ne!(kept, dropped);
        a.release(drop_);
        // The kept port is still owned; only the dropped one came back.
        assert_eq!(a.allocate(new_owner()), Some(dropped));
        assert_eq!(a.allocate(new_owner()), None);
    }

    #[test]
    fn allocator_wraps_around_the_window() {
        let range = PortRange {
            start: 21750,
            end: 21752,
        };
        // Start scanning at the last port: wraparound must reach the first.
        let mut a = Allocator::with_start(range, 21752, always_free);
        let owner = new_owner();
        assert_eq!(a.allocate(owner), Some(21752));
        assert_eq!(a.allocate(owner), Some(21750));
        assert_eq!(a.allocate(owner), Some(21751));
    }

    #[test]
    fn allocator_new_random_offset_stays_in_window() {
        let range = PortRange {
            start: 21760,
            end: 21764,
        };
        for _ in 0..1000 {
            let a = Allocator::new(range);
            assert!(
                range.start <= a.next && a.next <= range.end,
                "offset {} out of window {:?}",
                a.next,
                range
            );
        }
    }

    // Only test on the real probe_bind path: covers actual binding via the
    // default window. Assert in-window/distinct only, never exact values -
    // the default window is environment-dependent.
    #[test]
    fn global_allocate_returns_distinct_valid_ports() {
        let a = allocate(None).expect("allocation from default window");
        let b = allocate(None).expect("allocation from default window");
        assert_ne!(a, b);
        assert!(a >= MIN_PORT);
        assert!(b >= MIN_PORT);
    }

    #[test]
    fn global_release_of_unknown_owner_is_a_noop() {
        // Must not panic. GLOBAL may already be initialized by another test
        // in this process (tests run in parallel and share the OnceLock).
        release(new_owner());
    }
}
