use std::collections::HashMap;
use std::sync::Arc;

use bytes::BytesMut;
use regex::Regex;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::runtime::observe::event_log::BufferSnapshot;
use crate::runtime::vm::context::FailPattern;

// ─── FailPatternHit ─────────────────────────────────────────────

/// A fail pattern matched in the output buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailPatternHit {
    /// The pattern string that was being watched for (regex source or literal).
    pub(crate) pattern: String,
    /// The actual text in the buffer that matched.
    pub(crate) matched_text: String,
}

// ─── Constants ──────────────────────────────────────────────────

const BUFFER_PREFIX_LEN: usize = 40;
const BUFFER_SUFFIX_LEN: usize = 40;
pub(crate) const BUFFER_TAIL_LEN: usize = 80;

// ─── Truncation helpers ─────────────────────────────────────────

fn truncate_before(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let start = s.ceil_char_boundary(s.len() - max);
        format!("...{}", &s[start..])
    }
}

fn truncate_after(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max);
        format!("{}...", &s[..end])
    }
}

pub(crate) fn regex_error_summary(e: &regex::Error) -> String {
    let full = e.to_string();
    full.lines()
        .rev()
        .find(|l| !l.is_empty())
        .unwrap_or(&full)
        .strip_prefix("error: ")
        .unwrap_or(&full)
        .to_string()
}

// ─── Match Types ────────────────────────────────────────────────

/// Marker trait for match payload types.
pub trait MatchKind {}

/// Payload for a literal match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralMatch(pub String);
impl MatchKind for LiteralMatch {}

/// Payload for a regex match (capture groups by index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch(pub HashMap<String, String>);
impl MatchKind for RegexMatch {}

/// A match result with absolute byte offsets and typed payload.
#[derive(Debug, Clone)]
pub struct Match<T: MatchKind> {
    /// Absolute byte offset of match start (accounts for all prior truncations).
    pub start: usize,
    /// Absolute byte offset of match end.
    pub end: usize,
    /// Bytes consumed (everything up to and including the match, relative to current buffer).
    pub consumed: usize,
    /// The matched content.
    pub value: T,
}

// ─── OutputBuffer ───────────────────────────────────────────────

struct BufferInner {
    data: BytesMut,
    base: usize,
}

#[derive(Clone)]
pub struct OutputBuffer {
    inner: Arc<Mutex<BufferInner>>,
    pub(crate) notify: Arc<Notify>,
    recv_pending: Arc<Mutex<BytesMut>>,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                data: BytesMut::new(),
                base: 0,
            })),
            notify: Arc::new(Notify::new()),
            recv_pending: Arc::new(Mutex::new(BytesMut::new())),
        }
    }

    pub async fn append(&self, bytes: &[u8]) {
        self.inner.lock().await.data.extend_from_slice(bytes);
        self.recv_pending.lock().await.extend_from_slice(bytes);
        self.notify.notify_waiters();
    }

    pub async fn drain_recv(&self) -> Option<String> {
        let mut pending = self.recv_pending.lock().await;
        if pending.is_empty() {
            return None;
        }
        let bytes = pending.split();
        Some(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Find literal, extract truncated context, drain via split_to. One lock.
    /// Returns Match + BufferSnapshot for event emission.
    pub async fn consume_literal(
        &self,
        needle: &str,
    ) -> Option<(Match<LiteralMatch>, BufferSnapshot)> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let pos = text.find(needle)?;
        let end_pos = pos + needle.len();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: needle.to_string(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: LiteralMatch(needle.to_string()),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Some((m, snapshot))
    }

    /// Find regex, extract truncated context, drain via split_to. One lock.
    ///
    /// Guards against partial-line matches: if the match ends at the buffer
    /// boundary and the buffer does not end with a newline, the last line may
    /// still be arriving. In that case we return `None` so the caller waits
    /// for more data rather than consuming an incomplete line.
    pub async fn consume_regex(&self, re: &Regex) -> Option<(Match<RegexMatch>, BufferSnapshot)> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let cap = re.captures(&text)?;
        let whole = cap.get(0)?;
        let pos = whole.start();
        let end_pos = whole.end();

        if is_partial_line_match(re, end_pos, &text) {
            return None;
        }

        let matched_str = whole.as_str().to_string();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: matched_str.clone(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let mut captures = HashMap::new();
        for i in 0..cap.len() {
            if let Some(m) = cap.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: RegexMatch(captures),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Some((m, snapshot))
    }

    /// Check fail pattern against buffer, then try to consume literal — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if literal consumed, Ok(None) if not found.
    pub async fn fail_check_consume_literal(
        &self,
        needle: &str,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<LiteralMatch>, BufferSnapshot)>, FailPatternHit> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);

        // Check fail pattern first
        if let Some(fp) = fail_pattern
            && let Some(hit) = check_fail_in_buffer(&text, fp)
        {
            return Err(hit);
        }

        // Try to consume the literal
        let Some(pos) = text.find(needle) else {
            return Ok(None);
        };
        let end_pos = pos + needle.len();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: needle.to_string(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: LiteralMatch(needle.to_string()),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Ok(Some((m, snapshot)))
    }

    /// Check fail pattern against buffer, then try to consume regex — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if regex consumed, Ok(None) if not found.
    pub async fn fail_check_consume_regex(
        &self,
        re: &Regex,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<RegexMatch>, BufferSnapshot)>, FailPatternHit> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);

        // Check fail pattern first
        if let Some(fp) = fail_pattern
            && let Some(hit) = check_fail_in_buffer(&text, fp)
        {
            return Err(hit);
        }

        // Try to consume the regex
        let Some(cap) = re.captures(&text) else {
            return Ok(None);
        };
        let Some(whole) = cap.get(0) else {
            return Ok(None);
        };
        let pos = whole.start();
        let end_pos = whole.end();

        if is_partial_line_match(re, end_pos, &text) {
            return Ok(None);
        }

        let matched_str = whole.as_str().to_string();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: matched_str.clone(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let mut captures = HashMap::new();
        for i in 0..cap.len() {
            if let Some(m) = cap.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: RegexMatch(captures),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Ok(Some((m, snapshot)))
    }

    /// Check fail pattern against current buffer (peek only, no drain).
    pub async fn check_fail_pattern(
        &self,
        fail_pattern: Option<&FailPattern>,
    ) -> Option<FailPatternHit> {
        let fp = fail_pattern?;
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        check_fail_in_buffer(&text, fp)
    }

    /// Drain all buffered data, advancing base.
    pub async fn clear(&self) {
        let mut inner = self.inner.lock().await;
        let len = inner.data.len();
        let _ = inner.data.split_to(len);
        inner.base += len;
    }

    /// Return a BufferSnapshot::Tail of the current buffer (last `n` chars).
    pub async fn snapshot_tail(&self, n: usize) -> BufferSnapshot {
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        BufferSnapshot::Tail {
            content: truncate_before(&text, n),
        }
    }

    /// Return remaining unmatched buffer data.
    pub async fn remaining(&self) -> Vec<u8> {
        let inner = self.inner.lock().await;
        inner.data.to_vec()
    }
}

/// Returns `true` if a `$`-anchored regex matched at the buffer boundary
/// where the buffer does not end with a newline — meaning the last line may
/// still be arriving and `$` matched end-of-string rather than end-of-line.
///
/// Only applies when the regex source ends with an explicit `$` anchor.
/// Patterns without `$` (e.g. prompt matching with `^relux> `) are never
/// deferred, since they don't depend on line completeness.
fn is_partial_line_match(re: &Regex, match_end: usize, text: &str) -> bool {
    re.as_str().ends_with('$') && match_end == text.len() && !text.ends_with('\n')
}

/// Check if a fail pattern matches in the given text. Returns (pattern_str, matched_text).
fn check_fail_in_buffer(text: &str, pattern: &FailPattern) -> Option<FailPatternHit> {
    match pattern {
        FailPattern::Regex(re) => {
            let m = re.find(text)?;
            Some(FailPatternHit {
                pattern: re.as_str().to_string(),
                matched_text: m.as_str().to_string(),
            })
        }
        FailPattern::Literal(s) => {
            text.find(s.as_str())?;
            Some(FailPatternHit {
                pattern: s.clone(),
                matched_text: s.clone(),
            })
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use regex::RegexBuilder;

    // ── truncate_before ──────────────────────────────────────────────

    #[test]
    fn truncate_before_short_string_unchanged() {
        assert_eq!(truncate_before("hello", 10), "hello");
    }

    #[test]
    fn truncate_before_exact_length_unchanged() {
        assert_eq!(truncate_before("hello", 5), "hello");
    }

    #[test]
    fn truncate_before_keeps_last_n_chars() {
        assert_eq!(truncate_before("hello world", 5), "...world");
    }

    #[test]
    fn truncate_before_empty_string() {
        assert_eq!(truncate_before("", 5), "");
    }

    #[test]
    fn truncate_before_max_zero() {
        assert_eq!(truncate_before("hello", 0), "...");
    }

    // ── truncate_after ───────────────────────────────────────────────

    #[test]
    fn truncate_after_short_string_unchanged() {
        assert_eq!(truncate_after("hello", 10), "hello");
    }

    #[test]
    fn truncate_after_exact_length_unchanged() {
        assert_eq!(truncate_after("hello", 5), "hello");
    }

    #[test]
    fn truncate_after_keeps_first_n_chars() {
        assert_eq!(truncate_after("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_after_empty_string() {
        assert_eq!(truncate_after("", 5), "");
    }

    #[test]
    fn truncate_after_max_zero() {
        assert_eq!(truncate_after("hello", 0), "...");
    }

    // ── regex_error_summary ──────────────────────────────────────────

    #[test]
    #[allow(clippy::invalid_regex)]
    fn regex_error_summary_extracts_last_line() {
        let err = Regex::new("(unclosed").unwrap_err();
        let summary = regex_error_summary(&err);
        assert!(!summary.is_empty());
        assert!(!summary.starts_with("error: "));
    }

    #[test]
    #[allow(clippy::invalid_regex)]
    fn regex_error_summary_strips_error_prefix() {
        let err = Regex::new("[invalid").unwrap_err();
        let summary = regex_error_summary(&err);
        assert!(!summary.starts_with("error: "));
    }

    // ── OutputBuffer::new ────────────────────────────────────────────

    #[tokio::test]
    async fn output_buffer_new_is_empty() {
        let buf = OutputBuffer::new();
        assert!(buf.remaining().await.is_empty());
    }

    // ── OutputBuffer::append + remaining ─────────────────────────────

    #[tokio::test]
    async fn output_buffer_append_and_remaining() {
        let buf = OutputBuffer::new();
        buf.append(b"hello ").await;
        buf.append(b"world").await;
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn output_buffer_append_empty_bytes() {
        let buf = OutputBuffer::new();
        buf.append(b"").await;
        assert!(buf.remaining().await.is_empty());
    }

    // ── OutputBuffer::consume_literal ────────────────────────────────

    #[tokio::test]
    async fn consume_literal_basic() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let (m, snapshot) = buf.consume_literal("hello").await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        assert_eq!(m.consumed, 5);
        assert_eq!(m.value.0, "hello");
        assert!(matches!(snapshot, BufferSnapshot::Match { .. }));
        // Buffer should have " world" remaining
        assert_eq!(buf.remaining().await, b" world");
    }

    #[tokio::test]
    async fn consume_literal_drains_up_to_match_end() {
        let buf = OutputBuffer::new();
        buf.append(b"prefix MATCH suffix").await;
        let (m, _) = buf.consume_literal("MATCH").await.unwrap();
        assert_eq!(m.start, 7);
        assert_eq!(m.end, 12);
        assert_eq!(m.consumed, 12);
        assert_eq!(buf.remaining().await, b" suffix");
    }

    #[tokio::test]
    async fn consume_literal_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        assert!(buf.consume_literal("xyz").await.is_none());
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_literal_absolute_offsets_after_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"aaa bbb ccc").await;
        // Consume "aaa"
        let (m1, _) = buf.consume_literal("aaa").await.unwrap();
        assert_eq!(m1.start, 0);
        assert_eq!(m1.end, 3);
        // Now consume "bbb" — absolute offsets should account for drained bytes
        let (m2, _) = buf.consume_literal("bbb").await.unwrap();
        assert_eq!(m2.start, 4);
        assert_eq!(m2.end, 7);
        // Remaining should be " ccc"
        assert_eq!(buf.remaining().await, b" ccc");
    }

    #[tokio::test]
    async fn consume_literal_snapshot_has_truncated_context() {
        let buf = OutputBuffer::new();
        buf.append(b"before MATCH after").await;
        let (_, snapshot) = buf.consume_literal("MATCH").await.unwrap();
        match snapshot {
            BufferSnapshot::Match {
                before,
                matched,
                after,
            } => {
                assert_eq!(before, "before ");
                assert_eq!(matched, "MATCH");
                assert_eq!(after, " after");
            }
            _ => panic!("expected BufferSnapshot::Match"),
        }
    }

    // ── OutputBuffer::consume_regex ──────────────────────────────────

    #[tokio::test]
    async fn consume_regex_basic() {
        let buf = OutputBuffer::new();
        buf.append(b"abc 123 def").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert_eq!(m.value.0.get("0").unwrap(), "123");
        assert_eq!(buf.remaining().await, b" def");
    }

    #[tokio::test]
    async fn consume_regex_with_captures() {
        let buf = OutputBuffer::new();
        buf.append(b"name: Alice age: 30\n").await;
        let re = Regex::new(r"name: (\w+) age: (\d+)").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 19);
        assert_eq!(m.value.0.get("0").unwrap(), "name: Alice age: 30");
        assert_eq!(m.value.0.get("1").unwrap(), "Alice");
        assert_eq!(m.value.0.get("2").unwrap(), "30");
    }

    #[tokio::test]
    async fn consume_regex_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let re = Regex::new(r"\d+").unwrap();
        assert!(buf.consume_regex(&re).await.is_none());
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_regex_absolute_offsets_after_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"aaa 123 bbb 456\n").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m1, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m1.start, 4);
        assert_eq!(m1.end, 7);
        // After consuming "aaa 123", buffer has " bbb 456"
        let (m2, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m2.start, 12);
        assert_eq!(m2.end, 15);
    }

    // ── Partial-line guard ─────────────────────────────────────────

    #[tokio::test]
    async fn consume_regex_defers_partial_line() {
        // Simulate a partial delivery: no trailing newline
        let buf = OutputBuffer::new();
        buf.append(b"hello wor").await;
        let re = RegexBuilder::new(r"^(.+)$")
            .multi_line(true)
            .build()
            .unwrap();
        // Should defer — the line might not be complete yet
        assert!(buf.consume_regex(&re).await.is_none());
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"hello wor");

        // Now the rest arrives
        buf.append(b"ld\n").await;
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.value.0.get("0").unwrap(), "hello world");
    }

    #[tokio::test]
    async fn consume_regex_allows_match_before_partial_tail() {
        // A complete line followed by an incomplete one
        let buf = OutputBuffer::new();
        buf.append(b"first line\nsecond li").await;
        let re = RegexBuilder::new(r"^(.+)$")
            .multi_line(true)
            .build()
            .unwrap();
        // Should match the complete first line (match end < buffer len)
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.value.0.get("1").unwrap(), "first line");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_defers_partial_line() {
        let buf = OutputBuffer::new();
        buf.append(b"partial data").await;
        let re = RegexBuilder::new(r"^(.+)$")
            .multi_line(true)
            .build()
            .unwrap();
        let result = buf.fail_check_consume_regex(&re, None).await;
        assert!(result.unwrap().is_none());

        buf.append(b"\n").await;
        let result = buf.fail_check_consume_regex(&re, None).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0.get("0").unwrap(), "partial data");
    }

    // ── OutputBuffer::clear ─────────────────────────────────────────

    #[tokio::test]
    async fn clear_empties_buffer() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        buf.clear().await;
        assert!(buf.remaining().await.is_empty());
    }

    #[tokio::test]
    async fn clear_advances_base_correctly() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        buf.clear().await;
        buf.append(b"abc 123\n").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        // base should be 11 (from clear) + 4 (from "abc ") = absolute offset 15
        assert_eq!(m.start, 15);
        assert_eq!(m.end, 18);
    }

    // ── OutputBuffer::snapshot_tail ─────────────────────────────────

    #[tokio::test]
    async fn snapshot_tail_returns_tail() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let snapshot = buf.snapshot_tail(5).await;
        match snapshot {
            BufferSnapshot::Tail { content } => {
                assert_eq!(content, "...world");
            }
            _ => panic!("expected Tail"),
        }
    }

    #[tokio::test]
    async fn snapshot_tail_full_content_when_short() {
        let buf = OutputBuffer::new();
        buf.append(b"hi").await;
        let snapshot = buf.snapshot_tail(80).await;
        match snapshot {
            BufferSnapshot::Tail { content } => {
                assert_eq!(content, "hi");
            }
            _ => panic!("expected Tail"),
        }
    }

    // ── check_fail_in_buffer ────────────────────────────────────────

    #[test]
    fn check_fail_in_buffer_regex_match() {
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let hit = check_fail_in_buffer("some ERROR here", &fp).unwrap();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
    }

    #[test]
    fn check_fail_in_buffer_regex_no_match() {
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        assert!(check_fail_in_buffer("all good", &fp).is_none());
    }

    #[test]
    fn check_fail_in_buffer_literal_match() {
        let fp = FailPattern::Literal("FATAL".to_string());
        let hit = check_fail_in_buffer("got FATAL crash", &fp).unwrap();
        assert_eq!(hit.pattern, "FATAL");
        assert_eq!(hit.matched_text, "FATAL");
    }

    #[test]
    fn check_fail_in_buffer_literal_no_match() {
        let fp = FailPattern::Literal("FATAL".to_string());
        assert!(check_fail_in_buffer("all good", &fp).is_none());
    }

    // ── OutputBuffer::fail_check_consume_literal ────────────────────

    #[tokio::test]
    async fn fail_check_consume_literal_no_fail_pattern() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let result = buf.fail_check_consume_literal("hello", None).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0, "hello");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_fail_pattern_not_matched() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let result = buf.fail_check_consume_literal("hello", Some(&fp)).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0, "hello");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_fail_pattern_triggers() {
        let buf = OutputBuffer::new();
        buf.append(b"ERROR: something broke").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let result = buf.fail_check_consume_literal("broke", Some(&fp)).await;
        let hit = result.unwrap_err();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
        // Buffer unchanged — fail pattern short-circuits before consume
        assert_eq!(buf.remaining().await, b"ERROR: something broke");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_target_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let result = buf.fail_check_consume_literal("xyz", None).await;
        assert!(result.unwrap().is_none());
    }

    // ── OutputBuffer::fail_check_consume_regex ──────────────────────

    #[tokio::test]
    async fn fail_check_consume_regex_no_fail_pattern() {
        let buf = OutputBuffer::new();
        buf.append(b"abc 123 def").await;
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, None).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0.get("0").unwrap(), "123");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_fail_pattern_triggers() {
        let buf = OutputBuffer::new();
        buf.append(b"FATAL: abc 123").await;
        let fp = FailPattern::Literal("FATAL".to_string());
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, Some(&fp)).await;
        let hit = result.unwrap_err();
        assert_eq!(hit.pattern, "FATAL");
        assert_eq!(hit.matched_text, "FATAL");
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"FATAL: abc 123");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_target_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, None).await;
        assert!(result.unwrap().is_none());
    }

    // ── OutputBuffer::check_fail_pattern ─────────────────────────────

    #[tokio::test]
    async fn check_fail_pattern_none() {
        let buf = OutputBuffer::new();
        buf.append(b"ERROR here").await;
        assert!(buf.check_fail_pattern(None).await.is_none());
    }

    #[tokio::test]
    async fn check_fail_pattern_found() {
        let buf = OutputBuffer::new();
        buf.append(b"got ERROR output").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let hit = buf.check_fail_pattern(Some(&fp)).await.unwrap();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
    }

    #[tokio::test]
    async fn check_fail_pattern_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"all good").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        assert!(buf.check_fail_pattern(Some(&fp)).await.is_none());
    }
}
