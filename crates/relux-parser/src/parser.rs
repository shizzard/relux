//! The parser core: the cursor, the event stream, and the marker machinery
//! that the grammar tasks are written against.
//!
//! No grammar lives here. T03's builder turns the event stream into a rowan
//! green tree; the productions that emit the events arrive in T06 onward.

use relux_core::Span;

use crate::syntax_kind::SyntaxKind;

/// A single step in the parse, recorded rather than applied directly so that
/// a node's kind can be decided after its children have been parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Opens a node. Written as `TOMBSTONE` and patched in by `close`.
    Open { kind: SyntaxKind },
    /// Closes the innermost open node.
    Close,
    /// Consumes one token into the innermost open node.
    Advance,
    /// Records a syntax error at a byte range.
    Error { msg: String, span: Span },
}

/// A syntax error. Errors are stored only as `Event::Error`; this type is how
/// they are read back out, never a second place they are accumulated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{msg}")]
pub struct SyntaxError {
    pub msg: String,
    pub span: Span,
}

/// Read every error out of an event stream, in source order.
pub fn errors_of(events: &[Event]) -> Vec<SyntaxError> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Error { msg, span } => Some(SyntaxError {
                msg: msg.clone(),
                span: *span,
            }),
            _ => None,
        })
        .collect()
}

/// A node that has been opened but whose kind is not yet decided.
///
/// Dropping one leaves a `TOMBSTONE` in the event stream that does not fail
/// until T03's builder asserts on it -- by which point the failure looks like
/// a builder bug rather than a grammar bug.
#[must_use = "a dropped Marker leaves a TOMBSTONE in the event stream"]
pub struct Marker {
    pos: usize,
}

/// A node that has been opened and closed. Only useful as the argument to
/// `open_before`.
#[derive(Debug, Clone, Copy)]
pub struct Completed {
    pos: usize,
}

/// Walks a token stream, recording `Event`s. Grammar productions are written
/// as free functions taking `&mut Parser`.
pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<relux_lexer::Spanned<'a>>,
    pos: usize,
    events: Vec<Event>,
}

impl<'a> Parser<'a> {
    /// Lex `source` and position the cursor at its first token.
    pub fn new(source: &'a str) -> Parser<'a> {
        Parser {
            source,
            tokens: relux_lexer::lex(source),
            pos: 0,
            events: Vec::new(),
        }
    }

    /// Consume the parser, yielding the tokens and the recorded events.
    /// T03's `build_tree` needs both: the events give it the tree shape, the
    /// tokens give it the leaf spans.
    pub fn finish(self) -> (Vec<relux_lexer::Spanned<'a>>, Vec<Event>) {
        (self.tokens, self.events)
    }

    /// True once every token has been consumed.
    pub fn eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// The kind `n` tokens ahead of the cursor, or `EOF` past the end.
    pub fn nth(&self, n: usize) -> SyntaxKind {
        match self.tokens.get(self.pos + n) {
            Some(token) => crate::syntax_kind::kind_of(&token.node),
            None => SyntaxKind::EOF,
        }
    }

    /// The source text of the token `n` ahead, or `""` past the end.
    ///
    /// This is the raw source slice, never the token's payload -- the two
    /// differ for `Token::Escape`, and T01 and T03 both fixed leaf text as
    /// the slice.
    pub fn nth_text(&self, n: usize) -> &'a str {
        match self.tokens.get(self.pos + n) {
            Some(token) => &self.source[token.span.start()..token.span.end()],
            None => "",
        }
    }

    /// True if the cursor is on a token of kind `k`.
    pub fn at(&self, k: SyntaxKind) -> bool {
        self.nth(0) == k
    }

    /// True if the cursor is on any of `ks`.
    pub fn at_any(&self, ks: &[SyntaxKind]) -> bool {
        ks.contains(&self.nth(0))
    }

    /// Consume the current token into the innermost open node.
    ///
    /// Every consumed token is recorded, so no token is ever dropped on the
    /// floor -- that is what makes the resulting tree lossless. There is
    /// deliberately no whitespace-skipping variant: whitespace is
    /// grammatically significant in Relux, so skipping it at the cursor
    /// would make those distinctions unexpressible. Whitespace helpers
    /// belong in the grammar, where they consume into the open node rather
    /// than discarding.
    pub fn advance(&mut self) {
        assert!(!self.eof(), "advance past the end of input");
        self.pos += 1;
        self.events.push(Event::Advance);
    }

    /// Consume the current token if it is of kind `k`. Returns whether it was.
    ///
    /// `EOF` is zero-width: matching it consumes nothing, because it is a
    /// sentinel rather than a token. Without that, `eat(EOF)` would reach
    /// `advance` and panic on its past-the-end assertion precisely when the
    /// match succeeded.
    pub fn eat(&mut self, k: SyntaxKind) -> bool {
        if !self.at(k) {
            return false;
        }
        if k == SyntaxKind::EOF {
            return true;
        }
        self.advance();
        true
    }

    /// Consume a token of kind `k`, or record an error.
    ///
    /// **Does not advance on mismatch.** A production that needs to make
    /// progress regardless must advance itself; T22's recovery loops do.
    pub fn expect(&mut self, k: SyntaxKind) {
        if self.eat(k) {
            return;
        }
        self.error(format!("expected {k:?}"));
    }

    /// Record an error at the current token, or at the end of the source when
    /// the cursor is past the last token.
    pub fn error(&mut self, msg: impl Into<String>) {
        let span = self.current_span();
        self.events.push(Event::Error {
            msg: msg.into(),
            span,
        });
    }

    fn current_span(&self) -> Span {
        match self.tokens.get(self.pos) {
            Some(token) => token.span,
            None => Span::new(self.source.len(), self.source.len()),
        }
    }

    /// Open a node whose kind will be decided by `close`.
    pub fn open(&mut self) -> Marker {
        let pos = self.events.len();
        self.events.push(Event::Open {
            kind: SyntaxKind::TOMBSTONE,
        });
        Marker { pos }
    }

    /// Close `m`, deciding its kind.
    pub fn close(&mut self, m: Marker, k: SyntaxKind) -> Completed {
        self.events[m.pos] = Event::Open { kind: k };
        self.events.push(Event::Close);
        Completed { pos: m.pos }
    }

    /// Open a new node that will enclose the already-completed `c`.
    ///
    /// This is what makes left-associative reinterpretation possible without
    /// backtracking: parse an expression, then on seeing `=` or `?`, wrap the
    /// finished expression node in a `PURE_MATCH_STMT`.
    ///
    /// **Invalidates markers.** It inserts into the event vector, so every
    /// `Marker` and `Completed` holding a `pos >= c.pos` silently shifts by
    /// one and now points at the wrong event. Only ever call it on the most
    /// recently completed node.
    pub fn open_before(&mut self, c: Completed) -> Marker {
        self.events.insert(
            c.pos,
            Event::Open {
                kind: SyntaxKind::TOMBSTONE,
            },
        );
        Marker { pos: c.pos }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_lexes_the_source() {
        let (tokens, events) = Parser::new("test \"a\"").finish();

        assert!(!tokens.is_empty());
        assert!(events.is_empty(), "constructing must not record events");
    }

    #[test]
    fn eof_is_true_for_empty_input() {
        assert!(Parser::new("").eof());
    }

    #[test]
    fn eof_is_false_with_tokens_remaining() {
        assert!(!Parser::new("fn").eof());
    }

    #[test]
    fn finish_returns_the_tokens_it_lexed() {
        let source = "fn a";
        let expected = relux_lexer::lex(source).len();

        let (tokens, _) = Parser::new(source).finish();

        assert_eq!(tokens.len(), expected);
    }

    #[test]
    fn nth_reports_kinds_in_order() {
        let p = Parser::new("fn a");

        assert_eq!(p.nth(0), SyntaxKind::FN_KW);
        assert_eq!(p.nth(1), SyntaxKind::SPACE);
        assert_eq!(p.nth(2), SyntaxKind::TEXT);
    }

    #[test]
    fn nth_is_eof_past_the_end() {
        let p = Parser::new("fn");

        assert_eq!(p.nth(0), SyntaxKind::FN_KW);
        assert_eq!(p.nth(1), SyntaxKind::EOF);
        assert_eq!(p.nth(99), SyntaxKind::EOF);
    }

    #[test]
    fn nth_is_eof_for_empty_input() {
        assert_eq!(Parser::new("").nth(0), SyntaxKind::EOF);
    }

    #[test]
    fn nth_text_returns_the_source_slice() {
        let p = Parser::new("fn hello");

        assert_eq!(p.nth_text(0), "fn");
        assert_eq!(p.nth_text(1), " ");
        assert_eq!(p.nth_text(2), "hello");
    }

    /// `nth_text` is the source slice, not the token payload. They differ for
    /// `Token::Escape`, whose payload is `n` where the source reads `\n`.
    /// T01 and T03 both fixed leaf text as the source slice; this keeps the
    /// cursor consistent with them.
    #[test]
    fn nth_text_is_the_slice_not_the_payload() {
        let p = Parser::new(r"\n");

        assert_eq!(p.nth(0), SyntaxKind::ESCAPE);
        assert_eq!(p.nth_text(0), r"\n");
    }

    #[test]
    fn nth_text_is_empty_past_the_end() {
        assert_eq!(Parser::new("").nth_text(0), "");
        assert_eq!(Parser::new("fn").nth_text(5), "");
    }

    #[test]
    fn at_and_at_any_test_the_current_token() {
        let p = Parser::new("fn a");

        assert!(p.at(SyntaxKind::FN_KW));
        assert!(!p.at(SyntaxKind::TEST_KW));
        assert!(p.at_any(&[SyntaxKind::TEST_KW, SyntaxKind::FN_KW]));
        assert!(!p.at_any(&[SyntaxKind::TEST_KW, SyntaxKind::EFFECT_KW]));
    }

    #[test]
    fn at_matches_eof_at_the_end() {
        assert!(Parser::new("").at(SyntaxKind::EOF));
    }

    #[test]
    fn advance_moves_the_cursor_and_records_an_event() {
        let mut p = Parser::new("fn a");

        p.advance();

        assert_eq!(p.nth(0), SyntaxKind::SPACE);
        let (_, events) = p.finish();
        assert_eq!(events, vec![Event::Advance]);
    }

    #[test]
    fn eat_consumes_a_match_and_reports_it() {
        let mut p = Parser::new("fn");

        assert!(p.eat(SyntaxKind::FN_KW));
        assert!(p.eof());

        let (_, events) = p.finish();
        assert_eq!(events, vec![Event::Advance]);
    }

    /// `EOF` is a sentinel, not a token: matching it must not reach `advance`
    /// and trip its past-the-end assertion. Covers both empty input and
    /// input fully consumed down to the end.
    #[test]
    fn eat_eof_succeeds_without_advancing_or_panicking() {
        let mut empty = Parser::new("");
        assert!(empty.eat(SyntaxKind::EOF));
        let (_, events) = empty.finish();
        assert!(events.is_empty(), "matching EOF must not record an Advance");

        let mut consumed = Parser::new("fn");
        consumed.advance();
        assert!(consumed.eat(SyntaxKind::EOF));
        let (_, events) = consumed.finish();
        assert_eq!(events, vec![Event::Advance], "no second Advance for EOF");
    }

    /// `expect(EOF)` is the obvious spelling of "nothing may be left"; it
    /// must succeed silently at the true end of input and record exactly one
    /// error when tokens remain, without panicking either way.
    #[test]
    fn expect_eof_succeeds_at_the_end_and_errors_otherwise() {
        let mut empty = Parser::new("");
        empty.expect(SyntaxKind::EOF);
        let (_, events) = empty.finish();
        assert!(events.is_empty(), "expect(EOF) at EOF records nothing");

        let mut consumed = Parser::new("fn");
        consumed.advance();
        consumed.expect(SyntaxKind::EOF);
        let (_, events) = consumed.finish();
        assert_eq!(
            events,
            vec![Event::Advance],
            "expect(EOF) at EOF records nothing"
        );

        let mut remaining = Parser::new("fn");
        remaining.expect(SyntaxKind::EOF);
        assert!(
            remaining.at(SyntaxKind::FN_KW),
            "cursor must not have moved"
        );
        let (_, events) = remaining.finish();
        assert_eq!(errors_of(&events).len(), 1);
    }

    #[test]
    fn eat_leaves_a_mismatch_alone() {
        let mut p = Parser::new("fn");

        assert!(!p.eat(SyntaxKind::TEST_KW));
        assert!(p.at(SyntaxKind::FN_KW), "cursor must not have moved");
        let (_, events) = p.finish();
        assert!(events.is_empty(), "a failed eat records nothing");
    }

    #[test]
    fn expect_consumes_a_match_silently() {
        let mut p = Parser::new("fn");

        p.expect(SyntaxKind::FN_KW);

        let (_, events) = p.finish();
        assert_eq!(events, vec![Event::Advance]);
    }

    /// The API decision most likely to be assumed backwards. `expect` records
    /// the error and leaves the cursor where it was; assuming otherwise builds
    /// a loop that never advances, which Task 7's fuel counter then catches.
    #[test]
    fn expect_does_not_advance_on_mismatch() {
        let mut p = Parser::new("fn");

        p.expect(SyntaxKind::TEST_KW);

        assert!(p.at(SyntaxKind::FN_KW), "cursor must not have moved");
        let (_, events) = p.finish();
        assert_eq!(errors_of(&events).len(), 1);
    }

    #[test]
    fn error_spans_the_current_token() {
        let mut p = Parser::new("fn a");
        p.advance();
        p.advance();

        p.error("bad identifier");

        let (_, events) = p.finish();
        let errors = errors_of(&events);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].msg, "bad identifier");
        assert_eq!(errors[0].span, Span::new(3, 4));
    }

    #[test]
    fn error_at_eof_is_an_empty_span_at_the_end_of_source() {
        let source = "fn";
        let mut p = Parser::new(source);
        p.advance();

        p.error("unexpected end of input");

        let (_, events) = p.finish();
        let errors = errors_of(&events);
        assert_eq!(errors[0].span, Span::new(source.len(), source.len()));
    }

    #[test]
    fn errors_of_returns_errors_in_source_order() {
        let events = vec![
            Event::Open {
                kind: SyntaxKind::MODULE,
            },
            Event::Error {
                msg: "first".to_string(),
                span: Span::new(0, 1),
            },
            Event::Advance,
            Event::Error {
                msg: "second".to_string(),
                span: Span::new(4, 6),
            },
            Event::Close,
        ];

        let errors = errors_of(&events);

        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].msg, "first");
        assert_eq!(errors[0].span, Span::new(0, 1));
        assert_eq!(errors[1].msg, "second");
        assert_eq!(errors[1].span, Span::new(4, 6));
    }

    #[test]
    fn errors_of_is_empty_when_nothing_failed() {
        let events = vec![
            Event::Open {
                kind: SyntaxKind::MODULE,
            },
            Event::Advance,
            Event::Close,
        ];

        assert!(errors_of(&events).is_empty());
    }

    #[test]
    fn syntax_error_displays_its_message() {
        let err = SyntaxError {
            msg: "expected newline".to_string(),
            span: Span::new(3, 3),
        };

        assert_eq!(err.to_string(), "expected newline");
    }

    #[test]
    fn close_patches_the_kind_into_the_open_event() {
        let mut p = Parser::new("fn");
        let m = p.open();
        p.advance();
        p.close(m, SyntaxKind::FN_DEF);

        let (_, events) = p.finish();

        assert_eq!(
            events,
            vec![
                Event::Open {
                    kind: SyntaxKind::FN_DEF
                },
                Event::Advance,
                Event::Close,
            ]
        );
    }

    /// An unclosed marker is exactly the bug `#[must_use]` warns about: the
    /// `TOMBSTONE` survives into the stream and only fails later, inside
    /// T03's builder.
    #[test]
    fn an_unclosed_marker_leaves_a_tombstone() {
        let mut p = Parser::new("fn");
        let _dropped = p.open();
        p.advance();

        let (_, events) = p.finish();

        assert_eq!(
            events,
            vec![
                Event::Open {
                    kind: SyntaxKind::TOMBSTONE
                },
                Event::Advance,
            ]
        );
    }

    /// The reason events exist at all: a node's kind can be decided after its
    /// children are parsed. Here an expression is reinterpreted as the left
    /// side of a pure match once the `=` is seen.
    #[test]
    fn open_before_wraps_a_completed_node() {
        let mut p = Parser::new("fn");
        let m = p.open();
        p.advance();
        let completed = p.close(m, SyntaxKind::VAR_EXPR);

        let outer = p.open_before(completed);
        p.close(outer, SyntaxKind::PURE_MATCH_STMT);

        let (_, events) = p.finish();

        assert_eq!(
            events,
            vec![
                Event::Open {
                    kind: SyntaxKind::PURE_MATCH_STMT
                },
                Event::Open {
                    kind: SyntaxKind::VAR_EXPR
                },
                Event::Advance,
                Event::Close,
                Event::Close,
            ]
        );
    }

    #[test]
    fn nested_nodes_balance() {
        let mut p = Parser::new("fn a");
        let outer = p.open();
        p.advance();
        let inner = p.open();
        p.advance();
        p.close(inner, SyntaxKind::IDENT_FN);
        p.close(outer, SyntaxKind::FN_DEF);

        let (_, events) = p.finish();

        let opens = events
            .iter()
            .filter(|e| matches!(e, Event::Open { .. }))
            .count();
        let closes = events.iter().filter(|e| **e == Event::Close).count();
        assert_eq!(opens, closes);
    }
}
