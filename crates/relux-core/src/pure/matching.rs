//! One-shot string matching against a complete value - the shared
//! primitive behind marker conditions and (later) pure-match statements.
//! Literal mode is substring-contains; regex mode is an unanchored search
//! with numeric-keyed captures. Pure: no I/O, no observability.

use std::collections::HashMap;

/// A successful match. `matched_text` is the whole-match substring
/// (regex group 0, or the literal needle). `captures` holds numeric-keyed
/// regex groups (`"0"`..`"n"`); always empty for a literal match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PureMatchHit {
    pub matched_text: String,
    pub captures: HashMap<String, String>,
}

/// A pattern that failed to compile. Only reachable in regex mode with an
/// interpolated pattern; constant patterns are validated at lowering time.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid match pattern `{pattern}`: {reason}")]
pub struct PureMatchError {
    pub pattern: String,
    pub reason: String,
}

/// Match `value` against `pattern`. `Ok(None)` is a clean no-match;
/// `Ok(Some)` a hit; `Err` a malformed regex (regex mode only).
pub fn pure_match(
    value: &str,
    pattern: &str,
    is_regex: bool,
) -> Result<Option<PureMatchHit>, PureMatchError> {
    if is_regex {
        let re = regex::Regex::new(pattern).map_err(|e| PureMatchError {
            pattern: pattern.to_string(),
            reason: e.to_string(),
        })?;
        let Some(caps) = re.captures(value) else {
            return Ok(None);
        };
        let mut captures = HashMap::new();
        for i in 0..caps.len() {
            if let Some(m) = caps.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }
        let matched_text = caps
            .get(0)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        Ok(Some(PureMatchHit {
            matched_text,
            captures,
        }))
    } else if value.contains(pattern) {
        Ok(Some(PureMatchHit {
            matched_text: pattern.to_string(),
            captures: HashMap::new(),
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_contains_hit_has_needle_and_no_captures() {
        let hit = pure_match("ubuntu-linux", "linux", false).unwrap().unwrap();
        assert_eq!(hit.matched_text, "linux");
        assert!(hit.captures.is_empty());
    }

    #[test]
    fn literal_contains_miss_is_none() {
        assert!(pure_match("darwin", "linux", false).unwrap().is_none());
    }

    #[test]
    fn regex_hit_populates_numeric_captures() {
        let hit = pure_match("id=42", r"id=(\d+)", true).unwrap().unwrap();
        assert_eq!(hit.matched_text, "id=42");
        assert_eq!(hit.captures.get("0").map(String::as_str), Some("id=42"));
        assert_eq!(hit.captures.get("1").map(String::as_str), Some("42"));
    }

    #[test]
    fn regex_is_unanchored() {
        // No anchors: matches a substring, like marker `?` today.
        let hit = pure_match("x-linux-y", "linux", true).unwrap().unwrap();
        assert_eq!(hit.matched_text, "linux");
    }

    #[test]
    fn regex_miss_is_none() {
        assert!(pure_match("abc", r"^\d+$", true).unwrap().is_none());
    }

    #[test]
    fn malformed_regex_is_err() {
        let err = pure_match("abc", "(", true).unwrap_err();
        assert_eq!(err.pattern, "(");
        assert!(!err.reason.is_empty());
    }

    #[test]
    fn literal_never_errors_on_regex_metachars() {
        // A bare `(` is a literal needle here, not a regex - no error.
        assert!(pure_match("a(b", "(", false).unwrap().is_some());
    }
}
