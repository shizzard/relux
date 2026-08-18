use chrono::DateTime;
use chrono::Utc;
use std::iter::Peekable;
use std::str::Chars;

const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUM: &[u8] = b"0123456789";
const ALPHANUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const HEX: &[u8] = b"0123456789abcdef";
const OCT: &[u8] = b"01234567";
const BIN: &[u8] = b"01";

fn random_string(len: usize, charset: &[u8]) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| charset[rng.random_range(0..charset.len())] as char)
        .collect()
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

/// The current instant as a UTC `DateTime`. Degrades to the epoch if the clock
/// is somehow before 1970 (not reachable in practice).
fn utc_now() -> DateTime<Utc> {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos()).unwrap_or_default()
}

/// GNU date-style strftime, with two deliberate deviations from chrono:
/// fractional seconds accept any width (`%1f`..`%9f`, `%.1f`..`%.9f`, not just
/// 3/6/9), and an unknown specifier is emitted verbatim instead of blanking the
/// output. Infallible. Pure in `dt` (no clock read) so tests can pin an instant.
fn strftime_utc(dt: DateTime<Utc>, fmt: &str) -> String {
    use std::fmt::Write;
    let nanos = format!("{:09}", dt.timestamp_subsec_nanos());
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let token = take_token(&mut chars);
        if let Some(frac) = fractional(&token, &nanos) {
            out.push_str(&frac);
            continue;
        }
        // Delegate the single token to chrono; emit it verbatim on any error
        // (unknown specifier -> passthrough). chrono's Display returns Err
        // rather than panicking, so write! surfaces it here.
        let mut rendered = String::new();
        if write!(rendered, "{}", dt.format(&token)).is_ok() {
            out.push_str(&rendered);
        } else {
            out.push_str(&token);
        }
    }
    out
}

/// Consume one `%` token: `%` (already seen) + a run of modifier/width chars +
/// one terminal char. Returns the token including the leading `%`.
fn take_token(chars: &mut Peekable<Chars>) -> String {
    let mut token = String::from('%');
    while let Some(&m) = chars.peek() {
        if matches!(m, '-' | '_' | '0' | '#' | '.' | ':') || m.is_ascii_digit() {
            token.push(m);
            chars.next();
        } else {
            break;
        }
    }
    if let Some(terminal) = chars.next() {
        token.push(terminal);
    }
    token
}

/// Render `%<N>f` / `%.<N>f` for any explicit width `N`, taking the first `N`
/// of the 9-digit zero-padded nanoseconds (right-padding zeros if `N > 9`).
/// Returns `None` for non-fractional tokens and for the width-less `%f` / `%.f`,
/// which the caller delegates to chrono. The `?`-guards make `%` (no terminal)
/// and `%f` fall through without any out-of-range slicing.
fn fractional(token: &str, nanos: &str) -> Option<String> {
    let mid = token.strip_prefix('%')?.strip_suffix('f')?;
    let (dot, digits) = match mid.strip_prefix('.') {
        Some(rest) => (".", rest),
        None => ("", mid),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: usize = digits.parse().unwrap_or(9);
    let mut s = String::from(dot);
    if n <= 9 {
        s.push_str(&nanos[..n]); // ASCII digits: byte slice is char-safe.
    } else {
        s.push_str(nanos);
        s.push_str(&"0".repeat(n - 9));
    }
    Some(s)
}

/// Returns true if the given (name, arity) pair is a pure built-in function.
pub fn is_pure_bif(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("trim", 1)
            | ("upper", 1)
            | ("lower", 1)
            | ("replace", 3)
            | ("split", 3)
            | ("len", 1)
            | ("uuid", 0)
            | ("rand", 1)
            | ("rand", 2)
            | ("available_port", 0)
            | ("which", 1)
            | ("default", 2)
            | ("mnemonic", 1)
            | ("sha1", 1)
            | ("timestamp", 1)
    )
}

pub fn dispatch(
    name: &str,
    args: Vec<String>,
    port_owner: Option<crate::pure::ports::PortOwner>,
) -> String {
    match name {
        "trim" => args[0].trim().to_string(),
        "upper" => args[0].to_uppercase(),
        "lower" => args[0].to_lowercase(),
        "replace" => args[0].replace(&args[1], &args[2]),
        "split" => {
            let index: usize = args[2].parse().unwrap_or(0);
            let parts: Vec<&str> = args[0].split(&args[1]).collect();
            parts.get(index).unwrap_or(&"").to_string()
        }
        "len" => args[0].len().to_string(),
        "uuid" => uuid::Uuid::new_v4().to_string(),
        "rand" => {
            let n: usize = args[0].parse().unwrap_or(0);
            if args.len() == 1 {
                random_string(n, ALPHANUM)
            } else {
                let charset = match args[1].as_str() {
                    "alpha" => ALPHA,
                    "num" => NUM,
                    "alphanum" => ALPHANUM,
                    "hex" => HEX,
                    "oct" => OCT,
                    "bin" => BIN,
                    _ => ALPHANUM,
                };
                random_string(n, charset)
            }
        }
        "available_port" => crate::pure::ports::allocate(port_owner)
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-1".into()),
        "which" => {
            let name = &args[0];
            if name.is_empty() {
                return String::new();
            }
            if name.contains(std::path::MAIN_SEPARATOR) {
                let path = std::path::Path::new(name.as_str());
                if is_executable(path) {
                    return path.to_string_lossy().into_owned();
                }
                return String::new();
            }
            let path_var = std::env::var("PATH").unwrap_or_default();
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name);
                if is_executable(&candidate) {
                    return candidate.to_string_lossy().into_owned();
                }
            }
            String::new()
        }
        "default" => {
            let mut it = args.into_iter();
            let first = it.next().unwrap();
            if first.is_empty() {
                it.next().unwrap()
            } else {
                first
            }
        }
        "mnemonic" => crate::diagnostics::format_mnemonic(crate::hash::stable_hash(&args[0])),
        "sha1" => {
            use sha1::Digest;
            use sha1::Sha1;
            use std::fmt::Write;
            let digest = Sha1::digest(args[0].as_bytes());
            let mut out = String::with_capacity(40);
            for byte in digest {
                // Lowercase, zero-padded, two ASCII hex chars per byte.
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
        "timestamp" => strftime_utc(utc_now(), &args[0]),
        _ => unreachable!("unknown pure BIF: {name}"),
    }
}

// --- Tests -----------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bif_trim() {
        assert_eq!(dispatch("trim", vec!["  hi  ".into()], None), "hi");
    }

    #[test]
    fn bif_trim_no_whitespace() {
        assert_eq!(dispatch("trim", vec!["hi".into()], None), "hi");
    }

    #[test]
    fn bif_trim_only_whitespace() {
        assert_eq!(dispatch("trim", vec!["   ".into()], None), "");
    }

    #[test]
    fn bif_upper() {
        assert_eq!(dispatch("upper", vec!["hello".into()], None), "HELLO");
    }

    #[test]
    fn bif_upper_empty() {
        assert_eq!(dispatch("upper", vec![String::new()], None), "");
    }

    #[test]
    fn bif_lower() {
        assert_eq!(dispatch("lower", vec!["HELLO".into()], None), "hello");
    }

    #[test]
    fn bif_lower_empty() {
        assert_eq!(dispatch("lower", vec![String::new()], None), "");
    }

    #[test]
    fn bif_replace() {
        assert_eq!(
            dispatch("replace", vec!["aXb".into(), "X".into(), "Y".into()], None),
            "aYb"
        );
    }

    #[test]
    fn bif_replace_no_match() {
        assert_eq!(
            dispatch("replace", vec!["abc".into(), "X".into(), "Y".into()], None),
            "abc"
        );
    }

    #[test]
    fn bif_replace_empty_from() {
        let result = dispatch(
            "replace",
            vec!["abc".into(), String::new(), "X".into()],
            None,
        );
        assert!(result.contains('X'));
    }

    #[test]
    fn bif_split_basic() {
        assert_eq!(
            dispatch("split", vec!["a,b,c".into(), ",".into(), "1".into()], None),
            "b"
        );
    }

    #[test]
    fn bif_split_out_of_bounds() {
        assert_eq!(
            dispatch("split", vec!["a,b".into(), ",".into(), "5".into()], None),
            ""
        );
    }

    #[test]
    fn bif_split_first_element() {
        assert_eq!(
            dispatch("split", vec!["a,b,c".into(), ",".into(), "0".into()], None),
            "a"
        );
    }

    #[test]
    fn bif_len() {
        assert_eq!(dispatch("len", vec!["abc".into()], None), "3");
    }

    #[test]
    fn bif_len_empty() {
        assert_eq!(dispatch("len", vec![String::new()], None), "0");
    }

    #[test]
    fn bif_len_unicode_bytes() {
        // len counts bytes, not chars
        assert_eq!(dispatch("len", vec!["h\u{e9}llo".into()], None), "6");
    }

    #[test]
    fn bif_uuid_format() {
        let result = dispatch("uuid", vec![], None);
        assert_eq!(result.len(), 36);
        assert_eq!(result.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn bif_uuid_unique() {
        let a = dispatch("uuid", vec![], None);
        let b = dispatch("uuid", vec![], None);
        assert_ne!(a, b);
    }

    #[test]
    fn bif_rand_length() {
        let result = dispatch("rand", vec!["8".into()], None);
        assert_eq!(result.len(), 8);
    }

    #[test]
    fn bif_rand_with_mode_hex() {
        let result = dispatch("rand", vec!["16".into(), "hex".into()], None);
        assert_eq!(result.len(), 16);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn bif_rand_with_mode_alpha() {
        let result = dispatch("rand", vec!["10".into(), "alpha".into()], None);
        assert_eq!(result.len(), 10);
        assert!(result.chars().all(|c| c.is_ascii_alphabetic()));
    }

    #[test]
    fn bif_rand_with_mode_num() {
        let result = dispatch("rand", vec!["10".into(), "num".into()], None);
        assert_eq!(result.len(), 10);
        assert!(result.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn bif_available_port_numeric_and_unique() {
        let a: u16 = dispatch("available_port", vec![], None)
            .parse()
            .expect("should be a valid port number");
        let b: u16 = dispatch("available_port", vec![], None)
            .parse()
            .expect("should be a valid port number");
        assert!(a >= 1024);
        assert!(b >= 1024);
        assert_ne!(a, b, "unreleased allocations must never repeat");
    }

    #[test]
    fn bif_which_existing_command() {
        // "sh" should exist on any Unix system
        let result = dispatch("which", vec!["sh".into()], None);
        assert!(!result.is_empty());
        assert!(result.contains("sh"));
    }

    #[test]
    fn bif_which_nonexistent() {
        let result = dispatch("which", vec!["nonexistent_command_xyz_12345".into()], None);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_which_empty() {
        let result = dispatch("which", vec![String::new()], None);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_replace_all_occurrences() {
        let result = dispatch("replace", vec!["aaa".into(), "a".into(), "b".into()], None);
        assert_eq!(result, "bbb");
    }

    #[test]
    fn bif_replace_empty_to() {
        let result = dispatch(
            "replace",
            vec!["hello".into(), "l".into(), String::new()],
            None,
        );
        assert_eq!(result, "heo");
    }

    #[test]
    fn bif_split_delimiter_not_found() {
        let result = dispatch("split", vec!["abc".into(), ",".into(), "0".into()], None);
        assert_eq!(result, "abc");
    }

    #[test]
    fn bif_split_empty_string() {
        let result = dispatch("split", vec![String::new(), ",".into(), "0".into()], None);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_rand_unknown_mode_falls_back() {
        let result = dispatch("rand", vec!["10".into(), "invalid".into()], None);
        assert_eq!(result.len(), 10);
        // Fallback to ALPHANUM - all chars should be alphanumeric.
        assert!(result.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn bif_rand_zero_length() {
        let result = dispatch("rand", vec!["0".into()], None);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_default_returns_first_when_non_empty() {
        assert_eq!(
            dispatch("default", vec!["hello".into(), "fallback".into()], None),
            "hello"
        );
    }

    #[test]
    fn bif_default_returns_second_when_first_empty() {
        assert_eq!(
            dispatch("default", vec![String::new(), "fallback".into()], None),
            "fallback"
        );
    }

    #[test]
    fn bif_default_both_empty() {
        assert_eq!(
            dispatch("default", vec![String::new(), String::new()], None),
            ""
        );
    }

    #[test]
    fn bif_which_with_path_separator() {
        let result = dispatch("which", vec!["/nonexistent/path".into()], None);
        assert_eq!(result, "");
    }

    #[test]
    fn mnemonic_is_well_formed_and_stable() {
        let a = dispatch("mnemonic", vec!["empay".into()], None);
        let b = dispatch("mnemonic", vec!["empay".into()], None);
        assert_eq!(a, b, "same input must yield same mnemonic");
        // adjective-noun-NNNN, all-lowercase words, 4-digit suffix.
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.len(), 3, "expected adjective-noun-NNNN, got {a}");
        assert!(parts[0].chars().all(|c| c.is_ascii_lowercase()));
        assert!(parts[1].chars().all(|c| c.is_ascii_lowercase()));
        assert_eq!(parts[2].len(), 4);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn mnemonic_distinguishes_inputs() {
        assert_ne!(
            dispatch("mnemonic", vec!["alpha".into()], None),
            dispatch("mnemonic", vec!["beta".into()], None)
        );
    }

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(
            dispatch("sha1", vec![String::new()], None),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            dispatch("sha1", vec!["abc".into()], None),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn hashing_bifs_are_pure() {
        assert!(is_pure_bif("mnemonic", 1));
        assert!(is_pure_bif("sha1", 1));
    }

    // --- timestamp -------------------------------------------

    fn fixed_dt() -> chrono::DateTime<chrono::Utc> {
        // 1970-01-01T00:00:00Z with a fixed subsecond so fractional output
        // is deterministic (nanos = 123456789).
        chrono::DateTime::from_timestamp(0, 123_456_789).unwrap()
    }

    #[test]
    fn timestamp_calendar_and_unix() {
        let dt = fixed_dt();
        assert_eq!(
            strftime_utc(dt, "%Y-%m-%dT%H:%M:%SZ"),
            "1970-01-01T00:00:00Z"
        );
        assert_eq!(strftime_utc(dt, "%s"), "0");
    }

    #[test]
    fn timestamp_fractional_widths() {
        let dt = fixed_dt();
        assert_eq!(strftime_utc(dt, "%3f"), "123");
        assert_eq!(strftime_utc(dt, "%6f"), "123456");
        assert_eq!(strftime_utc(dt, "%9f"), "123456789");
        assert_eq!(strftime_utc(dt, "%4f"), "1234"); // chrono-unsupported width
        assert_eq!(strftime_utc(dt, "%.4f"), ".1234"); // dotted, also unsupported
        assert_eq!(strftime_utc(dt, "%12f"), "123456789000"); // right-padded
    }

    #[test]
    fn timestamp_bare_fractional_delegates_to_chrono() {
        let dt = fixed_dt();
        assert_eq!(strftime_utc(dt, "%f"), "123456789");
        // fractional() must decline the width-less forms so they reach chrono.
        assert_eq!(fractional("%f", "123456789"), None);
        assert_eq!(fractional("%.f", "123456789"), None);
        assert_eq!(fractional("%4f", "123456789"), Some("1234".to_string()));
    }

    #[test]
    fn timestamp_literal_and_percent() {
        let dt = fixed_dt();
        assert_eq!(strftime_utc(dt, "literal"), "literal");
        assert_eq!(strftime_utc(dt, "%%"), "%");
    }

    #[test]
    fn timestamp_unknown_specifier_passes_through() {
        let dt = fixed_dt();
        assert_eq!(strftime_utc(dt, "%Y%Q"), "1970%Q");
        assert_eq!(strftime_utc(dt, "x%"), "x%"); // lone trailing % -> no panic
    }

    #[test]
    fn timestamp_clock_path_smoke() {
        let year = dispatch("timestamp", vec!["%Y".into()], None);
        assert_eq!(year.len(), 4);
        assert!(year.chars().all(|c| c.is_ascii_digit()));
        let secs = dispatch("timestamp", vec!["%s".into()], None);
        assert!(!secs.is_empty());
        assert!(secs.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn timestamp_is_pure() {
        assert!(is_pure_bif("timestamp", 1));
    }
}
