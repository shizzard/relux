//! The recursive-descent grammar.
//!
//! At this task it is a single production that consumes the whole file into one
//! `MODULE` node, which is what lets T03's losslessness property run before any
//! real grammar exists. T06 replaces the loop body with item dispatch; the
//! `open`/`close` around it survives unchanged.

use crate::parser::Parser;
use crate::syntax_kind::SyntaxKind;

/// Parse a whole module.
///
/// Every token is consumed, so the tree built from these events reproduces the
/// source byte for byte. The grammar owns the root node: the builder never
/// wraps, so exactly one `MODULE` must be opened here.
pub fn module(p: &mut Parser<'_>) {
    let m = p.open();
    while !p.eof() {
        p.advance();
    }
    p.close(m, SyntaxKind::MODULE);
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parser::Event;

    #[test]
    fn module_opens_a_single_root_for_empty_input() {
        let mut p = Parser::new("");

        module(&mut p);

        let (_, events) = p.finish();
        assert_eq!(
            events,
            vec![
                Event::Open {
                    kind: SyntaxKind::MODULE
                },
                Event::Close
            ]
        );
    }

    #[test]
    fn module_consumes_every_token() {
        let source = "test \"a\"\n  send \"b\"\n";
        let mut p = Parser::new(source);

        module(&mut p);

        let (tokens, events) = p.finish();
        let advances = events
            .iter()
            .filter(|e| matches!(e, Event::Advance))
            .count();
        assert_eq!(
            advances,
            tokens.len(),
            "every token must be consumed or the tree cannot be lossless"
        );
    }

    #[test]
    fn module_wraps_everything_in_one_root() {
        let mut p = Parser::new("fn a");

        module(&mut p);

        let (_, events) = p.finish();
        assert!(matches!(
            events.first(),
            Some(Event::Open {
                kind: SyntaxKind::MODULE
            })
        ));
        assert!(matches!(events.last(), Some(Event::Close)));
    }
}
