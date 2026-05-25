# Matching

How `<?` and `<=` consume PTY output from the current cursor position.

## Operators

| Operator | Behavior |
|----------|----------|
| `<? regex` | Regex match against the buffer from the cursor. Sets `$0` (whole) and `$1`, `$2`, ... (groups). |
| `<= literal` | Literal substring match. Returns the matched text. |
| `<?` (no payload) | Reset the cursor to the end of the buffer (consume everything currently buffered). |
| `<=` (no payload) | Same: cursor reset. |

- Both block until a match is found or the active timeout fires.
- Both advance the cursor past the matched bytes.
- Variable interpolation applies in payload (see [interpolation](interpolation.md)).
- Inline timeout override: `<~2s? regex` (tolerance) or `<@500ms= literal` (assertion). See [timeouts](timeouts.md).

## Buffer and cursor

- The shell's stdout/stderr stream into one buffer.
- The cursor is the position from which the next match starts searching.
- Each successful match advances the cursor past its match-end.
- Bytes behind the cursor are no longer matchable in this shell, but remain visible in the [viewer](viewer.md).

## Captures

- After `<? port=(\d+)`, `$0` is `port=8080`, `$1` is `8080`.
- Captures live until the next `<?` overwrites them. Bind to a `let` immediately if you need them later.
- Multimatch (`<{ ... }`) does NOT populate captures -- see [multimatch](multimatch.md).

## Match expressions

`<? regex` and `<= literal` are also expressions. `let x = <? port=(\d+)` performs the match and binds the matched substring (`$0`) to `x`.

## The command-match-anchor loop

Every interaction with a shell follows a three-step rhythm: **command -> match -> anchor**. Treat it as the default; deviating without a clear reason leaves the cursor in an unpredictable position and the next loop starts hitting things it shouldn't.

1. **Command** -- send the input. One or more `>` / `=>` operators, possibly built from several pieces, that produce the line the program runs.
2. **Match** -- consume the program's output. One or more `<?` / `<=` / multimatch operators, each line-anchored with `^...$` so the echo never matches by accident (see [Echo trap](#echo-trap) below).
3. **Anchor** -- match a known boundary that is guaranteed to appear AFTER the output. The anchor proves the command finished and pins the cursor at a known position, so the next iteration starts clean.

The default anchor is the shell prompt, reached with [`match_ok()`](bifs.md) (verifies the previous command exited 0) or [`match_not_ok()`](bifs.md) / `match_not_ok(code)` (verifies non-zero exit). Both consume the prompt and the trailing exit-code probe, leaving the cursor immediately before the next prompt.

```relux
> ls -la
<? ^README\.md$
match_ok()
```

The rhythm scales to multi-step commands and multi-line output without change:

```relux
=> echo "BUILD_ID=$(date +%s) " && 
> echo "ready"
<? ^BUILD_ID=\d+\s+ready$
match_ok()
```

### Custom anchors for non-prompt shells

When the shell isn't a POSIX-y prompt (Python REPL, custom CLI, an interactive program with a unique end-of-output marker), `match_ok` doesn't apply. Extract a project-local anchor function shaped after `match_ok` -- one definition, every call site stays in the same rhythm:

```relux
fn match_pyrepl() {
    <? ^>>> $
}
```

Call sites then read like the canonical form: command, matches, then `match_pyrepl()`.

### When to drop the anchor

Two cases legitimately can't anchor:

- **Log tailing** -- a shell running `tail -f some.log` has no command/output boundary; chunks just arrive. Match what you expect; subsequent matches start from wherever the cursor landed.
- **No predictable boundary** -- programs that stream unstructured output indefinitely.

Outside those, dropping the anchor is a smell, and deviation from the 'command-match-anchor' rhythm must be justified clearly. The next command's output mingles with whatever was left in the buffer, and matches start finding things they shouldn't.

## Pitfalls and best practices

### Echo trap

After `> cmd`, the shell echoes `cmd` back to the PTY before the program runs. An unanchored match immediately after `>` happily picks up the echo and reports success on input you sent, not output the program produced. The fix is line-anchoring with `^...$`: the echo line starts with the command (`echo "README"`), so `^README$` cannot land on it -- only the actual output line matches. `<= README` does NOT solve this (literal substring match still hits `README` inside the echoed text).

Don't:

```relux
> echo "README"
<? README
```

Don't (literal has the same trap):

```relux
> echo "README"
<= README
```

Do:

```relux
> echo "README"
<? ^README$
```

The exception: deliberately matching the echo is fine when verifying the command itself reached the shell (rare).

### Anchor regex for exact lines

Unanchored regex matches a substring from the cursor forward. `<? ok` matches `okay`, `broken`, anything containing `ok`.

Don't:

```relux
<? ok
```

Do:

```relux
<? ^ok$
```

### Express numeric range assertions as regex

There is no `<` / `>` / `>=` comparison operator -- all values are strings. Encode ranges ("count > 0", "exit code is 2xx") as character-class regex; the match itself is the assertion, and `<?` captures at the same time.

Don't (no such operator):

```relux
> wc -l input.txt
<? ^(\d+)$
let count = $1
// count > 0 -- not expressible
```

Do (positive integer = leading non-zero digit):

```relux
> wc -l input.txt
<? ^([1-9][0-9]*)$
let count = $1
```

Do (HTTP 2xx):

```relux
> curl -o /dev/null -w "%{http_code}\n" "${url}"
<? ^2[0-9][0-9]$
```

### Don't use empty `<?` / `<=` unless you mean to discard

A bare `<?` or `<=` resets the cursor to the buffer end. It's useful to skip past noisy output you don't care about, but it silently throws away any pending match. Use deliberately, never as a "just in case".

Don't:

```relux
> noisy-cmd
<?
> probe
<? ready
```

Do:

```relux
> noisy-cmd
<? probe-banner
> probe
<? ready
```

## See also

- [multimatch](multimatch.md) -- read if you need to wait for several patterns in any order
- [fail-patterns](fail-patterns.md) -- read if you need background guards via `!?` / `!=`
- [interpolation](interpolation.md) -- read if you need `${var}` inside patterns
- [timeouts](timeouts.md) -- read if you need `~` tolerance vs `@` assertion or inline overrides
