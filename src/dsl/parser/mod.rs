pub mod ast;

use crate::Spanned;
use crate::dsl::lexer::{PayloadFragment, StringFragment, Token, lex};
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub use ast::*;

fn sp(s: SimpleSpan) -> Span {
    s.start..s.end
}

// ─── Fragment Conversion ─────────────────────────────────────

fn payload_to_expr(fragments: Vec<PayloadFragment<'_>>) -> AstStringExpr {
    let mut parts: Vec<AstStringPart> = fragments
        .into_iter()
        .map(|f| match f {
            PayloadFragment::Text(s) => AstStringPart::Literal(s.to_string()),
            PayloadFragment::Interpolation(s) => AstStringPart::Interp(s.to_string()),
            PayloadFragment::EscapedDollar => AstStringPart::EscapedDollar,
        })
        .collect();
    if let Some(AstStringPart::Literal(s)) = parts.first_mut() {
        *s = s.trim_start().to_string();
    }
    parts.retain(|p| !matches!(p, AstStringPart::Literal(s) if s.is_empty()));
    AstStringExpr { parts }
}

fn string_to_expr(fragments: Vec<StringFragment<'_>>) -> AstStringExpr {
    let parts = fragments
        .into_iter()
        .map(|f| match f {
            StringFragment::Text(s) => AstStringPart::Literal(s.to_string()),
            StringFragment::Interpolation(s) => AstStringPart::Interp(s.to_string()),
            StringFragment::Escape(s) => AstStringPart::Escape(s.to_string()),
        })
        .collect();
    AstStringExpr { parts }
}

fn string_to_plain(fragments: Vec<StringFragment<'_>>) -> String {
    fragments
        .into_iter()
        .map(|f| match f {
            StringFragment::Text(s) | StringFragment::Escape(s) => s.to_string(),
            StringFragment::Interpolation(s) => s.to_string(),
        })
        .collect()
}

// ─── Parser ──────────────────────────────────────────────────

fn module_parser<'t, 'src: 't, I>()
-> impl Parser<'t, I, Module, extra::Err<Rich<'t, Token<'src>>>> + Clone
where
    I: ValueInput<'t, Token = Token<'src>, Span = SimpleSpan>,
{
    // ── helpers ──

    let newlines = just(Token::Newline)
        .repeated()
        .ignored()
        .labelled("newline");

    let trivia = select! {
        Token::Newline => (),
        Token::Comment(_) => (),
    }
    .repeated()
    .ignored();

    let ident = select! { Token::Ident(s) => s.to_string() }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("identifier");

    let effect_ident = select! { Token::EffectIdent(s) => s.to_string() }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("effect identifier");

    let any_ident = select! {
        Token::Ident(s) => s.to_string(),
        Token::EffectIdent(s) => s.to_string(),
    }
    .map_with(|s, e| Spanned::new(s, sp(e.span())))
    .labelled("any identifier");

    let import_path = select! {
        Token::ModulePath(s) => s.to_string(),
        Token::Ident(s) => s.to_string(),
    }
    .map_with(|s, e| Spanned::new(s, sp(e.span())))
    .labelled("import path");

    // ── expressions ──

    let arg_expr = recursive(|arg_expr| {
        let string_lit = select! {
            Token::String(fragments) => AstExpr::String(string_to_expr(fragments)),
        }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("string literal");

        let var = select! {
            Token::Interpolation(s) => AstExpr::Var(s.to_string()),
        }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("interpolation");

        let call = ident
            .clone()
            .then(
                arg_expr
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::ParenOpen), just(Token::ParenClose)),
            )
            .map_with(|(name, args), e| {
                Spanned::new(AstExpr::Call(CallExpr { name, args }), sp(e.span()))
            })
            .labelled("function call");

        let number_lit = select! {
            Token::Number(n) => AstExpr::String(AstStringExpr {
                parts: vec![AstStringPart::Literal(n.to_string())],
            }),
        }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("number");

        let ident_var = ident
            .clone()
            .map(|name| Spanned::new(AstExpr::Var(name.node), name.span))
            .labelled("variable");

        choice((string_lit, var, call, number_lit, ident_var))
    })
    .labelled("argument expression");

    let send = select! { Token::Send(f) => AstExpr::Send(payload_to_expr(f)) }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("send");
    let send_raw = select! { Token::SendRaw(f) => AstExpr::SendRaw(payload_to_expr(f)) }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("send raw");
    let match_regex = select! { Token::MatchRegex(f) => AstExpr::MatchRegex(payload_to_expr(f)) }
        .map_with(|e, x| Spanned::new(e, sp(x.span())))
        .labelled("match regex");
    let match_literal =
        select! { Token::MatchLiteral(f) => AstExpr::MatchLiteral(payload_to_expr(f)) }
            .map_with(|e, x| Spanned::new(e, sp(x.span())))
            .labelled("match literal");

    let expr = choice((
        send.clone(),
        send_raw.clone(),
        match_regex,
        match_literal,
        arg_expr.clone(),
    ))
    .labelled("expression");

    // ── statements ──

    let let_stmt = just(Token::Let)
        .ignore_then(ident.clone())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .map_with(|(name, value), e| Spanned::new(Stmt::Let(LetStmt { name, value }), sp(e.span())))
        .labelled("variable declaration");

    let assign_stmt = ident
        .clone()
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map_with(|(name, value), e| {
            Spanned::new(Stmt::Assign(AssignStmt { name, value }), sp(e.span()))
        })
        .labelled("assignment");

    let timeout_stmt = select! { Token::Timeout(s) => Stmt::Timeout(s.to_string()) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("timeout");

    let fail_regex_stmt = select! { Token::FailRegex(f) => Stmt::FailRegex(payload_to_expr(f)) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("fail pattern");

    let fail_literal_stmt =
        select! { Token::FailLiteral(f) => Stmt::FailLiteral(payload_to_expr(f)) }
            .map_with(|s, e| Spanned::new(s, sp(e.span())))
            .labelled("literal fail pattern");

    let comment_stmt = select! { Token::Comment(s) => Stmt::Comment(s.to_string()) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("comment");

    let expr_stmt = expr
        .clone()
        .map(|spanned| Spanned::new(Stmt::Expr(spanned.node), spanned.span))
        .labelled("expression");

    let stmt = choice((
        let_stmt,
        assign_stmt,
        timeout_stmt,
        fail_regex_stmt,
        fail_literal_stmt,
        comment_stmt,
        expr_stmt,
    ))
    .labelled("statement");

    // ── cleanup statements ──

    let cleanup_send = select! { Token::Send(f) => CleanupStmt::Send(payload_to_expr(f)) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("send");
    let cleanup_send_raw =
        select! { Token::SendRaw(f) => CleanupStmt::SendRaw(payload_to_expr(f)) }
            .map_with(|s, e| Spanned::new(s, sp(e.span())))
            .labelled("send raw");
    let cleanup_let = just(Token::Let)
        .ignore_then(ident.clone())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .map_with(|(name, value), e| {
            Spanned::new(CleanupStmt::Let(LetStmt { name, value }), sp(e.span()))
        })
        .labelled("variable declaration");
    let cleanup_assign = ident
        .clone()
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map_with(|(name, value), e| {
            Spanned::new(
                CleanupStmt::Assign(AssignStmt { name, value }),
                sp(e.span()),
            )
        })
        .labelled("assignment");
    let cleanup_comment = select! { Token::Comment(s) => CleanupStmt::Comment(s.to_string()) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("comment");

    let cleanup_stmt = choice((
        cleanup_let,
        cleanup_assign,
        cleanup_comment,
        cleanup_send,
        cleanup_send_raw,
    ))
    .labelled("cleanup statement");

    // ── blocks ──

    let shell_block = just(Token::Shell)
        .ignore_then(ident.clone())
        .then(
            just(Token::BraceOpen)
                .ignore_then(
                    newlines
                        .clone()
                        .ignore_then(stmt.clone())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(newlines.clone())
                .then_ignore(just(Token::BraceClose)),
        )
        .map_with(|(name, stmts), e| Spanned::new(ShellBlock { name, stmts }, sp(e.span())))
        .labelled("shell block");

    let cleanup_block = just(Token::Cleanup)
        .ignore_then(
            just(Token::BraceOpen)
                .ignore_then(
                    newlines
                        .clone()
                        .ignore_then(cleanup_stmt)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(newlines.clone())
                .then_ignore(just(Token::BraceClose)),
        )
        .map_with(|stmts, e| Spanned::new(CleanupBlock { stmts }, sp(e.span())))
        .labelled("cleanup block");

    let overlay_entry = any_ident
        .clone()
        .then_ignore(just(Token::Eq))
        .then(arg_expr.clone())
        .map_with(|(key, value), e| Spanned::new(OverlayEntry { key, value }, sp(e.span())))
        .labelled("overlay entry");

    let overlay = just(Token::BraceOpen)
        .ignore_then(
            newlines
                .clone()
                .ignore_then(overlay_entry)
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(newlines.clone())
        .then_ignore(just(Token::BraceClose))
        .labelled("overlay");

    let need_decl = just(Token::Need)
        .ignore_then(effect_ident.clone())
        .then(just(Token::As).ignore_then(ident.clone()).or_not())
        .then(overlay.or_not())
        .map_with(|((effect, alias), overlay), e| {
            Spanned::new(
                NeedDecl {
                    effect,
                    alias,
                    overlay: overlay.unwrap_or_default(),
                },
                sp(e.span()),
            )
        })
        .labelled("need declaration");

    // ── import ──

    let fn_import_name = ident
        .clone()
        .then(just(Token::As).ignore_then(ident.clone()).or_not())
        .map_with(|(name, alias), e| Spanned::new(ImportName { name, alias }, sp(e.span())))
        .labelled("function import name");

    let effect_import_name = effect_ident
        .clone()
        .then(just(Token::As).ignore_then(effect_ident.clone()).or_not())
        .map_with(|(name, alias), e| Spanned::new(ImportName { name, alias }, sp(e.span())))
        .labelled("effect import name");

    let import_name = choice((fn_import_name, effect_import_name)).labelled("import name");
    let selective = just(Token::BraceOpen)
        .ignore_then(trivia.clone())
        .ignore_then(
            import_name
                .separated_by(just(Token::Comma).then_ignore(trivia.clone()))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(trivia.clone())
        .then_ignore(just(Token::BraceClose))
        .labelled("selective import");

    let import = just(Token::Import)
        .ignore_then(import_path)
        .then(selective.or_not())
        .map_with(|(path, names), e| {
            Spanned::new(Item::Import(Import { path, names }), sp(e.span()))
        })
        .labelled("import");

    // ── function definition ──

    let params = ident
        .clone()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::ParenOpen), just(Token::ParenClose))
        .labelled("function parameters");

    let fn_def = just(Token::Fn)
        .ignore_then(ident.clone())
        .then(params)
        .then(
            just(Token::BraceOpen)
                .ignore_then(
                    newlines
                        .clone()
                        .ignore_then(stmt)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(newlines.clone())
                .then_ignore(just(Token::BraceClose)),
        )
        .map_with(|((name, params), body), e| {
            Spanned::new(Item::Fn(FnDef { name, params, body }), sp(e.span()))
        })
        .labelled("function definition");

    // ── effect definition ──

    let effect_comment = select! { Token::Comment(s) => EffectItem::Comment(s.to_string()) }
        .map_with(|item, e| Spanned::new(item, sp(e.span())))
        .labelled("comment");
    let effect_need = need_decl
        .clone()
        .map(|s| Spanned::new(EffectItem::Need(s.node), s.span))
        .labelled("need declaration");
    let effect_let = just(Token::Let)
        .ignore_then(ident.clone())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .map_with(|(name, value), e| {
            Spanned::new(EffectItem::Let(LetStmt { name, value }), sp(e.span()))
        })
        .labelled("variable declaration");
    let effect_shell = shell_block
        .clone()
        .map(|s| Spanned::new(EffectItem::Shell(s.node), s.span))
        .labelled("shell block");
    let effect_cleanup = cleanup_block
        .clone()
        .map(|s| Spanned::new(EffectItem::Cleanup(s.node), s.span))
        .labelled("cleanup block");

    let effect_item = choice((
        effect_need,
        effect_let,
        effect_shell,
        effect_cleanup,
        effect_comment,
    ))
    .labelled("effect item");

    let effect_def = just(Token::Effect)
        .ignore_then(effect_ident.clone())
        .then_ignore(just(Token::Arrow))
        .then_ignore(just(Token::Shell))
        .then(ident.clone())
        .then(
            just(Token::BraceOpen)
                .ignore_then(
                    newlines
                        .clone()
                        .ignore_then(effect_item)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(newlines.clone())
                .then_ignore(just(Token::BraceClose)),
        )
        .map_with(|((name, exported_shell), body), e| {
            Spanned::new(
                Item::Effect(EffectDef {
                    name,
                    exported_shell,
                    body,
                }),
                sp(e.span()),
            )
        })
        .labelled("effect definition");

    // ── test definition ──

    let test_name = select! { Token::String(fragments) => string_to_plain(fragments) }
        .map_with(|s, e| Spanned::new(s, sp(e.span())))
        .labelled("test name");

    let test_comment = select! { Token::Comment(s) => TestItem::Comment(s.to_string()) }
        .map_with(|item, e| Spanned::new(item, sp(e.span())))
        .labelled("comment");
    let test_docstring = select! { Token::DocString(parts) => TestItem::DocString(parts.concat()) }
        .map_with(|item, e| Spanned::new(item, sp(e.span())))
        .labelled("docstring");
    let test_need = need_decl
        .map(|s| Spanned::new(TestItem::Need(s.node), s.span))
        .labelled("need declaration");
    let test_let = just(Token::Let)
        .ignore_then(ident.clone())
        .then(just(Token::Eq).ignore_then(expr).or_not())
        .map_with(|(name, value), e| {
            Spanned::new(TestItem::Let(LetStmt { name, value }), sp(e.span()))
        })
        .labelled("variable declaration");
    let test_shell = shell_block
        .map(|s| Spanned::new(TestItem::Shell(s.node), s.span))
        .labelled("shell block");
    let test_cleanup = cleanup_block
        .map(|s| Spanned::new(TestItem::Cleanup(s.node), s.span))
        .labelled("cleanup block");

    let test_item = choice((
        test_docstring,
        test_need,
        test_let,
        test_shell,
        test_cleanup,
        test_comment,
    ))
    .labelled("test item");

    let test_def = just(Token::Test)
        .ignore_then(test_name)
        .then(
            just(Token::BraceOpen)
                .ignore_then(
                    newlines
                        .clone()
                        .ignore_then(test_item)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(newlines.clone())
                .then_ignore(just(Token::BraceClose)),
        )
        .map_with(|(name, body), e| Spanned::new(Item::Test(TestDef { name, body }), sp(e.span())))
        .labelled("test definition");

    // ── top-level items ──

    let comment = select! { Token::Comment(s) => Item::Comment(s.to_string()) }
        .map_with(|item, e| Spanned::new(item, sp(e.span())))
        .labelled("comment");

    let item = choice((import, fn_def, effect_def, test_def, comment)).labelled("module item");

    // ── module ──

    newlines
        .clone()
        .ignore_then(item)
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(newlines)
        .then_ignore(end())
        .map(|items| Module { items })
}

// ─── Public API ──────────────────────────────────────────────

pub type ParseError = chumsky::error::Rich<'static, String, SimpleSpan>;

pub fn parse(source: &str) -> (Option<Module>, Vec<ParseError>) {
    let tokens = lex(source);
    let token_spans: Vec<(Token<'_>, SimpleSpan)> = tokens
        .into_iter()
        .map(|s| (s.node, SimpleSpan::from(s.span)))
        .collect();
    let eoi = SimpleSpan::from(source.len()..source.len());

    let (output, errors) = module_parser()
        .parse(token_spans.as_slice().split_token_span(eoi))
        .into_output_errors();

    let owned_errors = errors
        .into_iter()
        .map(|e| e.map_token(|t| t.to_string()).into_owned())
        .collect();

    (output, owned_errors)
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(source: &str) -> Module {
        let (module, errors) = parse(source);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        module.expect("expected parsed module")
    }

    // ── Phase 1: imports ──

    #[test]
    fn test_import_wildcard() {
        let m = parse_ok("import lib/module1\n");
        assert_eq!(m.items.len(), 1);
        match &m.items[0].node {
            Item::Import(imp) => {
                assert_eq!(imp.path.node, "lib/module1");
                assert!(imp.names.is_none());
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_import_selective() {
        let m = parse_ok("import lib/module1 { foo, bar as b }\n");
        assert_eq!(m.items.len(), 1);
        match &m.items[0].node {
            Item::Import(imp) => {
                assert_eq!(imp.path.node, "lib/module1");
                let names = imp.names.as_ref().unwrap();
                assert_eq!(names.len(), 2);
                assert_eq!(names[0].node.name.node, "foo");
                assert!(names[0].node.alias.is_none());
                assert_eq!(names[1].node.name.node, "bar");
                assert_eq!(names[1].node.alias.as_ref().unwrap().node, "b");
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_import_selective_trailing_comma() {
        let m = parse_ok("import lib/m { foo, bar, }\n");
        match &m.items[0].node {
            Item::Import(imp) => {
                assert_eq!(imp.names.as_ref().unwrap().len(), 2);
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_import_selective_multiline() {
        let m = parse_ok("import lib/m {\n  foo,\n  bar as b\n}\n");
        match &m.items[0].node {
            Item::Import(imp) => {
                let names = imp.names.as_ref().unwrap();
                assert_eq!(names.len(), 2);
                assert_eq!(names[0].node.name.node, "foo");
                assert_eq!(names[1].node.name.node, "bar");
                assert_eq!(names[1].node.alias.as_ref().unwrap().node, "b");
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_import_single_segment_path() {
        let m = parse_ok("import helpers\n");
        match &m.items[0].node {
            Item::Import(imp) => {
                assert_eq!(imp.path.node, "helpers");
                assert!(imp.names.is_none());
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_item() {
        let m = parse_ok("# hello world\n");
        assert_eq!(m.items.len(), 1);
        match &m.items[0].node {
            Item::Comment(s) => assert_eq!(s, " hello world"),
            other => panic!("expected Comment, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_module() {
        let m = parse_ok("");
        assert!(m.items.is_empty());
    }

    #[test]
    fn test_blank_lines() {
        let m = parse_ok("\n\n\n");
        assert!(m.items.is_empty());
    }

    // ── Phase 2: expressions ──

    #[test]
    fn test_expr_string() {
        let m = parse_ok("fn f() {\n  let x = \"hello\"\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => {
                assert_eq!(f.body.len(), 1);
                match &f.body[0].node {
                    Stmt::Let(l) => {
                        let val = l.value.as_ref().unwrap();
                        match &val.node {
                            AstExpr::String(s) => {
                                assert_eq!(s.parts, vec![AstStringPart::Literal("hello".into())]);
                            }
                            other => panic!("expected String, got {other:?}"),
                        }
                    }
                    other => panic!("expected Let, got {other:?}"),
                }
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_expr_var() {
        let m = parse_ok("fn f() {\n  ${myvar}\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Expr(AstExpr::Var(s)) => assert_eq!(s, "myvar"),
                other => panic!("expected Var, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_expr_call_no_args() {
        let m = parse_ok("fn f() {\n  match_uuid()\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Expr(AstExpr::Call(c)) => {
                    assert_eq!(c.name.node, "match_uuid");
                    assert!(c.args.is_empty());
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_expr_call_with_args() {
        let m = parse_ok("fn f() {\n  greet(\"a\", \"b\")\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Expr(AstExpr::Call(c)) => {
                    assert_eq!(c.name.node, "greet");
                    assert_eq!(c.args.len(), 2);
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_expr_send() {
        let m = parse_ok("fn f() {\n  > echo hello\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Expr(AstExpr::Send(s)) => {
                    assert_eq!(s.parts, vec![AstStringPart::Literal("echo hello".into())]);
                }
                other => panic!("expected Send, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_expr_send_interp() {
        let m = parse_ok("fn f() {\n  > echo ${name}\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Expr(AstExpr::Send(s)) => {
                    assert_eq!(
                        s.parts,
                        vec![
                            AstStringPart::Literal("echo ".into()),
                            AstStringPart::Interp("name".into()),
                        ]
                    );
                }
                other => panic!("expected Send, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    // ── Phase 3: statements ──

    #[test]
    fn test_let_uninitialized() {
        let m = parse_ok("fn f() {\n  let x\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Let(l) => {
                    assert_eq!(l.name.node, "x");
                    assert!(l.value.is_none());
                }
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_let_with_value() {
        let m = parse_ok("fn f() {\n  let x = \"val\"\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Let(l) => {
                    assert_eq!(l.name.node, "x");
                    assert!(l.value.is_some());
                }
                other => panic!("expected Let, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_assign() {
        let m = parse_ok("fn f() {\n  x = ${1}\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Assign(a) => {
                    assert_eq!(a.name.node, "x");
                    match &a.value.node {
                        AstExpr::Var(s) => assert_eq!(s, "1"),
                        other => panic!("expected Var, got {other:?}"),
                    }
                }
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_timeout() {
        let m = parse_ok("fn f() {\n  ~10s\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::Timeout(s) => assert_eq!(s, "10s"),
                other => panic!("expected Timeout, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_fail_regex() {
        let m = parse_ok("fn f() {\n  !? [Ee]rror|FATAL\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::FailRegex(s) => {
                    assert_eq!(
                        s.parts,
                        vec![AstStringPart::Literal("[Ee]rror|FATAL".into())]
                    );
                }
                other => panic!("expected FailRegex, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_fail_literal() {
        let m = parse_ok("fn f() {\n  != error\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => match &f.body[0].node {
                Stmt::FailLiteral(s) => {
                    assert_eq!(s.parts, vec![AstStringPart::Literal("error".into())]);
                }
                other => panic!("expected FailLiteral, got {other:?}"),
            },
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_in_body() {
        let m = parse_ok("fn f() {\n  # a comment\n  let x\n}\n");
        match &m.items[0].node {
            Item::Fn(f) => {
                assert_eq!(f.body.len(), 2);
                match &f.body[0].node {
                    Stmt::Comment(s) => assert_eq!(s, " a comment"),
                    other => panic!("expected Comment, got {other:?}"),
                }
                assert!(matches!(&f.body[1].node, Stmt::Let(_)));
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    // ── Phase 4: blocks ──

    #[test]
    fn test_shell_block() {
        let src = "test \"t\" {\n  shell myshell {\n    > hello\n  }\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => {
                assert_eq!(t.body.len(), 1);
                match &t.body[0].node {
                    TestItem::Shell(sb) => {
                        assert_eq!(sb.name.node, "myshell");
                        assert_eq!(sb.stmts.len(), 1);
                    }
                    other => panic!("expected Shell, got {other:?}"),
                }
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn test_cleanup_block() {
        let src = "test \"t\" {\n  cleanup {\n    > rm -rf /tmp/foo\n  }\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => match &t.body[0].node {
                TestItem::Cleanup(cb) => {
                    assert_eq!(cb.stmts.len(), 1);
                    assert!(matches!(&cb.stmts[0].node, CleanupStmt::Send(_)));
                }
                other => panic!("expected Cleanup, got {other:?}"),
            },
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn test_need_simple() {
        let src = "test \"t\" {\n  need Myeffect as e\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => match &t.body[0].node {
                TestItem::Need(n) => {
                    assert_eq!(n.effect.node, "Myeffect");
                    assert_eq!(n.alias.as_ref().unwrap().node, "e");
                    assert!(n.overlay.is_empty());
                }
                other => panic!("expected Need, got {other:?}"),
            },
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn test_need_no_alias() {
        let src = "test \"t\" {\n  need Myeffect\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => match &t.body[0].node {
                TestItem::Need(n) => {
                    assert_eq!(n.effect.node, "Myeffect");
                    assert!(n.alias.is_none());
                }
                other => panic!("expected Need, got {other:?}"),
            },
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn test_need_with_overlay() {
        let src = "test \"t\" {\n  need Myeffect as e {\n    MY_VAR = \"val\"\n  }\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => match &t.body[0].node {
                TestItem::Need(n) => {
                    assert_eq!(n.effect.node, "Myeffect");
                    assert_eq!(n.alias.as_ref().unwrap().node, "e");
                    assert_eq!(n.overlay.len(), 1);
                    assert_eq!(n.overlay[0].node.key.node, "MY_VAR");
                }
                other => panic!("expected Need, got {other:?}"),
            },
            other => panic!("expected Test, got {other:?}"),
        }
    }

    // ── Phase 5: top-level items ──

    #[test]
    fn test_fn_def() {
        let src = "fn greet(a, b) {\n  > echo ${a}\n  <? echo\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Fn(f) => {
                assert_eq!(f.name.node, "greet");
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].node, "a");
                assert_eq!(f.params[1].node, "b");
                assert_eq!(f.body.len(), 2);
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_fn_no_params() {
        let src = "fn noop() {\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Fn(f) => {
                assert_eq!(f.name.node, "noop");
                assert!(f.params.is_empty());
                assert!(f.body.is_empty());
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn test_effect_def() {
        let src = concat!(
            "effect StartDb -> shell db {\n",
            "  need Dep as d\n",
            "  let x\n",
            "  shell db {\n",
            "    > start\n",
            "  }\n",
            "  cleanup {\n",
            "    > stop\n",
            "  }\n",
            "}\n",
        );
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Effect(e) => {
                assert_eq!(e.name.node, "StartDb");
                assert_eq!(e.exported_shell.node, "db");
                assert_eq!(e.body.len(), 4);
                assert!(matches!(&e.body[0].node, EffectItem::Need(_)));
                assert!(matches!(&e.body[1].node, EffectItem::Let(_)));
                assert!(matches!(&e.body[2].node, EffectItem::Shell(_)));
                assert!(matches!(&e.body[3].node, EffectItem::Cleanup(_)));
            }
            other => panic!("expected Effect, got {other:?}"),
        }
    }

    #[test]
    fn test_test_def() {
        let src = concat!(
            "test \"my test\" {\n",
            "  \"\"\"\n  doc\n  \"\"\"\n",
            "  need E as alias\n",
            "  let x\n",
            "  shell s {\n",
            "    > hi\n",
            "  }\n",
            "  cleanup {\n",
            "    > bye\n",
            "  }\n",
            "}\n",
        );
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Test(t) => {
                assert_eq!(t.name.node, "my test");
                assert!(matches!(&t.body[0].node, TestItem::DocString(_)));
                assert!(matches!(&t.body[1].node, TestItem::Need(_)));
                assert!(matches!(&t.body[2].node, TestItem::Let(_)));
                assert!(matches!(&t.body[3].node, TestItem::Shell(_)));
                assert!(matches!(&t.body[4].node, TestItem::Cleanup(_)));
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn test_multiple_items() {
        let src = concat!("# top comment\n", "import lib/m\n", "fn f() {\n}\n",);
        let m = parse_ok(src);
        assert_eq!(m.items.len(), 3);
        assert!(matches!(&m.items[0].node, Item::Comment(_)));
        assert!(matches!(&m.items[1].node, Item::Import(_)));
        assert!(matches!(&m.items[2].node, Item::Fn(_)));
    }

    // ── Phase 6: span verification ──

    #[test]
    fn test_span_accuracy() {
        let src = "import lib/m\n";
        let m = parse_ok(src);
        let item_span = &m.items[0].span;
        assert_eq!(&src[item_span.clone()], "import lib/m");
    }

    #[test]
    fn test_ident_span() {
        let src = "fn hello() {\n}\n";
        let m = parse_ok(src);
        match &m.items[0].node {
            Item::Fn(f) => {
                assert_eq!(&src[f.name.span.clone()], "hello");
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    // ── Phase 6: integration test ──

    #[test]
    fn test_syntax_demo_parses() {
        let src = include_str!("../../../examples/syntax_demo.relux");
        let (module, errors) = parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let m = module.expect("expected parsed module");

        // Top-level items: comments, 2 imports, 2 fns, 1 effect, 1 test
        let imports: Vec<_> = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, Item::Import(_)))
            .collect();
        let fns: Vec<_> = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, Item::Fn(_)))
            .collect();
        let effects: Vec<_> = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, Item::Effect(_)))
            .collect();
        let tests: Vec<_> = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, Item::Test(_)))
            .collect();
        let comments: Vec<_> = m
            .items
            .iter()
            .filter(|i| matches!(&i.node, Item::Comment(_)))
            .collect();

        assert_eq!(imports.len(), 2, "expected 2 imports");
        assert_eq!(fns.len(), 2, "expected 2 functions");
        assert_eq!(effects.len(), 1, "expected 1 effect");
        assert_eq!(tests.len(), 1, "expected 1 test");
        assert!(
            comments.len() >= 6,
            "expected at least 6 top-level comments"
        );

        // ── imports ──

        match &imports[0].node {
            Item::Import(imp) => {
                assert_eq!(imp.path.node, "lib/module1");
                let names = imp.names.as_ref().expect("selective import");
                assert_eq!(names.len(), 6);
                assert_eq!(names[0].node.name.node, "function1");
                assert!(names[0].node.alias.is_none());
                assert_eq!(names[2].node.name.node, "function3");
                assert_eq!(names[2].node.alias.as_ref().unwrap().node, "f3");
                assert_eq!(names[5].node.name.node, "Effect3");
                assert_eq!(names[5].node.alias.as_ref().unwrap().node, "E3");
            }
            _ => unreachable!(),
        }

        match &imports[1].node {
            Item::Import(imp) => {
                assert_eq!(imp.path.node, "lib/module2");
                assert!(imp.names.is_none());
            }
            _ => unreachable!(),
        }

        // ── functions ──

        match &fns[0].node {
            Item::Fn(f) => {
                assert_eq!(f.name.node, "some_function");
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].node, "arg1");
                assert_eq!(f.params[1].node, "arg2");
                // body has comments + send + send_raw + match_literal + match_regex + let + var ref
                let non_comments: Vec<_> = f
                    .body
                    .iter()
                    .filter(|s| !matches!(&s.node, Stmt::Comment(_)))
                    .collect();
                assert_eq!(non_comments.len(), 6, "some_function non-comment stmts");
            }
            _ => unreachable!(),
        }

        match &fns[1].node {
            Item::Fn(f) => {
                assert_eq!(f.name.node, "match_uuid");
                assert!(f.params.is_empty());
                let non_comments: Vec<_> = f
                    .body
                    .iter()
                    .filter(|s| !matches!(&s.node, Stmt::Comment(_)))
                    .collect();
                assert_eq!(non_comments.len(), 2, "match_uuid: match_regex + var ref");
            }
            _ => unreachable!(),
        }

        // ── effect ──

        match &effects[0].node {
            Item::Effect(e) => {
                assert_eq!(e.name.node, "StartSomething");
                assert_eq!(e.exported_shell.node, "something");

                let needs: Vec<_> = e
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, EffectItem::Need(_)))
                    .collect();
                assert_eq!(needs.len(), 3, "effect has 3 needs");

                // Third need has overlay
                match &needs[2].node {
                    EffectItem::Need(n) => {
                        assert_eq!(n.effect.node, "E3");
                        assert_eq!(n.alias.as_ref().unwrap().node, "e3");
                        assert_eq!(n.overlay.len(), 1);
                        assert_eq!(n.overlay[0].node.key.node, "E3_VAR");
                    }
                    _ => unreachable!(),
                }

                let shells: Vec<_> = e
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, EffectItem::Shell(_)))
                    .collect();
                assert_eq!(shells.len(), 2, "effect has 2 shell blocks");

                let cleanups: Vec<_> = e
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, EffectItem::Cleanup(_)))
                    .collect();
                assert_eq!(cleanups.len(), 1, "effect has 1 cleanup");
            }
            _ => unreachable!(),
        }

        // ── test ──

        match &tests[0].node {
            Item::Test(t) => {
                assert_eq!(t.name.node, "Some test");

                let docstrings: Vec<_> = t
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, TestItem::DocString(_)))
                    .collect();
                assert_eq!(docstrings.len(), 1);

                let needs: Vec<_> = t
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, TestItem::Need(_)))
                    .collect();
                assert_eq!(needs.len(), 2, "test has 2 needs");

                // Second need has overlay
                match &needs[1].node {
                    TestItem::Need(n) => {
                        assert_eq!(n.effect.node, "StartSomething");
                        assert_eq!(n.alias.as_ref().unwrap().node, "another_something_shell");
                        assert_eq!(n.overlay.len(), 1);
                    }
                    _ => unreachable!(),
                }

                let shells: Vec<_> = t
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, TestItem::Shell(_)))
                    .collect();
                assert_eq!(shells.len(), 2, "test has 2 shell blocks");

                // First shell block (myshell) has timeouts, fail patterns, lets, sends, matches
                match &shells[0].node {
                    TestItem::Shell(sb) => {
                        assert_eq!(sb.name.node, "myshell");
                        let timeouts: Vec<_> = sb
                            .stmts
                            .iter()
                            .filter(|s| matches!(&s.node, Stmt::Timeout(_)))
                            .collect();
                        assert_eq!(timeouts.len(), 2, "myshell has 2 timeouts");
                    }
                    _ => unreachable!(),
                }

                // Second shell block (something_shell) has let+call, send, match
                match &shells[1].node {
                    TestItem::Shell(sb) => {
                        assert_eq!(sb.name.node, "something_shell");
                        let non_comments: Vec<_> = sb
                            .stmts
                            .iter()
                            .filter(|s| !matches!(&s.node, Stmt::Comment(_)))
                            .collect();
                        assert_eq!(
                            non_comments.len(),
                            4,
                            "something_shell: 2 lets + send + match"
                        );
                    }
                    _ => unreachable!(),
                }

                let cleanups: Vec<_> = t
                    .body
                    .iter()
                    .filter(|i| matches!(&i.node, TestItem::Cleanup(_)))
                    .collect();
                assert_eq!(cleanups.len(), 1, "test has 1 cleanup");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_syntax_demo_spans_match_source() {
        let src = include_str!("../../../examples/syntax_demo.relux");
        let m = parse_ok(src);

        // Verify function name span points to correct source text
        let fn_item = m
            .items
            .iter()
            .find(|i| matches!(&i.node, Item::Fn(f) if f.name.node == "some_function"))
            .expect("some_function not found");
        match &fn_item.node {
            Item::Fn(f) => {
                assert_eq!(&src[f.name.span.clone()], "some_function");
            }
            _ => unreachable!(),
        }

        // Verify effect name span
        let eff_item = m
            .items
            .iter()
            .find(|i| matches!(&i.node, Item::Effect(_)))
            .expect("effect not found");
        match &eff_item.node {
            Item::Effect(e) => {
                assert_eq!(&src[e.name.span.clone()], "StartSomething");
                assert_eq!(&src[e.exported_shell.span.clone()], "something");
            }
            _ => unreachable!(),
        }

        // Verify test name span covers the quoted string (without quotes)
        let test_item = m
            .items
            .iter()
            .find(|i| matches!(&i.node, Item::Test(_)))
            .expect("test not found");
        match &test_item.node {
            Item::Test(t) => {
                let name_src = &src[t.name.span.clone()];
                assert!(
                    name_src.starts_with('"'),
                    "test name span should include opening quote"
                );
                assert!(
                    name_src.ends_with('"'),
                    "test name span should include closing quote"
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_error_on_malformed_input() {
        let (module, errors) = parse("fn {\n}\n");
        assert!(!errors.is_empty(), "expected parse errors for malformed fn");
        assert!(module.is_none());
    }

    // ── Negative tests: error span accuracy ──

    fn error_span_text<'s>(source: &'s str, errors: &[ParseError]) -> Vec<&'s str> {
        errors
            .iter()
            .map(|e| {
                let s = e.span().start;
                let end = e.span().end;
                &source[s..end]
            })
            .collect()
    }

    #[test]
    fn test_error_span_fn_missing_name() {
        let src = "fn (\n)\n";
        //         0123
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(spans[0], "(", "error should point at the unexpected '('");
    }

    #[test]
    fn test_error_span_fn_missing_brace_open() {
        let src = "fn foo()\n  > echo hi\n}\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let start = errors[0].span().start;
        assert!(
            start <= src.find('\n').unwrap(),
            "error should be on the first line, got offset {start}",
        );
    }

    #[test]
    fn test_error_span_fn_unclosed_brace() {
        let src = "fn foo() {\n  > echo hi\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let start = errors[0].span().start;
        assert_eq!(
            start,
            src.len(),
            "error should point at end-of-input for unclosed brace",
        );
    }

    #[test]
    fn test_error_span_import_missing_path() {
        let src = "import {\n}\n";
        //         0123456^
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(
            spans[0], "{",
            "error should point at the unexpected '{{' after import"
        );
    }

    #[test]
    fn test_error_span_import_unclosed_selective() {
        let src = "import lib/m { foo, bar\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let start = errors[0].span().start;
        assert_eq!(
            start,
            src.len(),
            "error should point at end-of-input for unclosed selective import",
        );
    }

    #[test]
    fn test_error_span_effect_missing_arrow() {
        //                      v— expects -> here, finds "shell"
        let src = "effect Foo shell bar {\n}\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(
            spans[0], "shell",
            "error should point at the unexpected 'shell' where '->' was expected"
        );
    }

    #[test]
    fn test_error_span_test_missing_name() {
        let src = "test {\n}\n";
        //         01234^
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(
            spans[0], "{",
            "error should point at '{{' where test name string was expected"
        );
    }

    #[test]
    fn test_error_span_unexpected_token_at_toplevel() {
        let src = "let x = 1\n";
        //         ^-- 'let' is not a valid top-level item
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(
            spans[0], "let",
            "error should point at the unexpected 'let' at top level"
        );
    }

    #[test]
    fn test_error_span_invalid_token_between_items() {
        // `let` is a valid token but not a valid top-level item;
        // the parser should error pointing at `let`, not at `import`.
        let src = "import lib/a\nlet x = 1\nimport lib/b\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let start = errors[0].span().start;
        let let_offset = src.find("let").unwrap();
        assert_eq!(
            start, let_offset,
            "error should point at the invalid 'let' between items",
        );
    }

    #[test]
    fn test_error_span_unrecognized_input_between_items() {
        // `!!!` is squashed into a single Unrecognized token by the lexer;
        // the parser should error pointing at its full span.
        let src = "import lib/a\n!!!\nimport lib/b\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let err_start = errors[0].span().start;
        let err_end = errors[0].span().end;
        let bang_offset = src.find("!!!").unwrap();
        assert_eq!(err_start, bang_offset);
        assert_eq!(
            err_end,
            bang_offset + 3,
            "span should cover the entire '!!!' token"
        );
    }

    #[test]
    fn test_error_span_fn_missing_closing_paren() {
        let src = "fn foo(a, b {\n}\n";
        //                    ^-- expects ')' here
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let spans = error_span_text(src, &errors);
        assert_eq!(
            spans[0], "{",
            "error should point at '{{' where ')' was expected"
        );
    }

    #[test]
    fn test_error_span_points_at_eoi() {
        let src = "fn foo(";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        assert_eq!(
            errors[0].span().start,
            src.len(),
            "error should point at end-of-input",
        );
        assert_eq!(
            errors[0].span().end,
            src.len(),
            "eoi span should be zero-width",
        );
    }

    #[test]
    fn test_error_rich_contains_expected_info() {
        let src = "fn foo(a, b {\n}\n";
        let (_, errors) = parse(src);
        assert!(!errors.is_empty());
        let expected: Vec<String> = errors[0].expected().map(|p| p.to_string()).collect();
        assert!(
            expected.iter().any(|e| e.contains(")")),
            "expected patterns should mention ')', got: {expected:?}",
        );
        assert_eq!(
            errors[0].found().map(|t| t.as_str()),
            Some("{"),
            "found token should be '{{'",
        );
    }

    #[test]
    fn test_error_fn_name_must_be_lowercase() {
        let src = "fn Greet() {\n}\n";
        let (_, errors) = parse(src);
        assert!(
            !errors.is_empty(),
            "CamelCase function name should be a parse error"
        );
    }

    #[test]
    fn test_error_effect_name_must_be_uppercase() {
        let src = "effect start_db -> shell db {\n}\n";
        let (_, errors) = parse(src);
        assert!(
            !errors.is_empty(),
            "snake_case effect name should be a parse error"
        );
    }

    #[test]
    fn test_error_need_effect_must_be_uppercase() {
        let src = "test \"t\" {\n  need myeffect as e\n}\n";
        let (_, errors) = parse(src);
        assert!(
            !errors.is_empty(),
            "snake_case effect name in need should be a parse error"
        );
    }

    #[test]
    fn test_error_import_alias_casing_mismatch() {
        let src = "import lib/m { foo as Bar }\n";
        let (_, errors) = parse(src);
        assert!(
            !errors.is_empty(),
            "lowercase name aliased to CamelCase should be a parse error"
        );
    }
}
