# Pure Matching

A **pure match** asserts that a value your test has already computed
satisfies a pattern. Unlike the shell match operators (`<?` / `<=`),
which scan a shell's streaming output buffer for a match, a pure match
compares against a single complete value — a variable, a function
result, an interpolated string. Nothing is read from a PTY.

There are two statement forms:

```relux
<expr> = <pattern>      // exact-equality assertion
<expr> ? <pattern>      // regex assertion (binds captures $0..$n)
```

- `<expr>` is any pure expression: a bare identifier (`os`), a
  quoted/interpolated string (`"${HOST}:${PORT}"`), a function call
  (`which("docker")`), a dot-accessed exposed variable (`Db.port`), a
  capture (`$1`), or a bare number (`42`).
- `<pattern>` is an interpolated string to end of line — the same
  right-hand side shape as `<=` (for `=`) and `<?` (for `?`). `${var}`
  interpolation applies before the comparison.

These are the same `=` / `?` semantics as [condition
markers](02-syntax.md#condition-markers): `=` is exact equality and `?`
is an unanchored regex. A pure match reuses the shared matcher; the only
difference is that a marker gates a whole declaration before shells
spawn, while a pure-match statement asserts inline during execution.

## The `=` form: exact equality

`<expr> = <pattern>` passes only when the value of `<expr>` is
**byte-for-byte equal** to the interpolated pattern. It is not a
substring test:

```relux
let os := "linux"
os = linux                 // passes: "linux" == "linux"
os = lin                   // FAILS: not equal (substring is not enough)
os = ubuntu-linux          // FAILS: not equal
```

This differs from the shell literal operator `<=`, which scans the
output buffer and succeeds on any substring hit. `=` compares a
complete value, so it fails on a superstring or a partial overlap. For a
substring or pattern check, use `?` instead.

An empty pattern matches only an empty (or unset) value.

## The `?` form: regex and captures

`<expr> ? <pattern>` compiles `<pattern>` as a regular expression
(Rust's [`regex`](https://docs.rs/regex) crate) and passes when the
regex matches anywhere in the value — it is unanchored, exactly like
`<?`. Add `^` / `$` yourself when you need a full-value match.

A successful `?` binds the numeric capture groups `$0..$n` in the
current shell, read as `$0`, `$1`, `${2}`, and so on — the same binding
`<?` performs:

```relux
let greeting := "hello world"
greeting ? (hello) (world)
> echo "full=${0} first=${1} second=${2}"
```

After the match, `$0` is `hello world`, `$1` is `hello`, `$2` is
`world`. As with `<?`, the captures live until the next regex match
overwrites them — bind anything you need to keep with `let`. The `=`
form never binds captures.

## Assertion semantics: a no-match fails the test

A pure match is an **assertion**. Relux has no error handling, so a
no-match is a hard failure that stops the test immediately — the same
way a `<?` that never matches fails. There is no negated form.

```relux
let status := "500"
status ? ^2\d\d$           // no match -> the test fails right here
```

Because the value is already in hand, a pure match cannot time out (no
buffer, nothing to wait for) — it either matches or fails at once. A
`?` pattern that fails to compile (only possible when interpolation
produces malformed regex) is a runtime failure, not a no-match.

## Where pure matches are allowed

Pure-match statements are valid inside:

- **`shell` blocks** (in tests and effects)
- regular **`fn` bodies**

They are **not** yet allowed inside **`pure fn` bodies** — writing one
there is a compile error: *pure matching is not yet available in `pure
fn` bodies*. Returning a capture from a `pure fn` so a match can
extract a value is planned for a later milestone; it does not work
today.

A pure match is **statement-only**. It cannot appear as the right-hand
side of a `let`, an overlay value, or a cleanup value. The statement's
value is the left-hand side value, but the intended use is the assertion
(and, for `?`, the capture side effect) rather than a returned result.

## `:=` binds, `=` asserts

Mind the one-character difference:

| Statement   | Meaning                                             |
| ----------- | --------------------------------------------------- |
| `x := e`    | bind or reassign the variable `x` to the value of `e` |
| `x = e`     | **assert** that the value of `x` equals the pattern `e` |
| `let x := e`| declare `x` with the value of `e`                   |
| `let x = e` | error — binding requires `:=`, never bare `=`       |

Reassignment always uses `:=`. Bare `=` is now the exact-equality
pure-match assertion, so `x = e` no longer binds anything. `let x = e`
remains an error: use `let x := e`.

## What lands in the structured log

Every pure match emits a three-event trio into
[`events.json`](06-events-json-schema.md): `pure-match-start` (carries
`value`, `pattern`, `is_regex`) followed by either `pure-match-done`
(`matched` whole-match substring, plus `captures`) on a hit, or
`pure-match-failed` on a miss. A malformed regex emits nothing (no
orphan `pure-match-start`).

When a pure match fails the test, the outcome carries a
[`FailureRecord`](06-events-json-schema.md#failure) of type
`pure-match` with `span`, `event_seq`, `shell`, `value`, `pattern`,
`is_regex`, `call_stack`, and `vars_in_scope`. It deliberately has **no
`buffer_tail`** — a pure match has no buffer. The console and HTML
reporters render "pure match in shell `<shell>` did not match" together
with the value and the failing `= pattern` / `? pattern`.
