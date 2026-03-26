use std::ops::Range;

// ─── Span ────────────────────────────────────────────────────

/// Source span represented as a byte-offset range.
///
/// Fields are private by design — all span arithmetic must be implemented
/// as methods on this type and covered by unit tests. Do not expose fields
/// to prevent ad-hoc arithmetic at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Merge two spans into the smallest span covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Extend this span to also cover `pos`.
    pub fn extend_to(self, pos: usize) -> Span {
        Span {
            start: self.start.min(pos),
            end: self.end.max(pos),
        }
    }

    /// Extend the end of this span to `new_end`.
    pub fn extend_end(self, new_end: usize) -> Span {
        Span {
            start: self.start,
            end: new_end,
        }
    }

    /// True if `pos` falls within `[start, end)`.
    pub fn contains_pos(&self, pos: usize) -> bool {
        pos >= self.start && pos < self.end
    }

    /// True if `other` is entirely within this span.
    pub fn contains_span(&self, other: &Span) -> bool {
        other.start >= self.start && other.end <= self.end
    }
}

impl From<Range<usize>> for Span {
    fn from(r: Range<usize>) -> Self {
        Self {
            start: r.start,
            end: r.end,
        }
    }
}

impl From<Span> for Range<usize> {
    fn from(s: Span) -> Self {
        s.start..s.end
    }
}

impl From<chumsky::span::SimpleSpan> for Span {
    fn from(s: chumsky::span::SimpleSpan) -> Self {
        Self {
            start: s.start,
            end: s.end,
        }
    }
}

// ─── Spanned ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T, S = Span> {
    pub node: T,
    pub span: S,
}

impl<T, S> Spanned<T, S> {
    pub fn new(node: T, span: S) -> Self {
        Self { node, span }
    }
}

impl<T> From<Spanned<T>> for (T, chumsky::span::SimpleSpan) {
    fn from(s: Spanned<T>) -> Self {
        (s.node, (s.span.start()..s.span.end()).into())
    }
}

impl<T> From<(T, chumsky::span::SimpleSpan)> for Spanned<T> {
    fn from((node, span): (T, chumsky::span::SimpleSpan)) -> Self {
        Self {
            node,
            span: Span::from(span),
        }
    }
}

// ─── Span Tests ─────────────────────────────────────────────

#[cfg(test)]
mod span_tests {
    use super::Span;

    #[test]
    fn new_and_accessors() {
        let s = Span::new(5, 10);
        assert_eq!(s.start(), 5);
        assert_eq!(s.end(), 10);
    }

    #[test]
    fn len_and_is_empty() {
        assert_eq!(Span::new(0, 5).len(), 5);
        assert!(!Span::new(0, 5).is_empty());
        assert_eq!(Span::new(3, 3).len(), 0);
        assert!(Span::new(3, 3).is_empty());
    }

    #[test]
    fn merge() {
        assert_eq!(Span::new(2, 5).merge(Span::new(8, 12)), Span::new(2, 12));
        assert_eq!(Span::new(8, 12).merge(Span::new(2, 5)), Span::new(2, 12));
        assert_eq!(Span::new(2, 10).merge(Span::new(5, 8)), Span::new(2, 10));
    }

    #[test]
    fn extend_to() {
        assert_eq!(Span::new(5, 10).extend_to(2), Span::new(2, 10));
        assert_eq!(Span::new(5, 10).extend_to(15), Span::new(5, 15));
        assert_eq!(Span::new(5, 10).extend_to(7), Span::new(5, 10));
    }

    #[test]
    fn extend_end() {
        assert_eq!(Span::new(5, 10).extend_end(15), Span::new(5, 15));
        assert_eq!(Span::new(5, 10).extend_end(8), Span::new(5, 8));
    }

    #[test]
    fn contains_pos() {
        let s = Span::new(5, 10);
        assert!(s.contains_pos(5));
        assert!(s.contains_pos(9));
        assert!(!s.contains_pos(10)); // exclusive end
        assert!(!s.contains_pos(4));
    }

    #[test]
    fn contains_span() {
        let outer = Span::new(2, 12);
        assert!(outer.contains_span(&Span::new(2, 12)));
        assert!(outer.contains_span(&Span::new(5, 8)));
        assert!(!outer.contains_span(&Span::new(1, 8)));
        assert!(!outer.contains_span(&Span::new(5, 13)));
    }

    #[test]
    fn from_range_roundtrip() {
        let range = 3..7usize;
        let span = Span::from(range.clone());
        let back: std::ops::Range<usize> = span.into();
        assert_eq!(back, range);
    }
}

pub mod core;
pub mod diagnostics;
pub mod dsl;
pub mod history;
pub mod pure;
pub mod runtime;
