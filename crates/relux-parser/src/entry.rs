//! Fragment entry points for the CST front end.
//!
//! One function per construct that a `#[cfg(test)]` helper needs to parse in
//! isolation, plus `module`, which `parse()` dispatches to. Each is the same
//! four steps: build a `Parser`, run its grammar rule, build the tree, lower it.
//!
//! Only `module` has a body today, and it lowers to nothing -- T03 shipped a
//! stub grammar that swallows the whole file into one `MODULE` node. The rest
//! are shims naming the grammar task that replaces them, so a panic in the
//! conformance run says which task owns the gap.

use relux_ast::AstCleanupBlock;
use relux_ast::AstEffectDef;
use relux_ast::AstExpr;
use relux_ast::AstFnDef;
use relux_ast::AstImport;
use relux_ast::AstInterpolation;
use relux_ast::AstMarkerDecl;
use relux_ast::AstModule;
use relux_ast::AstOverlayEntry;
use relux_ast::AstPureFnDef;
use relux_ast::AstShellBlock;
use relux_ast::AstStartDecl;
use relux_ast::AstStmt;
use relux_ast::AstTestDef;
use relux_core::Spanned;
use rowan::GreenNode;

use crate::ParseError;
use crate::Span;
use crate::builder::build_tree;
use crate::grammar;
use crate::parser::Parser;

/// Parse `source` into a lossless green tree. The tree is the real product of
/// the CST front end; `module` below is the T04 placeholder that throws it
/// away, and T06 is what starts lowering it.
pub fn module_green(source: &str) -> GreenNode {
    let mut p = Parser::new(source);
    grammar::module(&mut p);
    let (tokens, events) = p.finish();
    build_tree(source, &tokens, &events)
}

/// The whole-file entry point. `parse()` dispatches here under the feature.
pub fn module(source: &str) -> Result<AstModule, ParseError> {
    // Built and discarded until T06 lowers it. This keeps `parse()` on the
    // path `module_green_reproduces_its_source` pins, so a builder panic
    // surfaces through the front-end switch and not only in the corpus test.
    // That test pins losslessness and nothing more: `to_string()` concatenates
    // leaf text, so a tree whose every node kind is wrong still reproduces its
    // source exactly -- the same caveat `builder.rs` records about its own
    // tests. Structural coverage arrives with T06's lowering.
    let _green = module_green(source);

    Ok(AstModule {
        items: vec![],
        span: Span::new(0, source.len()),
    })
}

pub fn import(_source: &str) -> Result<AstImport, ParseError> {
    unimplemented!("T10")
}

pub fn expr(_source: &str) -> Result<AstExpr, ParseError> {
    unimplemented!("T11")
}

pub fn interp_literal(_source: &str) -> Result<AstInterpolation, ParseError> {
    unimplemented!("T12")
}

pub fn interp_regex(_source: &str) -> Result<AstInterpolation, ParseError> {
    unimplemented!("T12")
}

pub fn comment(_source: &str) -> Result<String, ParseError> {
    unimplemented!("T13")
}

pub fn docstring(_source: &str) -> Result<String, ParseError> {
    unimplemented!("T13")
}

pub fn marker(_source: &str) -> Result<AstMarkerDecl, ParseError> {
    unimplemented!("T14")
}

pub fn stmt(_source: &str) -> Result<AstStmt, ParseError> {
    unimplemented!("T15")
}

pub fn shell_block(_source: &str) -> Result<AstShellBlock, ParseError> {
    unimplemented!("T17")
}

/// `shell Effect.name { .. }`. T17 will most likely make this an alias of
/// `shell_block`; it is separate here because `block.rs` has a separate
/// combinator and the helper's semantics must be preserved exactly.
pub fn qualified_shell_block(_source: &str) -> Result<AstShellBlock, ParseError> {
    unimplemented!("T17")
}

pub fn cleanup_block(_source: &str) -> Result<AstCleanupBlock, ParseError> {
    unimplemented!("T17")
}

pub fn fn_def(_source: &str) -> Result<AstFnDef, ParseError> {
    unimplemented!("T18")
}

pub fn pure_fn_def(_source: &str) -> Result<AstPureFnDef, ParseError> {
    unimplemented!("T18")
}

pub fn start_decl(_source: &str) -> Result<AstStartDecl, ParseError> {
    unimplemented!("T19")
}

pub fn overlay(_source: &str) -> Result<Vec<Spanned<AstOverlayEntry>>, ParseError> {
    unimplemented!("T19")
}

pub fn effect(_source: &str) -> Result<AstEffectDef, ParseError> {
    unimplemented!("T20")
}

pub fn test_def(_source: &str) -> Result<AstTestDef, ParseError> {
    unimplemented!("T21")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Without this, the entire CST path can be deleted from `module` and every
    // test in the workspace stays green -- which is exactly what a review
    // mutation demonstrated. Losslessness is the one property the front end
    // has at T04, so it is the one worth pinning.
    #[test]
    fn module_green_reproduces_its_source() {
        for src in [
            "fn dummy() {}\n",
            "test \"t\" {\n    shell s {\n        > echo hi\n        <? ^hi$\n    }\n}\n",
            "// crlf\r\nfn f() {}\r\n",
            "",
            "fn f() {}",
            "// \u{00e9} \u{4e2d}\u{6587}\nfn f() {}\n",
        ] {
            assert_eq!(
                module_green(src).to_string(),
                src,
                "tree is not lossless for {src:?}"
            );
        }
    }
}
