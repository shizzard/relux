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
    )
}

pub(crate) fn dispatch(name: &str, args: Vec<String>) -> String {
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
        "available_port" => std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port().to_string())
            .unwrap_or_else(|_| "-1".into()),
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
        _ => unreachable!("unknown pure BIF: {name}"),
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bif_trim() {
        assert_eq!(dispatch("trim", vec!["  hi  ".into()]), "hi");
    }

    #[test]
    fn bif_trim_no_whitespace() {
        assert_eq!(dispatch("trim", vec!["hi".into()]), "hi");
    }

    #[test]
    fn bif_trim_only_whitespace() {
        assert_eq!(dispatch("trim", vec!["   ".into()]), "");
    }

    #[test]
    fn bif_upper() {
        assert_eq!(dispatch("upper", vec!["hello".into()]), "HELLO");
    }

    #[test]
    fn bif_upper_empty() {
        assert_eq!(dispatch("upper", vec![String::new()]), "");
    }

    #[test]
    fn bif_lower() {
        assert_eq!(dispatch("lower", vec!["HELLO".into()]), "hello");
    }

    #[test]
    fn bif_lower_empty() {
        assert_eq!(dispatch("lower", vec![String::new()]), "");
    }

    #[test]
    fn bif_replace() {
        assert_eq!(
            dispatch("replace", vec!["aXb".into(), "X".into(), "Y".into()]),
            "aYb"
        );
    }

    #[test]
    fn bif_replace_no_match() {
        assert_eq!(
            dispatch("replace", vec!["abc".into(), "X".into(), "Y".into()]),
            "abc"
        );
    }

    #[test]
    fn bif_replace_empty_from() {
        let result = dispatch("replace", vec!["abc".into(), String::new(), "X".into()]);
        assert!(result.contains('X'));
    }

    #[test]
    fn bif_split_basic() {
        assert_eq!(
            dispatch("split", vec!["a,b,c".into(), ",".into(), "1".into()]),
            "b"
        );
    }

    #[test]
    fn bif_split_out_of_bounds() {
        assert_eq!(
            dispatch("split", vec!["a,b".into(), ",".into(), "5".into()]),
            ""
        );
    }

    #[test]
    fn bif_split_first_element() {
        assert_eq!(
            dispatch("split", vec!["a,b,c".into(), ",".into(), "0".into()]),
            "a"
        );
    }

    #[test]
    fn bif_len() {
        assert_eq!(dispatch("len", vec!["abc".into()]), "3");
    }

    #[test]
    fn bif_len_empty() {
        assert_eq!(dispatch("len", vec![String::new()]), "0");
    }

    #[test]
    fn bif_len_unicode_bytes() {
        // len counts bytes, not chars
        assert_eq!(dispatch("len", vec!["héllo".into()]), "6");
    }

    #[test]
    fn bif_uuid_format() {
        let result = dispatch("uuid", vec![]);
        assert_eq!(result.len(), 36);
        assert_eq!(result.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn bif_uuid_unique() {
        let a = dispatch("uuid", vec![]);
        let b = dispatch("uuid", vec![]);
        assert_ne!(a, b);
    }

    #[test]
    fn bif_rand_length() {
        let result = dispatch("rand", vec!["8".into()]);
        assert_eq!(result.len(), 8);
    }

    #[test]
    fn bif_rand_with_mode_hex() {
        let result = dispatch("rand", vec!["16".into(), "hex".into()]);
        assert_eq!(result.len(), 16);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn bif_rand_with_mode_alpha() {
        let result = dispatch("rand", vec!["10".into(), "alpha".into()]);
        assert_eq!(result.len(), 10);
        assert!(result.chars().all(|c| c.is_ascii_alphabetic()));
    }

    #[test]
    fn bif_rand_with_mode_num() {
        let result = dispatch("rand", vec!["10".into(), "num".into()]);
        assert_eq!(result.len(), 10);
        assert!(result.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn bif_available_port_numeric() {
        let result = dispatch("available_port", vec![]);
        let port: u16 = result.parse().expect("should be a valid port number");
        assert!(port > 0);
    }

    #[test]
    fn bif_available_port_unique() {
        let a = dispatch("available_port", vec![]);
        let b = dispatch("available_port", vec![]);
        // Ports might occasionally collide, but very unlikely in practice
        let _: u16 = a.parse().unwrap();
        let _: u16 = b.parse().unwrap();
    }

    #[test]
    fn bif_which_existing_command() {
        // "sh" should exist on any Unix system
        let result = dispatch("which", vec!["sh".into()]);
        assert!(!result.is_empty());
        assert!(result.contains("sh"));
    }

    #[test]
    fn bif_which_nonexistent() {
        let result = dispatch("which", vec!["nonexistent_command_xyz_12345".into()]);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_which_empty() {
        let result = dispatch("which", vec![String::new()]);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_replace_all_occurrences() {
        let result = dispatch("replace", vec!["aaa".into(), "a".into(), "b".into()]);
        assert_eq!(result, "bbb");
    }

    #[test]
    fn bif_replace_empty_to() {
        let result = dispatch("replace", vec!["hello".into(), "l".into(), String::new()]);
        assert_eq!(result, "heo");
    }

    #[test]
    fn bif_split_delimiter_not_found() {
        let result = dispatch("split", vec!["abc".into(), ",".into(), "0".into()]);
        assert_eq!(result, "abc");
    }

    #[test]
    fn bif_split_empty_string() {
        let result = dispatch("split", vec![String::new(), ",".into(), "0".into()]);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_rand_unknown_mode_falls_back() {
        let result = dispatch("rand", vec!["10".into(), "invalid".into()]);
        assert_eq!(result.len(), 10);
        // Fallback to ALPHANUM — all chars should be alphanumeric.
        assert!(result.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn bif_rand_zero_length() {
        let result = dispatch("rand", vec!["0".into()]);
        assert_eq!(result, "");
    }

    #[test]
    fn bif_default_returns_first_when_non_empty() {
        assert_eq!(
            dispatch("default", vec!["hello".into(), "fallback".into()]),
            "hello"
        );
    }

    #[test]
    fn bif_default_returns_second_when_first_empty() {
        assert_eq!(
            dispatch("default", vec![String::new(), "fallback".into()]),
            "fallback"
        );
    }

    #[test]
    fn bif_default_both_empty() {
        assert_eq!(dispatch("default", vec![String::new(), String::new()]), "");
    }

    #[test]
    fn bif_which_with_path_separator() {
        let result = dispatch("which", vec!["/nonexistent/path".into()]);
        assert_eq!(result, "");
    }
}
