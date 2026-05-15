use std::collections::HashMap;
use std::sync::Arc;

use bytes::BytesMut;
use regex::Regex;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::observe::structured::BufferEventKind;
use crate::observe::structured::EventSeq;
use crate::observe::structured::StructuredLogBuilder;
use crate::observe::structured::Utf8Stream;
use crate::vm::context::FailPattern;

// ─── FailPatternHit ─────────────────────────────────────────────

/// A fail pattern matched in the output buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailPatternHit {
    /// The pattern string that was being watched for (regex source or literal).
    pub(crate) pattern: String,
    /// Whether `pattern` is a regex (`true`) or a literal substring (`false`).
    pub(crate) is_regex: bool,
    /// The actual text in the buffer that matched.
    pub(crate) matched_text: String,
}

// ─── MatchContext ───────────────────────────────────────────────

/// `(before, matched, after)` slices around a match. Used by the VM to push a
/// `BufferEventKind::Matched` describing how the cursor advanced.
///
/// All three strings carry the *full* bytes around the match, untruncated.
/// The viewer reconstructs each shell's append-only buffer from the `grew`
/// stream and validates that `before + matched + after` equals the
/// currently-unmatched buffer tail at the moment of the match.
pub type MatchContext = (String, String, String);

// ─── Tail truncation helpers (failure-context capture only) ────
// `match_context` does NOT use these — match events ship full bytes so the
// viewer can rebuild append-only history losslessly. These helpers are
// kept for `snapshot_tail` and other places that intentionally want a
// human-sized excerpt of the buffer.

fn truncate_before(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let start = s.ceil_char_boundary(s.len() - max);
        format!("...{}", &s[start..])
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

fn match_context(text: &str, pos: usize, end_pos: usize, matched: &str) -> MatchContext {
    (
        text[..pos].to_string(),
        matched.to_string(),
        text[end_pos..].to_string(),
    )
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
    utf8: Utf8Stream,
}

#[derive(Clone)]
pub struct OutputBuffer {
    inner: Arc<Mutex<BufferInner>>,
    pub(crate) notify: Arc<Notify>,
    /// Log builder used to emit buffer events (grew/matched/reset)
    /// while still holding the inner mutex. Optional so unit tests
    /// can construct an `OutputBuffer` without a log surface.
    log: Option<StructuredLogBuilder>,
    shell_name: String,
    shell_marker: String,
}

impl OutputBuffer {
    /// Construct an `OutputBuffer` wired to the given log builder.
    /// `append`/`consume_*`/`clear` will emit their corresponding
    /// buffer events on `log` while still holding the inner mutex,
    /// preventing a race between byte appends and event order.
    pub fn new(log: StructuredLogBuilder, shell_name: String, shell_marker: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                data: BytesMut::new(),
                base: 0,
                utf8: Utf8Stream::new(),
            })),
            notify: Arc::new(Notify::new()),
            log: Some(log),
            shell_name,
            shell_marker,
        }
    }

    /// Construct an `OutputBuffer` with no log surface — buffer-event
    /// emissions are silently dropped. Unit-test only.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                data: BytesMut::new(),
                base: 0,
                utf8: Utf8Stream::new(),
            })),
            notify: Arc::new(Notify::new()),
            log: None,
            shell_name: String::new(),
            shell_marker: String::new(),
        }
    }

    pub async fn append(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock().await;
        inner.data.extend_from_slice(bytes);
        let decoded = inner.utf8.feed(bytes);
        if !decoded.is_empty()
            && let Some(log) = &self.log
        {
            log.push_buffer_event(
                &self.shell_name,
                &self.shell_marker,
                BufferEventKind::Grew { data: decoded },
            );
        }
        drop(inner);
        self.notify.notify_waiters();
    }

    /// Find literal, drain via split_to, push the `Matched` buffer event
    /// while still holding the inner lock, and return the match plus the
    /// `EventSeq` of the just-pushed buffer event. All under one lock.
    pub async fn consume_literal(&self, needle: &str) -> Option<(Match<LiteralMatch>, EventSeq)> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let pos = text.find(needle)?;
        let end_pos = pos + needle.len();

        let (before, matched_str, after) = match_context(&text, pos, end_pos, needle);

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

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Some((m, buffer_seq))
    }

    /// Find regex, drain via split_to, push the `Matched` buffer event,
    /// and return the match plus the `EventSeq` of the just-pushed
    /// buffer event. All under one lock.
    ///
    /// Guards against partial-line matches: if the match ends at the buffer
    /// boundary and the buffer does not end with a newline, the last line may
    /// still be arriving. In that case we return `None` so the caller waits
    /// for more data rather than consuming an incomplete line.
    pub async fn consume_regex(&self, re: &Regex) -> Option<(Match<RegexMatch>, EventSeq)> {
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
        let (before, _, after) = match_context(&text, pos, end_pos, &matched_str);

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

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Some((m, buffer_seq))
    }

    /// Check fail pattern against buffer, then try to consume literal — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if literal consumed, Ok(None) if not found.
    /// On success the `Matched` buffer event is pushed before releasing the lock.
    pub async fn fail_check_consume_literal(
        &self,
        needle: &str,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<LiteralMatch>, EventSeq)>, FailPatternHit> {
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

        let (before, matched_str, after) = match_context(&text, pos, end_pos, needle);

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

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Ok(Some((m, buffer_seq)))
    }

    /// Check fail pattern against buffer, then try to consume regex — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if regex consumed, Ok(None) if not found.
    /// On success the `Matched` buffer event is pushed before releasing the lock.
    pub async fn fail_check_consume_regex(
        &self,
        re: &Regex,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<RegexMatch>, EventSeq)>, FailPatternHit> {
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
        let (before, _, after) = match_context(&text, pos, end_pos, &matched_str);

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

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Ok(Some((m, buffer_seq)))
    }

    /// Push a `Matched` buffer event on the log, if one is wired up.
    /// Returns the event seq (or `0` when no log is configured).
    fn emit_matched(&self, before: String, matched: String, after: String) -> EventSeq {
        if let Some(log) = &self.log {
            log.push_buffer_event(
                &self.shell_name,
                &self.shell_marker,
                BufferEventKind::Matched {
                    before,
                    matched,
                    after,
                },
            )
        } else {
            0
        }
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

    /// Drain all buffered data, advancing base. Emit a `Reset` buffer
    /// event with the discarded text (before releasing the lock).
    /// Returns the discarded text.
    pub async fn clear(&self) -> String {
        let mut inner = self.inner.lock().await;
        let len = inner.data.len();
        let chunk = inner.data.split_to(len);
        inner.base += len;
        let discarded = String::from_utf8_lossy(&chunk).to_string();
        if let Some(log) = &self.log {
            log.push_buffer_event(
                &self.shell_name,
                &self.shell_marker,
                BufferEventKind::Reset {
                    discarded: discarded.clone(),
                },
            );
        }
        discarded
    }

    /// Return the tail of the current buffer (last `n` chars) as a string.
    pub async fn snapshot_tail(&self, n: usize) -> String {
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        truncate_before(&text, n)
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
                is_regex: true,
                matched_text: m.as_str().to_string(),
            })
        }
        FailPattern::Literal(s) => {
            text.find(s.as_str())?;
            Some(FailPatternHit {
                pattern: s.clone(),
                is_regex: false,
                matched_text: s.clone(),
            })
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::observe::progress;
    use regex::RegexBuilder;

    /// Construct an `OutputBuffer` wired to a fresh `StructuredLogBuilder`,
    /// returning both so tests can assert on the buffer events that the
    /// `OutputBuffer` emits.
    fn wired_buffer() -> (
        OutputBuffer,
        StructuredLogBuilder,
        tokio::sync::mpsc::UnboundedReceiver<crate::observe::progress::ProgressEvent>,
    ) {
        let (tx, rx) = progress::channel();
        let sources = relux_core::table::SharedTable::new();
        let builder = StructuredLogBuilder::new(
            tx,
            Instant::now(),
            sources,
            Arc::from(PathBuf::from("/project").as_path()),
        );
        let buf = OutputBuffer::new(builder.clone(), "sh".into(), "m".into());
        (buf, builder, rx)
    }

    /// Inspect the last buffer event the builder accumulated.
    fn last_matched(builder: &StructuredLogBuilder) -> Option<(String, String, String)> {
        let events = builder.buffer_events_for_tests();
        events.last().and_then(|ev| match &ev.kind {
            BufferEventKind::Matched {
                before,
                matched,
                after,
            } => Some((before.clone(), matched.clone(), after.clone())),
            _ => None,
        })
    }

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

    // ── OutputBuffer::append / remaining ────────────────────────────

    #[tokio::test]
    async fn output_buffer_append_and_remaining() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello").await;
        assert_eq!(buf.remaining().await, b"hello");
    }

    #[tokio::test]
    async fn output_buffer_append_empty_bytes() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"").await;
        assert!(buf.remaining().await.is_empty());
    }

    // ── OutputBuffer::consume_literal ────────────────────────────────

    #[tokio::test]
    async fn consume_literal_basic() {
        let (buf, builder, _rx) = wired_buffer();
        buf.append(b"hello world").await;
        let (m, _buffer_seq) = buf.consume_literal("hello").await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        assert_eq!(m.consumed, 5);
        assert_eq!(m.value.0, "hello");
        let (before, matched, after) = last_matched(&builder).expect("matched event");
        assert_eq!(before, "");
        assert_eq!(matched, "hello");
        assert_eq!(after, " world");
        // Buffer should have " world" remaining
        assert_eq!(buf.remaining().await, b" world");
    }

    #[tokio::test]
    async fn consume_literal_drains_up_to_match_end() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"prefix MATCH suffix").await;
        let (m, _) = buf.consume_literal("MATCH").await.unwrap();
        assert_eq!(m.start, 7);
        assert_eq!(m.end, 12);
        assert_eq!(m.consumed, 12);
        assert_eq!(buf.remaining().await, b" suffix");
    }

    #[tokio::test]
    async fn consume_literal_not_found() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        assert!(buf.consume_literal("xyz").await.is_none());
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_literal_absolute_offsets_after_drain() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"aaa bbb ccc").await;
        let (m1, _) = buf.consume_literal("aaa").await.unwrap();
        assert_eq!(m1.start, 0);
        assert_eq!(m1.end, 3);
        let (m2, _) = buf.consume_literal("bbb").await.unwrap();
        assert_eq!(m2.start, 4);
        assert_eq!(m2.end, 7);
        assert_eq!(buf.remaining().await, b" ccc");
    }

    #[tokio::test]
    async fn consume_literal_context_carries_full_before_and_after() {
        let (buf, builder, _rx) = wired_buffer();
        let huge_prefix = "x".repeat(500);
        let huge_suffix = "y".repeat(500);
        buf.append(format!("{huge_prefix}MATCH{huge_suffix}").as_bytes())
            .await;
        let _ = buf.consume_literal("MATCH").await.unwrap();
        let (before, matched, after) = last_matched(&builder).expect("matched event");
        assert_eq!(before, huge_prefix);
        assert_eq!(matched, "MATCH");
        assert_eq!(after, huge_suffix);
    }

    // ── OutputBuffer::consume_regex ──────────────────────────────────

    #[tokio::test]
    async fn consume_regex_basic() {
        let buf = OutputBuffer::for_tests();
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
        let buf = OutputBuffer::for_tests();
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
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        let re = Regex::new(r"\d+").unwrap();
        assert!(buf.consume_regex(&re).await.is_none());
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_regex_absolute_offsets_after_drain() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"aaa 123 bbb 456\n").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m1, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m1.start, 4);
        assert_eq!(m1.end, 7);
        let (m2, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m2.start, 12);
        assert_eq!(m2.end, 15);
    }

    // ── Partial-line guard ─────────────────────────────────────────

    #[tokio::test]
    async fn consume_regex_defers_partial_line() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello wor").await;
        let re = RegexBuilder::new(r"^(.+)$")
            .multi_line(true)
            .build()
            .unwrap();
        assert!(buf.consume_regex(&re).await.is_none());
        assert_eq!(buf.remaining().await, b"hello wor");

        buf.append(b"ld\n").await;
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.value.0.get("0").unwrap(), "hello world");
    }

    #[tokio::test]
    async fn consume_regex_allows_match_before_partial_tail() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"first line\nsecond li").await;
        let re = RegexBuilder::new(r"^(.+)$")
            .multi_line(true)
            .build()
            .unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.value.0.get("1").unwrap(), "first line");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_defers_partial_line() {
        let buf = OutputBuffer::for_tests();
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
    async fn clear_empties_buffer_and_returns_discarded() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        let discarded = buf.clear().await;
        assert_eq!(discarded, "hello world");
        assert!(buf.remaining().await.is_empty());
    }

    #[tokio::test]
    async fn clear_advances_base_correctly() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        let _ = buf.clear().await;
        buf.append(b"abc 123\n").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        // base should be 11 (from clear) + 4 (from "abc ") = absolute offset 15
        assert_eq!(m.start, 15);
        assert_eq!(m.end, 18);
    }

    // ── OutputBuffer::snapshot_tail ─────────────────────────────────

    #[tokio::test]
    async fn snapshot_tail_returns_truncated_tail() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        let tail = buf.snapshot_tail(5).await;
        assert_eq!(tail, "...world");
    }

    #[tokio::test]
    async fn snapshot_tail_full_content_when_short() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hi").await;
        let tail = buf.snapshot_tail(80).await;
        assert_eq!(tail, "hi");
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
}
