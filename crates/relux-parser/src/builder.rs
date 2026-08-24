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
/// `events` must be a whole-file stream: exactly one root node, and every
/// token consumed. A single production's events are neither, so this cannot be
/// pointed at one directly -- a unit test for, say, `timeout` over `"~5s\n"`
/// has to wrap the production in a node of its own and drain the parser to EOF
/// first, or it trips the dropped-token assertion on the trailing newline.
///
/// `tokens` must be `relux_lexer::lex(source)` for this same `source`: leaf
/// text is sliced out of `source` at each token's span, so a stale pairing
/// yields garbage leaves or panics on a char boundary. The assertion messages
/// slice `source` too, so a violation can panic while one of them is being
/// formatted -- a panic inside a panic, which aborts the process without
/// reporting either.
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
                    "token {idx} ({}) is consumed outside any node: the \
                     grammar advanced before opening a node or after closing \
                     the last one",
                    at_token(source, tokens, idx)
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
         a whole-file stream has a single root, which for the real grammar is \
         MODULE"
    );
    debug_assert!(
        idx == tokens.len(),
        "the grammar dropped {} token(s) starting at {} ({}); the tree would \
         not be lossless",
        tokens.len() - idx,
        idx,
        at_token(source, tokens, idx)
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
        None => format!("past the end of a {}-token stream", tokens.len()),
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

    /// `at_token` renders the parenthetical that every cursor-position message
    /// carries. Nothing else pins its shape: a `should_panic` expectation that
    /// stops at the token ordinal still passes with the whole parenthetical
    /// deleted, so the feature needs a test of its own.
    #[test]
    fn at_token_names_the_byte_offset_and_the_source_text() {
        let source = "fn a";
        let tokens = relux_lexer::lex(source);

        assert_eq!(at_token(source, &tokens, 0), "byte 0, \"fn\"");
        assert_eq!(at_token(source, &tokens, 1), "byte 2, \" \"");
        assert_eq!(at_token(source, &tokens, 2), "byte 3, \"a\"");
    }

    /// A production that stops one token early, before its trailing newline,
    /// is the likeliest way into these messages, so the newline token is the
    /// likeliest thing to be rendered. Escaping it keeps the message on one
    /// line -- `Token`'s own `Debug` does not.
    #[test]
    fn at_token_escapes_a_newline_rather_than_breaking_the_line() {
        let source = "a\nb";
        let tokens = relux_lexer::lex(source);

        let rendered = at_token(source, &tokens, 1);

        assert_eq!(rendered, "byte 1, \"\\n\"");
        assert!(!rendered.contains('\n'), "the message must be one line");
    }

    /// The offset is a byte offset, so the text beside it must still be cut on
    /// a char boundary: a whole character, never a byte prefix of one.
    #[test]
    fn at_token_slices_multi_byte_text_whole() {
        let source = "caf\u{00e9} \u{4e2d}\u{6587}";
        let tokens = relux_lexer::lex(source);

        assert_eq!(at_token(source, &tokens, 0), "byte 0, \"caf\u{00e9}\"");
        assert_eq!(at_token(source, &tokens, 2), "byte 6, \"\u{4e2d}\u{6587}\"");
    }

    /// Every end-of-stream assertion reports from a cursor at or past the last
    /// token, and the token list itself can be empty.
    #[test]
    fn at_token_reports_a_cursor_past_the_end() {
        let source = "fn a";
        let tokens = relux_lexer::lex(source);

        assert_eq!(
            at_token(source, &tokens, tokens.len()),
            "past the end of a 3-token stream"
        );
        assert_eq!(
            at_token(source, &tokens, 99),
            "past the end of a 3-token stream"
        );
        assert_eq!(at_token("", &[], 0), "past the end of a 0-token stream");
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

    /// `open_before` retrofits a parent around an already-closed node, which is
    /// how T15 turns a parsed expression into the left operand of the operator
    /// that follows it. It inserts an `Open` into the middle of the stream, so
    /// the builder sees a node begin before events it has already replayed --
    /// the one event ordering nothing else on this branch produces.
    ///
    /// The assertion is the rust-analyzer-style dump, which prints kinds,
    /// ranges and nesting in one go. `{:#?}` on a `SyntaxNode` gives it for
    /// free.
    #[test]
    fn a_retrofitted_parent_nests_the_node_it_wrapped() {
        let source = "a=b";
        let mut p = Parser::new(source);

        let root = p.open();
        let lhs = p.open();
        p.advance();
        let lhs = p.close(lhs, SyntaxKind::VAR_EXPR);
        // Retroactively make the closed VAR_EXPR the first child of a
        // statement that did not exist when it was parsed.
        let stmt = p.open_before(lhs);
        p.advance();
        p.advance();
        p.close(stmt, SyntaxKind::PURE_MATCH_STMT);
        p.close(root, SyntaxKind::MODULE);

        let (tokens, events) = p.finish();
        let green = build_tree(source, &tokens, &events);

        assert_eq!(green.to_string(), source, "still lossless");
        assert_eq!(
            format!("{:#?}", SyntaxNode::new_root(green)),
            "MODULE@0..3\n  \
               PURE_MATCH_STMT@0..3\n    \
                 VAR_EXPR@0..1\n      \
                   TEXT@0..1 \"a\"\n    \
                 EQ@1..2 \"=\"\n    \
                 TEXT@2..3 \"b\"\n"
        );
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

        /// Pins the whole rendered message over a non-empty source, so the
        /// cursor parenthetical is covered end to end rather than only in
        /// `at_token`'s own tests.
        #[test]
        #[should_panic(expected = "a TOMBSTONE reached the builder at token 1 \
                                   (byte 2, \" \"): a Marker was opened and \
                                   never closed")]
        fn an_assertion_message_names_where_the_cursor_is() {
            build(
                "fn a",
                &[
                    open(SyntaxKind::MODULE),
                    Event::Advance,
                    open(SyntaxKind::TOMBSTONE),
                    Event::Close,
                    Event::Close,
                ],
            );
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
        ///
        /// The expectation runs through the cursor parenthetical: stopping at
        /// the token ordinal passes with `at_token` and its argument deleted
        /// from the message, which is the whole point of routing them here.
        #[test]
        #[should_panic(expected = "token 0 (byte 0, \"fn\") is consumed \
                                   outside any node")]
        fn advancing_after_the_root_closed_is_rejected() {
            build(
                "fn",
                &[open(SyntaxKind::MODULE), Event::Close, Event::Advance],
            );
        }

        /// And a loop that consumes a token before opening its node.
        #[test]
        #[should_panic(expected = "token 0 (byte 0, \"fn\") is consumed \
                                   outside any node")]
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

        /// The source is empty, so this also pins the past-the-end arm of
        /// `at_token` as it is actually rendered into a message.
        #[test]
        #[should_panic(expected = "unbalanced Close: no node is open, while \
                                   the token cursor was at 0 (past the end of \
                                   a 0-token stream)")]
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
        #[should_panic(expected = "dropped 1 token(s) starting at 0 \
                                   (byte 0, \"fn\")")]
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
