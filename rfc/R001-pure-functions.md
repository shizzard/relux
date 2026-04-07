# R001: Pure Functions

- **Status**: implemented
- **Created**: 2026-03-03

## Motivation

Regular functions execute in the caller's shell context and can only be called inside `shell` blocks. Pure functions remove this restriction — they operate exclusively on strings and can be called from any expression context, including `let` declarations at test/effect scope and overlay values in `need` declarations.

## Syntax

```
pure fn format_url(host, port) {
    let base = replace(host, "https://", "")
    "${base}:${port}/api"
}
```

The `pure` keyword before `fn` is the only syntactic addition. The body accepts `let`, assignment, string literals, variable references, and calls to pure functions/BIFs. Shell operators (`>`, `=>`, `<=`, `<?`, `~`, `!=`, `!?`) are forbidden — this is enforced at parse time, not just at resolve time.

## Call sites

Pure functions can be called from:

- Inside `shell` blocks (like regular functions)
- `let` declarations at test/effect scope (before any shell block)
- Overlay values in `need` declarations
- Other pure function bodies

Regular functions remain restricted to `shell` blocks.

## Argument types by context

When calling a pure function, the argument expressions are evaluated at the call site. The allowed argument types depend on the calling context:

| Call site | Argument type | Example |
|-----------|--------------|---------|
| Shell block (impure) | Any expression, including shell ops | `let x = fmt(<= some match)` |
| Test/effect scope | Pure expressions only | `let x = fmt("hello")` |
| Pure function body | Pure expressions only | `let y = other_pure(a, b)` |

Arguments evaluate at the call site and arrive as plain strings. The pure function itself never sees the expression that produced them.

## Type-level enforcement

Pure functions use a separate type hierarchy in both the AST and IR. The `PureExpr` enum lacks `Send`, `Match`, and `TimedMatch` variants — a pure function body containing shell operations cannot be constructed at the type level.

Duplicated types (small, structurally identical to their impure counterparts but parameterized over `PureExpr`):

- `PureStmt` — `Let`, `Assign`, `Expr` (no `Timeout`, `FailRegex`, `FailLiteral`)
- `PureExpr` — `String`, `Var`, `Call` (no `Send`, `Match`, etc.)
- `PureCallExpr`, `PureLetStmt`, `PureAssignStmt` — same fields, `PureExpr` instead of `AstExpr`

## Pure BIFs

A separate `PureBif` trait with no `VmContext` parameter:

```rust
#[async_trait]
pub trait PureBif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, args: Vec<String>, span: &Span) -> Result<String, Failure>;
}
```

### Pure BIFs

| BIF                         | Notes |
|-----------------------------|-------|
| `trim(s)`                   |       |
| `upper(s)`                  |       |
| `lower(s)`                  |       |
| `replace(s, from, to)`      |       |
| `split(s, sep, idx)`        |       |
| `len(s)`                    |       |
| `uuid()`                    |       |
| `rand(n)` / `rand(n, mode)` |       |
| `sleep(duration)`           |       |
| `log(s)`                    |       |
| `annotate(s)`               |       |

### Impure BIFs (shell context required)

| BIF                                                                | Why impure                      |
|--------------------------------------------------------------------|---------------------------------|
| `match_prompt()`                                                   | Reads shell output              |
| `match_exit_code(code)`                                            | Sends to shell and reads output |
| `match_ok()`                                                       | Sends to shell and reads output |
| `ctrl_c()`, `ctrl_d()`, `ctrl_z()`, `ctrl_l()`, `ctrl_backslash()` | Sends bytes to shell            |

## Runtime

Pure functions are evaluated by a standalone async function that takes a `ScopeStack` but no `Vm`:

```rust
async fn eval_pure_expr(
    expr: &Spanned<ir::PureExpr>,
    scope: &mut ScopeStack,
    code: &PureCodeServer,
) -> Result<String, Failure>
```

When a `Vm` encounters a call to a pure function, it delegates to this evaluator, passing its own scope stack. The pure function executes using the caller's variable scope but without touching any shell.

## A note on purity

The term "pure" in Relux differs from its meaning in functional programming. In the FP sense, a pure function is deterministic and free of side effects — calling it with the same arguments always produces the same result with no observable effects.

Several Relux pure BIFs violate this definition:

- `uuid()` and `rand()` return different values on each call (non-deterministic)
- `sleep()` has a time-based side effect
- `log()` and `annotate()` produce observable output

In Relux, "pure" means **shell-independent**: the function does not read from or write to any shell. It operates on string values only and does not require a PTY or output buffer. This is a weaker guarantee than FP purity, but it is the guarantee that matters for Relux — it determines where a function can be called (inside or outside a shell context).
