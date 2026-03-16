# Fail Patterns

[Previous: Timeouts](09-timeouts.md)

So far, every check in a test has been explicit: you send a command, then match the output you expect. But what about output you *don't* expect? An `ERROR` buried in a log stream, a `Segfault` from a crashing service, a `PANIC` from an unhandled exception — these can appear at any point, and you can't predict exactly when. Writing a match for every line of output just to catch them would be impractical.

Fail patterns solve this. They set up a background monitor on a shell's output: if the pattern ever appears, the test fails immediately — no matter where you are in the test. Think of them as a tripwire stretched across the output stream.

```relux
test "service stays healthy" {
    shell server {
        !? FATAL|ERROR|panic
        > start-my-service --foreground
        <? listening on port 8080
    }

    shell client {
        > curl http://localhost:8080/health
        <? 200 OK
        match_prompt()
    }
}
```

The `!?` on line 3 sets a regex fail pattern on the `server` shell. From that point forward, every piece of output from that shell is checked against `FATAL|ERROR|panic`. If any of those strings appear — in the service's startup logs, in background output while the `client` shell runs its health check, anywhere — the test fails on the spot. The match operators check for output you *expect*; the fail pattern watches for output you *don't*.

## Regex fail patterns with `!?`

The `!?` operator sets a regex fail pattern:

```relux
shell s {
    !? [Ee][Rr][Rr][Oo][Rr]
    > echo "all good"
    <? ^all good$
    match_prompt()
}
```

The pattern `[Ee][Rr][Rr][Oo][Rr]` is a regular expression — the same [regex syntax](07-regex-matching.md) you use with `<?`. This one matches "error" in any mix of upper and lower case. As long as the shell's output doesn't contain a match, the test proceeds normally. The moment it does, the test fails.

Relux checks the fail pattern every time a new piece of shell output arrives in the [output buffer](04-the-output-buffer.md). As the shell prints data — command output, log lines, error messages — each chunk is checked against the active fail pattern before anything else happens.

When a fail pattern matches, Relux reports exactly what triggered it — the pattern, the matched text, and the shell name — so you can diagnose the problem quickly.

## Literal fail patterns with `!=`

If your error string doesn't need regex, use `!=` for a literal (substring) match:

```relux
shell s {
    != FATAL ERROR
    > echo "all good"
    <? ^all good$
    match_prompt()
}
```

This watches for the exact substring `FATAL ERROR` in the output. No regex interpretation — dots, brackets, and other special characters are matched literally. Use `!=` when the string you're watching for contains regex metacharacters and you don't want to escape them, or when you simply don't need pattern matching.

Both `!?` and `!=` behave identically in every other way: same checking points, same single-slot rule, same scoping.

## One pattern at a time

Each shell has a single fail pattern slot. Setting a new fail pattern — whether regex or literal — replaces whatever was there before:

```relux
shell s {
    !? first_pattern
    !? second_pattern
    > echo "first_pattern is fine now"
    <? ^first_pattern is fine now$
    match_prompt()
}
```

After line 3, only `second_pattern` is active. The first pattern is gone. This test passes because `first_pattern` in the output no longer triggers a failure.

The replacement works across types too. A `!=` replaces a `!?`, and vice versa:

```relux
shell s {
    !? first_pattern
    != second_pattern
    > echo "first_pattern is fine now"
    <? ^first_pattern is fine now$
    match_prompt()
}
```

Fail patterns do not stack. There is always at most one active fail pattern per shell.

## Immediate buffer rescan

When you set a fail pattern, Relux doesn't just watch for *future* output — it immediately rescans the existing [output buffer](04-the-output-buffer.md) for the new pattern. If the buffer already contains a match, the test fails right then.

This matters for ordering. Consider:

```relux
shell s {
    > echo "ERROR: something went wrong"
    <= something went wrong
    match_prompt()
    !? ERROR
}
```

The `<=` on line 3 scans forward from the cursor and finds `something went wrong` in the echoed command — consuming everything up to and including that first occurrence. But the actual command output `ERROR: something went wrong` is still in the buffer, unconsumed. When `!?` is set on line 5, Relux rescans the buffer and finds `ERROR` in that remaining output. The test fails.

The takeaway: set your fail pattern *before* generating output that might match it. The natural place is at the top of a shell block.

## Variable interpolation

Fail pattern payloads support [variable interpolation](06-variables.md), just like other operators:

```relux
shell s {
    let bad = "PANIC"
    !? ${bad}
    > echo "no panic here"
    <? ^no panic here$
    match_prompt()
}
```

The pattern is interpolated at the moment the `!?` statement executes. After interpolation, the resulting string is compiled as a regex (for `!?`) or used as a literal substring (for `!=`).

Watch out with `!?`: if the interpolated variable contains [regex metacharacters](07-regex-matching.md) like `.`, `*`, `(`, or `[`, they become part of the compiled pattern. A variable holding `error (fatal)` would be compiled as a regex where the parentheses create a capture group, not a literal match for `(fatal)`. If the value might contain special characters, use `!=` instead.

## Clearing fail patterns

A bare `!?` or `!=` with no payload clears the active fail pattern:

```relux
shell s {
    !? BOOM
    > echo safe
    <? ^safe$
    match_prompt()
    !?
    > echo BOOM
    <? ^BOOM$
    match_prompt()
}
```

Line 2 sets the fail pattern. Lines 3–5 work normally under its protection. Line 6 clears it — from this point on, there is no active fail pattern. Lines 7–9 can safely produce `BOOM` without triggering a failure.

Either `!?` or `!=` can clear the pattern, regardless of which type was used to set it. They both clear the same single slot.

## Scoping across function calls

Fail patterns follow the same scoping rule as [timeouts](09-timeouts.md): a [function](08-functions.md) inherits the caller's fail pattern, but any changes the function makes are reverted when it returns.

```relux
fn set_fail_pattern_inside() {
    !? BOOM
    > echo "in fn"
    <? ^in fn$
    match_prompt()
}

test "fail pattern set inside function does not persist in caller" {
    shell s {
        set_fail_pattern_inside()
        > echo "BOOM is safe now"
        <? ^BOOM is safe now$
        match_prompt()
    }
}
```

Inside `set_fail_pattern_inside`, the fail pattern `BOOM` is active — if the function's own `echo` had produced `BOOM`, the test would fail. But after the function returns on line 10, the caller's original state is restored (no active fail pattern in this case). The `echo "BOOM is safe now"` on line 11 is safe.

This means functions can set up their own fail patterns for internal safety without polluting the caller's monitoring. It also means a caller's fail pattern protects the function's execution — the function inherits it automatically.

## Best practices

### Set fail patterns early

Place your `!?` or `!=` as the first statement in a shell block, before any commands. This maximizes coverage — the pattern is active from the very first command output. A fail pattern set after several commands has no protection over the output those commands already produced (the immediate rescan will catch it if it's in the buffer, but that turns a background monitor into a retroactive check, which is harder to reason about).

### Use fail patterns for long-running services

Fail patterns are at their most valuable when testing long-running services that produce logs you don't exhaustively match on. A web server, a database, a background worker — these emit output continuously, and you only match the specific lines that tell you the service is ready or responding correctly. A fail pattern like `!? FATAL|panic|Segfault` acts as a safety net across all that unmatched output. You focus your `<=` and `<?` operators on expected behavior; the fail pattern catches unexpected crashes in the background.

### Don't use fail patterns as assertions

Fail patterns are background monitors, not replacements for match operators. If you *expect* specific output, use [`<=`](03-send-match-and-logs.md) or [`<?`](07-regex-matching.md) to match it. If you want to ensure something *doesn't* appear, that's what fail patterns are for. The distinction matters: match operators advance the [output buffer cursor](04-the-output-buffer.md) and participate in the test's flow; fail patterns operate silently in the background and only surface when something goes wrong.

### Combine multiple error strings with regex alternation

Since each shell has only one fail pattern slot, setting a second `!?` replaces the first. If you need to watch for multiple error patterns, combine them into a single regex using alternation:

```relux
shell s {
    !? ERROR|PANIC|FATAL|Segfault
    > start-my-service
    <? ready
    match_prompt()
}
```

Do not write:

```relux
shell s {
    !? ERROR
    !? PANIC
    !? FATAL
    > start-my-service
    <? ready
    match_prompt()
}
```

Only `FATAL` is active after line 4 — the first two patterns are gone.

## Try it yourself

Write a test that starts a simulated service and monitors it for errors:

1. Create a shell block and set a fail pattern that watches for `ERROR`, `FATAL`, and `PANIC` using a single regex alternation
2. Use `echo` to simulate several lines of normal service output (startup messages, connection logs) and match key lines with `<=` or `<?`
3. Clear the fail pattern, then echo a line containing `ERROR` — verify the test still passes because the pattern was cleared
4. As a bonus: extract the fail pattern setup into a function. Verify that the pattern is active inside the function but does not persist after the function returns

---

Next: [Pure Functions](11-pure-functions.md) — functions that compute values without touching a shell
