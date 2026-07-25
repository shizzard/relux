# Fail Patterns

Background guards on a shell's output. If the pattern appears, the test fails immediately.

## Operators

| Operator | Behavior |
|----------|----------|
| `!? regex` | Set a regex fail pattern for the current shell. |
| `!= literal` | Set a literal substring fail pattern. |
| `!?` (no payload) | Clear the active fail pattern. |
| `!=` (no payload) | Clear the active fail pattern. |

- Variable interpolation applies in payload.
- Both forms behave identically in scope, checking, and replacement rules.

## Slot semantics

- One active fail pattern per shell. Setting a new one replaces the previous, regardless of kind.
- Checked on every new chunk of buffered output, inline with match operations and at statement boundaries.
- When set, the buffer is rescanned immediately -- if a match is already present, the test fails on the spot.
- Scope is the shell. Switching shells switches slots.

## Cleanup interaction

- Cleanup runs in a freshly spawned implicit shell -- the slot starts empty.
- A `!?` / `!=` set in the test body does not carry into cleanup. Set guards inside the cleanup block if you want them there.

## Pitfalls and best practices

### Clear the slot before reusing the shell for unrelated work

A guard from one phase keeps firing in the next. If the test legitimately emits the guard string later, clear with `!?` (no payload) first.

Don't:

```relux
!? ERROR
> start-service
<? ready
> simulate-recoverable-error
<? recovered
```

Do:

```relux
!? ERROR
> start-service
<? ready
!?
> simulate-recoverable-error
<? recovered
```

### Use `!=` when the pattern contains regex metacharacters

`!?` interprets `.`, `(`, `*` etc. as regex. For a literal string with special characters, `!=` is correct and clearer.

Don't:

```relux
!? FATAL ERROR (panic.rs:42)
```

Do:

```relux
!= FATAL ERROR (panic.rs:42)
```

### Don't set fail patterns in cleanup

Cleanup is best-effort: a `!?` / `!=` hit there only emits a warning that never changes the test outcome. The slot doesn't carry from the test body (cleanup's shell starts empty), and there's no value in re-setting it -- skip the guard entirely. See [cleanup](cleanup.md).

## See also

- [matching](matching.md) -- read if you need the match operators a fail pattern guards alongside
- [cleanup](cleanup.md) -- read if you need fresh-implicit-shell rules for cleanup
- [multimatch](multimatch.md) -- read if you need to know how fail patterns behave during a multimatch block
