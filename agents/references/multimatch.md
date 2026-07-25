# Multimatch

The `<{ ... }` block waits for several patterns to appear in any order before advancing the cursor.

## Syntax

```relux
<{
    ? <regex>
    // comment
    = <literal>
    ? <another regex>
}
```

- Inner lines: `? <regex>` or `= <literal>`, one per line. Mix freely.
- Empty form `<{ }` is a parse error.
- Comments between inner lines are permitted.
- Inline timeout override: `<~10s{ ... }` (tolerance) or `<@2s{ ... }` (assertion).

## Atomic-cursor rule

- The cursor sits at its block-entry position for the entire block.
- Every byte that arrives is offered to every still-unmatched pattern, scanning from the same anchor.
- A pattern that matches stops participating; the cursor still does not move.
- At block exit, the cursor advances once, to the maximum of the per-pattern match-end offsets.
- Overlapping matches are permitted; duplicate inner patterns are independent slots.

## Command-match-anchor still applies

A multimatch block is the *match* phase of one iteration of the [command-match-anchor loop](matching.md). The *anchor* (typically `match_ok()` or a project-local equivalent) still belongs immediately after, for the same reasons: it confirms the command finished and pins the cursor at a known boundary before the next command.

```relux
> run-pipeline
<{
    ? ^stage-1 done$
    ? ^stage-2 done$
    ? ^stage-3 done$
}
match_ok()
```

## Fail patterns during the block

- The shell's active fail pattern keeps firing during the block; a hit aborts the block exactly as it would abort a `<?` / `<=`.

## Failure reporting

- The whole block shares one timeout (inherited or the inline override).
- On timeout, the failure record lists each inner pattern as `matched:` or `timed out:`, in the same `? `/`= ` form as written.

## Pitfalls and best practices

### Captures don't bind in multimatch

Parens inside a multimatch pattern are syntactic only. `$1`, `$2`, ... are not written by the block; they keep whatever they had before. If you need a captured value, follow with a separate `<?`.

Don't:

```relux
<{
    ? ^port=(\d+)$
    ? ^host=(.+)$
}
> curl http://localhost:${1}/health
```

Do:

```relux
<{
    = batch complete
    ? ^\d+ items processed$
}
<? ^port=(\d+)$
let port = $1
> curl http://localhost:${port}/health
```

### Multimatch is for waiting, not parsing

If you need a value, match it outside the block. Multimatch's job is "wait for all of these to appear" -- nothing more.

Don't:

```relux
<{
    = service started
    ? ^port=(\d+)$
}
> use_port ${1}
```

Do:

```relux
<{
    = service started
    ? ^port=\d+$
}
<? ^port=(\d+)$
let port = $1
> use_port ${port}
```

### Use multimatch only when order is incidental

Chained `<?` / `<=` impose a strict order. Multimatch deliberately does not. Reach for it when the system genuinely produces output in non-deterministic order (parallel jobs, interleaved batches); otherwise the sequential form is clearer.

Don't:

```relux
> printf 'a\nb\nc\n'
<{
    = a
    = b
    = c
}
```

Do:

```relux
> printf 'a\nb\nc\n'
<= a
<= b
<= c
```

## See also

- [matching](matching.md) -- read if you need single-pattern `<?` and `<=`
- [fail-patterns](fail-patterns.md) -- read if you need to know how guards behave inside the block
- [timeouts](timeouts.md) -- read if you need the inline override syntax `<~Ns{ ... }`
- [interpolation](interpolation.md) -- read if you need `${var}` inside inner patterns
