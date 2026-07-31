# Statements

Statements that live inside `shell` blocks, `fn` bodies, and `cleanup` blocks.

## Send

| Operator | Behavior |
|----------|----------|
| `> text` | Send `text` to the PTY followed by a newline. |
| `=> text` | Raw send: bytes only, no trailing newline. |

- Payload is to end of line. Variable interpolation applies in payload.
- Chain `=>` to assemble one command from pieces; finish with `>` to deliver.
- For control bytes (ctrl-c, ctrl-d, etc.), use BIFs from [bifs](bifs.md).

## Let bindings and reassignment

| Form | Meaning |
|---|---|
| `let name := "value"` | declare with value |
| `let name := <expression>` | declare from expression value |
| `let name` | declare empty (defaults to `""`) |
| `name := <expression>` | reassign existing variable |

- All values are strings.
- Bare numeric literals are accepted and stored as their string form -- quotes are optional for numbers. `let count := 8080` is identical to `let count := "8080"`; `match_exit_code(0)` and `INSTANCE := 1` work the same way. Use bare numerics for ports, exit codes, counts, indices -- anywhere a number is meant as a number.
- Re-binding the same name with `let` in the same scope shadows; reassignment without `let` mutates the existing binding.
- Scoped to enclosing block; inner blocks (e.g. shell inside test) can shadow outer (e.g. test-level) bindings.
- The right-hand side may be a literal, an interpolation, a function call, a capture (`$1`, `$2`), or a match expression (`<? regex` returns the matched text).
- See [interpolation](interpolation.md) for `${name}`, `${1}`, `${Alias.var}`, and `$$` escaping.

## Pure match (value assertions)

Assert that a value you already have satisfies a pattern -- no shell I/O. Distinct from `<?` / `<=`, which scan the output buffer (see [matching](matching.md)); a pure match compares a complete value.

| Form | Behavior |
|---|---|
| `<expr> = <pattern>` | Assert `<expr>` equals `<pattern>` exactly (byte-for-byte literal equality; a superstring or partial overlap fails). |
| `<expr> ? <pattern>` | Assert `<expr>` matches the regex `<pattern>` (unanchored, like `<?`). Binds numeric captures `$0`..`$n`. |

- Same `=` / `?` semantics as [markers](markers.md): `=` is exact equality, `?` is an unanchored regex.
- `<expr>` is any pure expression (identifier, quoted/interpolated string, function call, `Alias.var`, `$n`, number). `<pattern>` is an interpolated string to end of line, same shape as the `<=` / `<?` payload.
- A no-match FAILS the test immediately -- a pure match is an assertion and cannot time out (the value is already in hand). There is no negated form.
- A `?` hit binds `$0`..`$n` in the current shell exactly like `<?`; bind with `let` before the next regex match clobbers them. The `=` form binds nothing.
- Allowed in `shell` blocks and `fn` bodies. NOT allowed in `pure fn` bodies -- compile error (`pure matching is not yet available in pure fn bodies`).
- Statement-only: never a `let` right-hand side, an overlay value, or a cleanup value.
- On failure the outcome is a `pure-match` FailureRecord (`value`, `pattern`, `is_regex`; no `buffer_tail`). See [events-failures](events-failures.md).

```relux
os = linux                    // passes only if os is exactly "linux"
"${HOST}:${PORT}" ? ^db:\d+$
pair ? ^(\w+)=(.+)$           // on a hit: $1 key, $2 value
```

### `:=` binds, `=` asserts

`x := e` binds/reassigns; `x = e` asserts that `x` equals `e`. They are different statements. `let x = e` is an error -- declarations require `:=`.

There is no `==` operator: `x == y` parses as a pure match of `x` against the literal pattern `= y` (leading `= `), usually not what you want. Use `x = y` to assert equality, `x := y` to bind.

Don't (meant to bind, actually asserts and fails when unequal):

```relux
port = 8080
```

Do (bind with `:=`):

```relux
port := 8080
```

## Logging and annotation

- `log("...")` -- emit a message to the structured event log. Returns the message.
- `annotate("...")` -- emit a progress annotation; renders inline on the live progress line.

These are BIFs (parenthesised). See [bifs](bifs.md).

## Sleep

- `sleep("2s")` or `sleep("500ms")` -- pause the current shell only. Pure BIF; usable in pure contexts too.

## Comments

- `//` to end of line. There is no block-comment form.

## Pitfalls and best practices

### Prefer matches over `sleep`

`sleep` ignores readiness. A `<?` (or `<=`) on the line that proves readiness is faster, more reliable, and tells you why if it times out.

Don't:

```relux
shell server {
    > start-server
    sleep("2s")
}
shell client {
    > curl http://localhost:8080/health
}
```

Do:

```relux
shell server {
    > start-server
    <? ready on port 8080
}
shell client {
    > curl http://localhost:8080/health
}
```

### Bind captures with `let` before the next match overwrites them

`$1` holds the captures of the most recent `<?` in the current shell. The next `<?` clobbers them. Bind immediately if you need the value later.

Don't:

```relux
<? port=(\d+)
<? something-else
> curl http://localhost:${1}/health
```

Do:

```relux
<? port=(\d+)
let port := $1
<? something-else
> curl http://localhost:${port}/health
```

## See also

- [interpolation](interpolation.md) -- read if you need `${name}`, captures, or the `$$` escape
- [matching](matching.md) -- read if you need `<?` and `<=` semantics
- [markers](markers.md) -- read if you need the shared `=` / `?` matcher rules markers also use
- [bifs](bifs.md) -- read if you need `log`, `annotate`, `sleep`, or control characters
