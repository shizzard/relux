//! Streaming UTF-8 decoder for byte streams arriving in arbitrary chunks.
//!
//! PTY reads land at unpredictable byte boundaries: a 4-byte UTF-8 codepoint
//! (for example U+1F389, encoded as `F0 9F 8E 89`) can be split across two
//! reads. Decoding each chunk independently with `from_utf8_lossy` would
//! replace both halves with `U+FFFD` and lose the codepoint.
//!
//! `Utf8Stream` keeps up to 3 trailing bytes of an unfinished sequence as
//! carryover; the next chunk is prepended with that carryover before decoding.
//! Genuinely invalid sequences (not partial) emit a single `U+FFFD` and the
//! stream resynchronizes on the next valid byte.

#[derive(Debug, Default)]
pub struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes; return everything that decoded into complete
    /// codepoints. Up to 3 trailing bytes belonging to a partial sequence are
    /// kept internally and prepended to the next call.
    pub fn feed(&mut self, chunk: &[u8]) -> String {
        // Combine carryover with the new chunk.
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(chunk);

        let mut out = String::new();
        let mut start = 0;
        loop {
            let slice = &bytes[start..];
            match std::str::from_utf8(slice) {
                Ok(s) => {
                    out.push_str(s);
                    return out;
                }
                Err(e) => {
                    let valid_up_to = e.valid_up_to();
                    // Safety: from_utf8 just told us this prefix is valid UTF-8.
                    out.push_str(unsafe { std::str::from_utf8_unchecked(&slice[..valid_up_to]) });
                    match e.error_len() {
                        // None means the trailing bytes look like the start of
                        // a multi-byte sequence that hasn't fully arrived. Hold
                        // them as carryover for the next feed.
                        None => {
                            self.pending = slice[valid_up_to..].to_vec();
                            return out;
                        }
                        // A genuine encoding error of `len` bytes: emit the
                        // replacement character and keep going.
                        Some(len) => {
                            out.push('\u{FFFD}');
                            start += valid_up_to + len;
                        }
                    }
                }
            }
        }
    }

    /// Number of carryover bytes currently held back (an incomplete trailing
    /// multi-byte sequence waiting for completion). Always `<= 3`.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Flush any carryover at end-of-stream. Trailing partial bytes that never
    /// completed are treated as invalid and become a single `U+FFFD`.
    pub fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            String::new()
        } else {
            self.pending.clear();
            "\u{FFFD}".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        let mut s = Utf8Stream::new();
        assert_eq!(s.feed(b"hello"), "hello");
        assert_eq!(s.feed(b" world"), " world");
        assert!(s.pending.is_empty());
    }

    #[test]
    fn split_4byte_codepoint_round_trips() {
        // U+1F389 PARTY POPPER, encoded as F0 9F 8E 89.
        let mut s = Utf8Stream::new();
        let first = s.feed(&[0xF0, 0x9F]);
        // Both bytes are partial; nothing emitted yet.
        assert_eq!(first, "");
        assert_eq!(s.pending, vec![0xF0, 0x9F]);

        let second = s.feed(&[0x8E, 0x89]);
        // Now the codepoint is complete and emitted exactly once.
        assert_eq!(second, "\u{1F389}");
        assert!(s.pending.is_empty());
    }

    #[test]
    fn split_3byte_codepoint_round_trips() {
        // U+2122 TRADE MARK SIGN, encoded as E2 84 A2.
        let mut s = Utf8Stream::new();
        assert_eq!(s.feed(&[0xE2]), "");
        assert_eq!(s.feed(&[0x84, 0xA2]), "\u{2122}");
    }

    #[test]
    fn invalid_byte_emits_replacement_and_recovers() {
        let mut s = Utf8Stream::new();
        // 0xFF is never valid in UTF-8; the surrounding ASCII must still come through.
        let out = s.feed(&[b'a', 0xFF, b'b']);
        assert_eq!(out, "a\u{FFFD}b");
        assert!(s.pending.is_empty());
    }

    #[test]
    fn carryover_is_bounded() {
        let mut s = Utf8Stream::new();
        // Feed only the first byte of a 4-byte sequence many times — pending
        // must never exceed 3 bytes.
        for _ in 0..10 {
            s.feed(&[0xF0]);
            assert!(s.pending.len() <= 3, "pending grew beyond 3 bytes");
        }
    }

    #[test]
    fn lone_continuation_byte_emits_replacement() {
        let mut s = Utf8Stream::new();
        // 0x8E by itself is a continuation byte with no leader.
        let out = s.feed(&[0x8E]);
        assert_eq!(out, "\u{FFFD}");
        assert!(s.pending.is_empty());
    }

    #[test]
    fn pending_len_reports_carryover_size() {
        let mut s = Utf8Stream::new();
        assert_eq!(s.pending_len(), 0);
        s.feed(b"hello");
        assert_eq!(s.pending_len(), 0);
        // First two bytes of U+1F389 (F0 9F 8E 89) — incomplete.
        s.feed(&[0xF0, 0x9F]);
        assert_eq!(s.pending_len(), 2);
        // Finish the codepoint — pending drains.
        s.feed(&[0x8E, 0x89]);
        assert_eq!(s.pending_len(), 0);
    }

    #[test]
    fn flush_emits_replacement_for_pending_bytes() {
        let mut s = Utf8Stream::new();
        s.feed(&[0xF0, 0x9F]);
        // Stream ended mid-codepoint; flush turns the pending bytes into a
        // single replacement char.
        assert_eq!(s.flush(), "\u{FFFD}");
        assert_eq!(s.flush(), "");
    }
}
