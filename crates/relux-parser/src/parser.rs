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

#[cfg(test)]
mod tests {
    use super::*;

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
}
