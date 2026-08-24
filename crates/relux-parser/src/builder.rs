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
///
/// `tokens` must be `relux_lexer::lex(source)` for this same `source`: leaf
/// text is sliced out of `source` at each token's span, so a stale pairing
/// yields garbage leaves or panics on a char boundary.
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
                    "a TOMBSTONE reached the builder at token {idx} ({}): a \
                     Marker was opened and never closed",
                    at_token(source, tokens, idx)
                );
                debug_assert!(
                    !kind.is_leaf() && *kind != SyntaxKind::EOF,
                    "Open with the non-node kind {kind:?} at token {idx} ({}): \
                     only an inner-node kind can open a node",
                    at_token(source, tokens, idx)
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
                    "unbalanced Close: no node is open, while the token cursor \
                     was at {idx} ({})",
                    at_token(source, tokens, idx)
                );
                depth -= 1;
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
                    depth > 0,
                    "token {idx} ({:?}) is consumed outside any node: the \
                     grammar advanced before opening a node or after closing \
                     the last one",
                    token.node
                );
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

/// Describe where the token cursor is, for an assertion message. A token
/// ordinal alone is close to unactionable on a real file -- it counts every
/// space, tab and newline -- so the byte offset and the source text go with it.
///
/// `idx` may sit past the end of `tokens`, which is why this takes it by value
/// rather than a token.
fn at_token(source: &str, tokens: &[Spanned<'_>], idx: usize) -> String {
    match tokens.get(idx) {
        Some(token) => format!(
            "byte {}, {:?}",
            token.span.start(),
            &source[token.span.start()..token.span.end()]
        ),
        None => format!("past the end of the {} token(s)", tokens.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use relux_core::Span;

    use crate::parser::Parser;
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

    /// Text alone does not pin the tree down: every leaf could carry the wrong
    /// kind and `to_string` would be unchanged. Whitespace is a real leaf here,
    /// not rowan trivia, so it appears in the sequence.
    #[test]
    fn leaves_carry_the_kind_of_their_token() {
        let source = "fn a";
        let events = [
            open(SyntaxKind::MODULE),
            Event::Advance,
            Event::Advance,
            Event::Advance,
            Event::Close,
        ];

        let root = SyntaxNode::new_root(build(source, &events));
        let kinds: Vec<SyntaxKind> = root
            .children_with_tokens()
            .map(|element| element.kind())
            .collect();

        assert_eq!(
            kinds,
            vec![SyntaxKind::FN_KW, SyntaxKind::SPACE, SyntaxKind::TEXT]
        );
    }

    /// Leaf text is sliced by raw byte offset, so a token span landing off a
    /// char boundary panics outright. Covers 2-, 3- and 4-byte characters.
    #[test]
    fn multi_byte_characters_survive_slicing() {
        let source = "send \"caf\u{00e9} \u{4e2d}\u{6587} \u{1f600}\"";
        let tokens = relux_lexer::lex(source);
        let mut events = vec![open(SyntaxKind::MODULE)];
        events.extend(tokens.iter().map(|_| Event::Advance));
        events.push(Event::Close);

        assert_eq!(build_tree(source, &tokens, &events).to_string(), source);
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

    /// The grammar and the builder are each other's only real client, and
    /// losslessness is the property they hold jointly. Neither module's own
    /// tests can see that.
    #[test]
    fn the_stub_grammar_and_the_builder_round_trip_a_source() {
        let source = "test \"a\"\n  send \"b\"\n";
        let mut p = Parser::new(source);

        crate::grammar::module(&mut p);

        let (tokens, events) = p.finish();
        assert_eq!(build_tree(source, &tokens, &events).to_string(), source);
    }

    /// The invariants are `debug_assert!`, so the tests that trip them are
    /// compiled only in a debug profile -- which is what `cargo test` builds by
    /// default. Under `--release` the assertion does not run, and a
    /// `should_panic` test that does not panic fails, so these must not exist
    /// there at all.
    #[cfg(debug_assertions)]
    mod assertions {
        use super::*;

        #[test]
        #[should_panic(expected = "a TOMBSTONE reached the builder")]
        fn a_tombstone_is_rejected() {
            build("", &[open(SyntaxKind::TOMBSTONE), Event::Close]);
        }

        #[test]
        #[should_panic(expected = "Open with the non-node kind SPACE")]
        fn opening_a_node_with_a_leaf_kind_is_rejected() {
            build("", &[open(SyntaxKind::SPACE), Event::Close]);
        }

        #[test]
        #[should_panic(expected = "Open with the non-node kind EOF")]
        fn opening_a_node_with_eof_is_rejected() {
            build("", &[open(SyntaxKind::EOF), Event::Close]);
        }

        /// The mirror image of the dropped-token case: a dispatch loop that
        /// advances after closing the node it meant to fill.
        #[test]
        #[should_panic(expected = "is consumed outside any node")]
        fn advancing_after_the_root_closed_is_rejected() {
            build(
                "fn",
                &[open(SyntaxKind::MODULE), Event::Close, Event::Advance],
            );
        }

        /// And a loop that consumes a token before opening its node.
        #[test]
        #[should_panic(expected = "is consumed outside any node")]
        fn advancing_before_any_node_opens_is_rejected() {
            build(
                "fn",
                &[Event::Advance, open(SyntaxKind::MODULE), Event::Close],
            );
        }

        #[test]
        #[should_panic(expected = "a WORD leaf reached the builder")]
        fn a_word_leaf_is_rejected() {
            // `lex()` rewrites every `Word` into `Text`, so this token has to
            // be built by hand. Taking `tokens` as a parameter is what makes
            // that possible.
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
}
