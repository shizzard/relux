//! The green-tree builder: replays a parser event stream into a rowan tree.
//!
//! Losslessness is a joint property. The grammar must consume every token and
//! the builder must emit every consumed token's source slice; neither half is
//! sufficient alone. The builder is the only component positioned to notice the
//! first half failing, which is what its debug assertions are for.

use relux_lexer::Spanned;
use rowan::GreenNode;
use rowan::GreenNodeBuilder;

use crate::parser::Event;
use crate::syntax_kind::SyntaxKind;
use crate::syntax_kind::kind_of;

/// Replay `events` into a green tree, slicing leaf text out of `source`.
///
/// Returns `GreenNode` rather than `SyntaxNode` because only the green tree is
/// `Send + Sync`: rowan's `SyntaxNode` is a raw pointer with a non-atomic
/// refcount, so it cannot live in a document store shared across tasks. Callers
/// that want to walk the tree build a root with `SyntaxNode::new_root`.
///
/// Takes `events` by reference so the caller keeps the stream that
/// `crate::parser::errors_of` reads the syntax errors out of.
pub fn build_tree(source: &str, tokens: &[Spanned<'_>], events: &[Event]) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    let mut idx = 0usize;
    let mut depth = 0usize;
    let mut roots = 0usize;

    for event in events {
        match event {
            Event::Open { kind } => {
                debug_assert!(
                    *kind != SyntaxKind::TOMBSTONE,
                    "a TOMBSTONE reached the builder at token {idx}: a Marker \
                     was opened and never closed"
                );
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                builder.start_node((*kind).into());
            }
            Event::Close => {
                debug_assert!(
                    depth > 0,
                    "unbalanced Close at token {idx}: no node is open"
                );
                depth = depth.saturating_sub(1);
                builder.finish_node();
            }
            Event::Advance => {
                debug_assert!(
                    idx < tokens.len(),
                    "the event stream advances past the end of the token stream \
                     ({} tokens)",
                    tokens.len()
                );
                let token = &tokens[idx];
                let kind = kind_of(&token.node);
                debug_assert!(
                    kind != SyntaxKind::WORD,
                    "a WORD leaf reached the builder at token {idx}: lex() \
                     rewrites every Word into Text, so this is unreachable"
                );
                builder.token(kind.into(), &source[token.span.start()..token.span.end()]);
                idx += 1;
            }
            // Errors live only in the event stream at this stage; T22 wraps
            // error regions in ERROR nodes. Skipping one consumes no token, so
            // the token cursor is undisturbed.
            Event::Error { .. } => {}
        }
    }

    debug_assert!(
        depth == 0,
        "{depth} node(s) left open at the end of the event stream"
    );
    debug_assert!(
        roots == 1,
        "the event stream produced {roots} root node(s), expected exactly 1: \
         the grammar owns the root and must open exactly one MODULE"
    );
    debug_assert!(
        idx == tokens.len(),
        "the grammar dropped {} token(s) starting at {} ({:?} {:?}); the tree \
         would not be lossless",
        tokens.len() - idx,
        idx,
        tokens[idx].node,
        &source[tokens[idx].span.start()..tokens[idx].span.end()]
    );

    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    use relux_core::Span;

    use crate::syntax_kind::SyntaxNode;

    /// Build a tree from hand-written events over the real lexing of `source`.
    fn build(source: &str, events: &[Event]) -> GreenNode {
        build_tree(source, &relux_lexer::lex(source), events)
    }

    fn open(kind: SyntaxKind) -> Event {
        Event::Open { kind }
    }

    #[test]
    fn an_empty_module_is_a_single_empty_root() {
        let green = build("", &[open(SyntaxKind::MODULE), Event::Close]);

        assert_eq!(green.to_string(), "");
        assert_eq!(green.kind(), SyntaxKind::MODULE.into());
    }

    #[test]
    fn leaves_carry_the_source_slice() {
        let source = "fn a";
        let events = [
            open(SyntaxKind::MODULE),
            Event::Advance,
            Event::Advance,
            Event::Advance,
            Event::Close,
        ];

        assert_eq!(build(source, &events).to_string(), source);
    }

    /// Leaf text is the source slice, never the token payload. The two differ
    /// for `Token::Escape`, whose payload is `n` where the source reads `\n`.
    #[test]
    fn leaf_text_is_the_slice_not_the_payload() {
        let source = r"\n";
        let events = [open(SyntaxKind::MODULE), Event::Advance, Event::Close];

        let green = build(source, &events);

        assert_eq!(green.to_string(), r"\n");
        assert_eq!(green.to_string().len(), 2, "the payload would be 1 byte");
    }

    #[test]
    fn nodes_nest() {
        let source = "fn a";
        let events = [
            open(SyntaxKind::MODULE),
            Event::Advance,
            open(SyntaxKind::FN_DEF),
            Event::Advance,
            Event::Advance,
            Event::Close,
            Event::Close,
        ];

        let green = build(source, &events);
        assert_eq!(green.to_string(), source);

        let root = SyntaxNode::new_root(green);
        let child = root.children().next().expect("MODULE has one child node");
        assert_eq!(child.kind(), SyntaxKind::FN_DEF);
        assert_eq!(child.text().to_string(), " a");
    }

    /// `Error` events emit nothing and consume no token, so they must leave the
    /// token cursor undisturbed.
    #[test]
    fn error_events_are_skipped_without_shifting_tokens() {
        let source = "fn a";
        let events = [
            open(SyntaxKind::MODULE),
            Event::Advance,
            Event::Error {
                msg: "boom".to_string(),
                span: Span::new(0, 2),
            },
            Event::Advance,
            Event::Advance,
            Event::Close,
        ];

        let green = build(source, &events);

        assert_eq!(green.to_string(), source);
        assert_eq!(
            SyntaxNode::new_root(green).children().count(),
            0,
            "an Error event must not produce a node"
        );
    }

    // The assertions below are `debug_assert!`, so these tests are meaningful
    // only in a debug profile. That is the default for `cargo test`; a
    // `--release` run reports them as passing without executing the assertion.

    #[test]
    #[should_panic(expected = "a TOMBSTONE reached the builder")]
    fn a_tombstone_is_rejected() {
        build("", &[open(SyntaxKind::TOMBSTONE), Event::Close]);
    }

    #[test]
    #[should_panic(expected = "a WORD leaf reached the builder")]
    fn a_word_leaf_is_rejected() {
        // `lex()` rewrites every `Word` into `Text`, so this token has to be
        // built by hand. Taking `tokens` as a parameter is what makes that
        // possible.
        let tokens = vec![Spanned::new(
            relux_lexer::Token::Word("fn"),
            Span::new(0, 2),
        )];

        build_tree(
            "fn",
            &tokens,
            &[open(SyntaxKind::MODULE), Event::Advance, Event::Close],
        );
    }

    #[test]
    #[should_panic(expected = "unbalanced Close")]
    fn an_unbalanced_close_is_rejected() {
        build("", &[open(SyntaxKind::MODULE), Event::Close, Event::Close]);
    }

    #[test]
    #[should_panic(expected = "1 node(s) left open")]
    fn an_unclosed_node_is_rejected() {
        build("", &[open(SyntaxKind::MODULE)]);
    }

    #[test]
    #[should_panic(expected = "produced 0 root node(s)")]
    fn an_empty_event_stream_is_rejected() {
        build("", &[]);
    }

    #[test]
    #[should_panic(expected = "produced 2 root node(s)")]
    fn two_roots_are_rejected() {
        build(
            "",
            &[
                open(SyntaxKind::MODULE),
                Event::Close,
                open(SyntaxKind::MODULE),
                Event::Close,
            ],
        );
    }

    #[test]
    #[should_panic(expected = "dropped 1 token(s) starting at 0")]
    fn a_dropped_token_is_rejected() {
        build("fn", &[open(SyntaxKind::MODULE), Event::Close]);
    }

    #[test]
    #[should_panic(expected = "advances past the end of the token stream")]
    fn advancing_past_the_tokens_is_rejected() {
        build(
            "",
            &[open(SyntaxKind::MODULE), Event::Advance, Event::Close],
        );
    }
}
