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
- [bifs](bifs.md) -- read if you need `log`, `annotate`, `sleep`, or control characters
