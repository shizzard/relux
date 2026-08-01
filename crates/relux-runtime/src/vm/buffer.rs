use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use std::sync::Mutex;
use tokio::sync::Notify;

use crate::observe::structured::BufferEventKind;
use crate::observe::structured::EventSeq;
use crate::observe::structured::StructuredLogBuilder;
use crate::observe::structured::Utf8Stream;
use crate::vm::context::FailPattern;

// --- FailPatternHit --------------------------------------

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

// --- MatchSlices -----------------------------------------

/// `(before, matched, after)` slices around a match. Used by the VM to push a
/// `BufferEventKind::Matched` describing how the cursor advanced.
///
/// All three strings carry the *full* bytes around the match, untruncated.
/// The viewer reconstructs each shell's append-only buffer from the `grew`
/// stream and validates that `before + matched + after` equals the
/// currently-unmatched buffer tail at the moment of the match.
pub type MatchSlices = (String, String, String);

// --- Tail truncation helpers (failure-context capture only) ---
// `match_slices` does NOT use these - match events ship full bytes so the
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

fn match_slices(text: &str, pos: usize, end_pos: usize, matched: &str) -> MatchSlices {
    (
        text[..pos].to_string(),
        matched.to_string(),
        text[end_pos..].to_string(),
    )
}

// --- Match Types -----------------------------------------

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

// --- Multimatch types ------------------------------------

/// A pattern the multimatch scan is still looking for.
/// `regex` is `None` for literal patterns; the source string lives in
/// `pattern_str` either way so the builder can record it without going
/// back to the IR.
#[derive(Debug, Clone)]
pub struct PatternSlot {
    pub(crate) pattern_str: String,
    pub(crate) regex: Option<Regex>,
}

impl PatternSlot {
    pub fn literal(needle: String) -> Self {
        Self {
            pattern_str: needle,
            regex: None,
        }
    }

    pub fn regex(source: String, compiled: Regex) -> Self {
        Self {
            pattern_str: source,
            regex: Some(compiled),
        }
    }

    pub fn is_regex(&self) -> bool {
        self.regex.is_some()
    }

    pub fn pattern(&self) -> &str {
        &self.pattern_str
    }
}

/// Result of a successful scan against one pattern slot.
#[derive(Debug, Clone)]
pub struct MultiMatchHit {
    /// The full bytes that matched. Equal to `whole.as_str()` for regex,
    /// equal to the needle for literal.
    pub matched_text: String,
    /// Absolute byte offset of match start (accounts for all prior drains).
    pub start_abs: usize,
    /// Absolute byte offset of match end.
    pub end_abs: usize,
    /// `before` slice (the prefix of `decoded` ahead of the match) captured
    /// at the moment the scan ran. Used by the caller when emitting the
    /// per-pattern `Matched` buffer event.
    pub before: String,
    /// `after` slice (the suffix of `decoded` after the match).
    pub after: String,
}

// --- OutputBuffer ----------------------------------------

struct BufferInner {
    /// Cleanly-decoded bytes available for matching. Always valid UTF-8.
    /// Invalid input bytes are surfaced as `U+FFFD` here via `Utf8Stream`,
    /// so byte offsets and char-aware slicing coincide for drains.
    decoded: String,
    /// Absolute byte offset (in decoded coordinates) of the first byte
    /// currently held in `decoded`. Advanced on every drain; used to
    /// compute `Match.start` / `Match.end` across the shell's lifetime.
    base: usize,
    /// Streaming UTF-8 decoder; holds back the trailing bytes of any
    /// incomplete multi-byte sequence until the next `append`.
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
    /// `append`/`consume_*`/`clear` emit their corresponding buffer events
    /// on `log` while still holding the inner mutex, preventing a race
    /// between byte appends and event order. The inner mutex is a
    /// `std::sync::Mutex` - every critical section is pure CPU work, so no
    /// `.await` ever happens under the guard and a blocking lock is correct.
    pub fn new(log: StructuredLogBuilder, shell_name: String, shell_marker: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                decoded: String::new(),
                base: 0,
                utf8: Utf8Stream::new(),
            })),
            notify: Arc::new(Notify::new()),
            log: Some(log),
            shell_name,
            shell_marker,
        }
    }

    /// Construct an `OutputBuffer` with no log surface - buffer-event
    /// emissions are silently dropped. Unit-test only.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                decoded: String::new(),
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
        let mut inner = self.inner.lock().unwrap();
        let decoded = inner.utf8.feed(bytes);
        if !decoded.is_empty() {
            inner.decoded.push_str(&decoded);
            if let Some(log) = &self.log {
                log.push_buffer_event(
                    &self.shell_name,
                    &self.shell_marker,
                    BufferEventKind::Grew { data: decoded },
                );
            }
        }
        drop(inner);
        self.notify.notify_waiters();
    }

    /// Find literal, drain the decoded prefix up to the match end, push the
    /// `Matched` buffer event while still holding the inner lock, and return
    /// the match plus the `EventSeq` of the just-pushed buffer event. All
    /// under one lock.
    pub async fn consume_literal(&self, needle: &str) -> Option<(Match<LiteralMatch>, EventSeq)> {
        let mut inner = self.inner.lock().unwrap();
        let pos = inner.decoded.find(needle)?;
        let end_pos = pos + needle.len();

        let (before, matched_str, after) = match_slices(&inner.decoded, pos, end_pos, needle);

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: LiteralMatch(needle.to_string()),
        };

        inner.decoded.drain(..end_pos);
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
        let mut inner = self.inner.lock().unwrap();
        let (pos, end_pos, matched_str, captures) = {
            let cap = re.captures(&inner.decoded)?;
            let whole = cap.get(0)?;
            let pos = whole.start();
            let end_pos = whole.end();
            if is_partial_line_match(re, end_pos, &inner.decoded) {
                return None;
            }
            let matched_str = whole.as_str().to_string();
            let mut captures = HashMap::new();
            for i in 0..cap.len() {
                if let Some(m) = cap.get(i) {
                    captures.insert(i.to_string(), m.as_str().to_string());
                }
            }
            (pos, end_pos, matched_str, captures)
        };

        let (before, _, after) = match_slices(&inner.decoded, pos, end_pos, &matched_str);

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: RegexMatch(captures),
        };

        inner.decoded.drain(..end_pos);
        inner.base += end_pos;

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Some((m, buffer_seq))
    }

    /// Check fail pattern against buffer, then try to consume literal - under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if literal consumed, Ok(None) if not found.
    /// On success the `Matched` buffer event is pushed before releasing the lock.
    pub async fn fail_check_consume_literal(
        &self,
        needle: &str,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<LiteralMatch>, EventSeq)>, FailPatternHit> {
        let mut inner = self.inner.lock().unwrap();

        // Check fail pattern first
        if let Some(fp) = fail_pattern
            && let Some(hit) = check_fail_in_buffer(&inner.decoded, fp)
        {
            return Err(hit);
        }

        // Try to consume the literal
        let Some(pos) = inner.decoded.find(needle) else {
            return Ok(None);
        };
        let end_pos = pos + needle.len();

        let (before, matched_str, after) = match_slices(&inner.decoded, pos, end_pos, needle);

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: LiteralMatch(needle.to_string()),
        };

        inner.decoded.drain(..end_pos);
        inner.base += end_pos;

        let buffer_seq = self.emit_matched(before, matched_str, after);
        Ok(Some((m, buffer_seq)))
    }

    /// Check fail pattern against buffer, then try to consume regex - under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if regex consumed, Ok(None) if not found.
    /// On success the `Matched` buffer event is pushed before releasing the lock.
    pub async fn fail_check_consume_regex(
        &self,
        re: &Regex,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<RegexMatch>, EventSeq)>, FailPatternHit> {
        let mut inner = self.inner.lock().unwrap();

        // Check fail pattern first
        if let Some(fp) = fail_pattern
            && let Some(hit) = check_fail_in_buffer(&inner.decoded, fp)
        {
            return Err(hit);
        }

        let (pos, end_pos, matched_str, captures) = {
            let Some(cap) = re.captures(&inner.decoded) else {
                return Ok(None);
            };
            let Some(whole) = cap.get(0) else {
                return Ok(None);
            };
            let pos = whole.start();
            let end_pos = whole.end();
            if is_partial_line_match(re, end_pos, &inner.decoded) {
                return Ok(None);
            }
            let matched_str = whole.as_str().to_string();
            let mut captures = HashMap::new();
            for i in 0..cap.len() {
                if let Some(m) = cap.get(i) {
                    captures.insert(i.to_string(), m.as_str().to_string());
                }
            }
            (pos, end_pos, matched_str, captures)
        };

        let (before, _, after) = match_slices(&inner.decoded, pos, end_pos, &matched_str);

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: RegexMatch(captures),
        };

        inner.decoded.drain(..end_pos);
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

    /// Scan every still-unmatched slot against the current decoded buffer
    /// **without draining and without checking fail patterns**. Returns,
    /// per slot index, an `Option<MultiMatchHit>` (`Some` iff the slot
    /// matched this round). Regex slots respect the partial-line guard.
    ///
    /// `block_entry` is the absolute offset of the multimatch block's entry
    /// point. The scan operates against `decoded[block_entry - base ..]`.
    pub async fn multimatch_scan(
        &self,
        slots: &mut [PatternSlot],
        block_entry: usize,
    ) -> Vec<Option<MultiMatchHit>> {
        let inner = self.inner.lock().unwrap();
        let start_rel = block_entry.saturating_sub(inner.base);
        let text = if start_rel >= inner.decoded.len() {
            ""
        } else {
            &inner.decoded[start_rel..]
        };

        let mut results = Vec::with_capacity(slots.len());
        for slot in slots.iter() {
            let hit = match &slot.regex {
                Some(re) => scan_regex_in(re, text, inner.base + start_rel),
                None => scan_literal_in(&slot.pattern_str, text, inner.base + start_rel),
            };
            results.push(hit);
        }
        results
    }

    /// Drop the prefix of `decoded` up to absolute offset `target` without
    /// emitting any buffer event. Used at multimatch block exit to advance
    /// past `max(end_abs)` across all pattern hits in a single step.
    pub async fn drain_to(&self, target: usize) {
        let mut inner = self.inner.lock().unwrap();
        if target <= inner.base {
            return;
        }
        let advance = target - inner.base;
        debug_assert!(
            advance <= inner.decoded.len(),
            "drain_to target {target} exceeds buffer end {base}+{len}={end}",
            base = inner.base,
            len = inner.decoded.len(),
            end = inner.base + inner.decoded.len(),
        );
        let advance = advance.min(inner.decoded.len());
        inner.decoded.drain(..advance);
        inner.base += advance;
    }

    /// Current absolute `base` offset - the byte position in this shell's
    /// lifetime stream where `decoded` starts. Used by the VM at multimatch
    /// block entry so the loop's scan operates in absolute coordinates that
    /// remain stable across in-block `Grew` appends.
    pub async fn base_offset(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.base
    }

    /// Push a `Matched` buffer event for a multimatch per-pattern hit.
    /// Same `before`/`matched`/`after` shape as single-match, but **does
    /// not drain** - the actual drain happens once at block exit via
    /// `drain_to`. Returns the `EventSeq` of the just-emitted event.
    pub fn push_multimatch_matched_event(
        &self,
        before: String,
        matched: String,
        after: String,
    ) -> EventSeq {
        let _guard = self.inner.lock().unwrap();
        self.emit_matched(before, matched, after)
    }

    /// Check fail pattern against current buffer (peek only, no drain).
    pub async fn check_fail_pattern(
        &self,
        fail_pattern: Option<&FailPattern>,
    ) -> Option<FailPatternHit> {
        let fp = fail_pattern?;
        let inner = self.inner.lock().unwrap();
        check_fail_in_buffer(&inner.decoded, fp)
    }

    /// Drain the cleanly-decoded portion of the buffer, advancing base.
    /// Trailing bytes of an incomplete UTF-8 sequence stay carried over inside
    /// `Utf8Stream`, to be completed by a future `append`. Emits a `Reset`
    /// buffer event carrying the consumed prefix - byte-identical to the
    /// concatenation of `Grew` payloads emitted since the previous reset -
    /// before releasing the lock. Returns the consumed prefix.
    pub async fn clear(&self) -> String {
        let mut inner = self.inner.lock().unwrap();
        let consumed = std::mem::take(&mut inner.decoded);
        inner.base += consumed.len();
        if let Some(log) = &self.log {
            log.push_buffer_event(
                &self.shell_name,
                &self.shell_marker,
                BufferEventKind::Reset {
                    consumed: consumed.clone(),
                },
            );
        }
        consumed
    }

    /// Return the tail of the current buffer (last `n` chars) as a string.
    pub async fn snapshot_tail(&self, n: usize) -> String {
        let inner = self.inner.lock().unwrap();
        truncate_before(&inner.decoded, n)
    }

    /// Return remaining unmatched buffer data (decoded prefix as bytes).
    /// Pending bytes of an incomplete UTF-8 sequence held back by
    /// `Utf8Stream` are not returned.
    pub async fn remaining(&self) -> Vec<u8> {
        let inner = self.inner.lock().unwrap();
        inner.decoded.as_bytes().to_vec()
    }
}

/// Returns `true` if a `$`-anchored regex matched at the buffer boundary
/// where the buffer does not end with a newline - meaning the last line may
/// still be arriving and `$` matched end-of-string rather than end-of-line.
///
/// Only applies when the regex source ends with an *unescaped* `$` anchor.
/// Patterns ending in `\$` (literal dollar sign) are not anchored. Patterns
/// without a trailing `$` are never deferred.
fn is_partial_line_match(re: &Regex, match_end: usize, text: &str) -> bool {
    has_trailing_anchor(re.as_str()) && match_end == text.len() && !text.ends_with('\n')
}

/// `true` iff `src` ends in an unescaped `$` anchor. Counts the run of
/// trailing backslashes before the final `$`: an even count (including zero)
/// means the `$` is not escaped.
fn has_trailing_anchor(src: &str) -> bool {
    let Some(stripped) = src.strip_suffix('$') else {
        return false;
    };
    let trailing_backslashes = stripped.bytes().rev().take_while(|&b| b == b'\\').count();
    trailing_backslashes % 2 == 0
}

fn scan_regex_in(re: &Regex, text: &str, base: usize) -> Option<MultiMatchHit> {
    let cap = re.captures(text)?;
    let whole = cap.get(0)?;
    let pos = whole.start();
    let end_pos = whole.end();
    if is_partial_line_match(re, end_pos, text) {
        return None;
    }
    let matched = whole.as_str().to_string();
    Some(MultiMatchHit {
        matched_text: matched.clone(),
        start_abs: base + pos,
        end_abs: base + end_pos,
        before: text[..pos].to_string(),
        after: text[end_pos..].to_string(),
    })
}

fn scan_literal_in(needle: &str, text: &str, base: usize) -> Option<MultiMatchHit> {
    let pos = text.find(needle)?;
    let end_pos = pos + needle.len();
    Some(MultiMatchHit {
        matched_text: needle.to_string(),
        start_abs: base + pos,
        end_abs: base + end_pos,
        before: text[..pos].to_string(),
        after: text[end_pos..].to_string(),
    })
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

// --- Tests -----------------------------------------------

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

    /// Inspect the last `Reset` buffer event the builder accumulated.
    fn last_reset(builder: &StructuredLogBuilder) -> Option<String> {
        let events = builder.buffer_events_for_tests();
        events.last().and_then(|ev| match &ev.kind {
            BufferEventKind::Reset { consumed } => Some(consumed.clone()),
            _ => None,
        })
    }

    /// Collect every `Grew` payload the builder has accumulated, in order.
    fn all_grew(builder: &StructuredLogBuilder) -> Vec<String> {
        builder
            .buffer_events_for_tests()
            .iter()
            .filter_map(|ev| match &ev.kind {
                BufferEventKind::Grew { data } => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    // --- truncate_before ---------------------------------

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

    // --- OutputBuffer::append / remaining ----------------

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

    // --- OutputBuffer::consume_literal -------------------

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

    #[tokio::test]
    async fn consume_literal_handles_invalid_utf8_in_buffer() {
        // Regression test: invalid bytes (here 0xFF) must not corrupt offsets
        // for the drain after the match - `Utf8Stream` surfaces them as a
        // U+FFFD replacement and matching works in decoded coordinates.
        let buf = OutputBuffer::for_tests();
        let mut bytes = b"prefix".to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(b"MATCH suffix");
        buf.append(&bytes).await;
        let (m, _) = buf.consume_literal("MATCH").await.expect("found");
        assert_eq!(m.value.0, "MATCH");
        assert_eq!(buf.remaining().await, " suffix".as_bytes());
    }

    // --- OutputBuffer::consume_regex ---------------------

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

    // --- Partial-line guard ------------------------------

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

    #[tokio::test]
    async fn consume_regex_handles_invalid_utf8_in_buffer() {
        let buf = OutputBuffer::for_tests();
        let mut bytes = b"abc".to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(b" 123 def");
        buf.append(&bytes).await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.expect("found");
        assert_eq!(m.value.0.get("0").unwrap(), "123");
        assert_eq!(buf.remaining().await, " def".as_bytes());
    }

    #[tokio::test]
    async fn consume_regex_does_not_defer_on_escaped_trailing_dollar() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"price: $9").await;
        // Pattern source ends with `$` literally, but it is escaped (`\$`),
        // so it is NOT an anchor. Must not be treated as a partial-line match.
        let re = Regex::new(r"price: \$\d+").unwrap();
        let (m, _) = buf
            .consume_regex(&re)
            .await
            .expect("escaped trailing dollar must not defer");
        assert_eq!(m.value.0.get("0").unwrap(), "price: $9");
    }

    // --- has_trailing_anchor -----------------------------

    #[test]
    fn has_trailing_anchor_unescaped() {
        assert!(super::has_trailing_anchor("foo$"));
        assert!(super::has_trailing_anchor(r"^(.+)$"));
        // Two backslashes = an escaped backslash followed by an anchor.
        assert!(super::has_trailing_anchor(r"foo\\$"));
    }

    #[test]
    fn has_trailing_anchor_escaped() {
        assert!(!super::has_trailing_anchor(r"price: \$"));
        // Three backslashes = escaped backslash + escaped dollar.
        assert!(!super::has_trailing_anchor(r"foo\\\$"));
    }

    #[test]
    fn has_trailing_anchor_no_dollar() {
        assert!(!super::has_trailing_anchor("foo"));
        assert!(!super::has_trailing_anchor(""));
    }

    // --- OutputBuffer::clear -----------------------------

    #[tokio::test]
    async fn clear_empties_buffer_and_returns_consumed() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world").await;
        let consumed = buf.clear().await;
        assert_eq!(consumed, "hello world");
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

    #[tokio::test]
    async fn clear_drops_incomplete_utf8_trailing_sequence() {
        let (buf, builder, _rx) = wired_buffer();
        // U+1F389 PARTY POPPER, encoded as F0 9F 8E 89. Feed "ok" then only
        // the first two bytes of the codepoint - Utf8Stream holds them back.
        buf.append(b"ok").await;
        buf.append(&[0xF0, 0x9F]).await;
        let _ = buf.clear().await;
        let consumed = last_reset(&builder).expect("reset event");
        // Only the decoded prefix is emitted; the partial bytes are silently
        // held back (verified separately in clear_preserves_partial_utf8_in_buffer).
        assert_eq!(consumed, "ok");
    }

    #[tokio::test]
    async fn clear_consumed_equals_sum_of_grew_payloads() {
        let (buf, builder, _rx) = wired_buffer();
        buf.append(b"alpha ").await;
        buf.append(b"beta ").await;
        buf.append("gamma\n".as_bytes()).await;
        let grew_sum: String = all_grew(&builder).concat();
        let _ = buf.clear().await;
        let consumed = last_reset(&builder).expect("reset event");
        assert_eq!(consumed, grew_sum);
        assert_eq!(consumed, "alpha beta gamma\n");
    }

    #[tokio::test]
    async fn clear_preserves_partial_utf8_in_buffer() {
        let (buf, builder, _rx) = wired_buffer();
        // First two bytes of U+1F389 only - entire buffer is `pending`.
        buf.append(&[0xF0, 0x9F]).await;
        let _ = buf.clear().await;
        let consumed = last_reset(&builder).expect("reset event");
        assert_eq!(consumed, "");
        // Now finish the codepoint - Grew should fire with the completed char,
        // proving the partial bytes survived the reset.
        buf.append(&[0x8E, 0x89]).await;
        let grew: Vec<String> = all_grew(&builder);
        assert_eq!(grew.last().map(String::as_str), Some("\u{1F389}"));
    }

    // --- OutputBuffer::snapshot_tail ---------------------

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

    // --- check_fail_in_buffer ----------------------------

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

    // --- multimatch_scan / drain_to ----------------------

    #[tokio::test]
    async fn multimatch_scan_finds_literal_and_regex_without_drain() {
        let (buf, _builder, _rx) = wired_buffer();
        buf.append(
            b"job-a: started\njob-b: started\njob-a: complete (id=17)\njob-b: complete (id=23)\n",
        )
        .await;
        let block_entry = 0;

        let re_a = RegexBuilder::new(r"^job-a: complete \(id=\d+\)$")
            .multi_line(true)
            .crlf(true)
            .build()
            .unwrap();
        let re_b = RegexBuilder::new(r"^job-b: complete \(id=\d+\)$")
            .multi_line(true)
            .crlf(true)
            .build()
            .unwrap();

        let mut slots = vec![
            PatternSlot::regex("^job-a: complete \\(id=\\d+\\)$".to_string(), re_a),
            PatternSlot::regex("^job-b: complete \\(id=\\d+\\)$".to_string(), re_b),
        ];

        let hits = buf.multimatch_scan(&mut slots, block_entry).await;
        assert!(hits[0].is_some(), "first slot should hit");
        assert!(hits[1].is_some(), "second slot should hit");
        // Critical: scan did not drain.
        let remaining_len = buf.remaining().await.len();
        assert_eq!(
            remaining_len, 78,
            "scan must be non-destructive (got len {remaining_len})"
        );
    }

    #[tokio::test]
    async fn multimatch_scan_returns_absolute_offsets() {
        let buf = OutputBuffer::for_tests();
        // Prime + drain to advance base.
        buf.append(b"prefix ").await;
        let _ = buf.consume_literal("prefix ").await.unwrap();
        buf.append(b"target line\n").await;

        let re = RegexBuilder::new(r"^target line$")
            .multi_line(true)
            .crlf(true)
            .build()
            .unwrap();
        let mut slots = vec![PatternSlot::regex("^target line$".to_string(), re)];
        let hits = buf.multimatch_scan(&mut slots, 7).await;
        let hit = hits[0].as_ref().expect("hit");
        assert_eq!(
            hit.start_abs, 7,
            "start is absolute, accounting for prior drain"
        );
        assert_eq!(hit.end_abs, 7 + "target line".len());
    }

    #[tokio::test]
    async fn multimatch_scan_defers_partial_line_regex() {
        let buf = OutputBuffer::for_tests();
        // No trailing newline - `^line$` should be deferred.
        buf.append(b"line").await;
        let re = RegexBuilder::new(r"^line$")
            .multi_line(true)
            .crlf(true)
            .build()
            .unwrap();
        let mut slots = vec![PatternSlot::regex("^line$".to_string(), re)];
        let hits = buf.multimatch_scan(&mut slots, 0).await;
        assert!(
            hits[0].is_none(),
            "trailing-anchored regex must defer until newline"
        );
    }

    #[tokio::test]
    async fn multimatch_scan_duplicate_patterns_match_independently() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"hello world hello world\n").await;
        let mut slots = vec![
            PatternSlot::literal("hello".to_string()),
            PatternSlot::literal("hello".to_string()),
        ];
        let hits = buf.multimatch_scan(&mut slots, 0).await;
        // Both slots get a hit. Caller is responsible for deciding whether
        // they want distinct ranges - the scan reports the first occurrence
        // for each slot. R014 v1 accepts this; the test pins the contract.
        assert!(hits[0].is_some());
        assert!(hits[1].is_some());
    }

    #[tokio::test]
    async fn drain_to_advances_base_and_drops_prefix_no_event() {
        let (buf, builder, _rx) = wired_buffer();
        buf.append(b"abc\ndef\n").await;
        let grew_count_before = all_grew(&builder).len();
        buf.drain_to(4).await; // drop "abc\n"
        let remaining = buf.remaining().await;
        assert_eq!(remaining, b"def\n");
        assert_eq!(
            all_grew(&builder).len(),
            grew_count_before,
            "drain_to must not emit any buffer event"
        );
    }

    #[tokio::test]
    async fn drain_to_is_noop_when_offset_equals_base() {
        let buf = OutputBuffer::for_tests();
        buf.append(b"abc\n").await;
        buf.drain_to(0).await;
        assert_eq!(buf.remaining().await, b"abc\n");
    }
}
