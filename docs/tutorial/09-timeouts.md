# Timeouts

[Previous: Functions](08-functions.md)

Every match operation in Relux has a timeout — a maximum duration to wait for the expected output to appear. If the output does not arrive in time, the test fails. So far, the tutorials have relied on the default timeout from `Relux.toml` without thinking about it. That works for simple cases, but real test suites need more control: some commands respond in milliseconds, others take seconds, and some tests must enforce strict time boundaries on the system under test.

Relux provides four levels of timeout control — from broad defaults down to single-operation precision:

```relux
# Relux.toml sets the baseline:
#   [timeout]
#   match = "5s"

test "layered timeouts" {
    shell s {
        ~10s
        > slow_startup_command
        <? ready

        ~2s
        > fast_command
        <? done

        > very_slow_query --timeout 25
        <~28s? ^query complete$
    }
}
```

The config sets a 5-second default for all matches. Inside the shell, `~10s` raises the match timeout to 10 seconds for the startup command, then `~2s` drops it back for fast commands. The final match uses `<~28s?` to override the timeout for just that one operation, without changing the 2-second default for anything that follows.

## Config defaults

The `[timeout]` section in [`Relux.toml`](02-getting-started.md) controls three values:

```toml
[timeout]
match = "5s"
test = "10m"
suite = "1h"
```

**`match`** is the default timeout for every match operation — [`<=`](03-send-match-and-logs.md), [`<?`](07-regex-matching.md), and their variants. When a match operator waits for output, this is how long it waits. Defaults to `5s` if not specified.

**`test`** is the maximum duration for a single test. If a test exceeds this limit, Relux aborts it and reports a timeout failure. Optional — no limit by default.

**`suite`** is the maximum duration for the entire test run. If the suite exceeds this limit, Relux aborts the remaining tests. Optional — no limit by default.

These are **environmental tolerances** — they define how patient Relux should be when waiting. They are not assertions about the system under test. This distinction matters when we get to test-level timeouts later in this article.

## `--timeout-multiplier`

Different environments run at different speeds. A test suite that passes in 2 seconds on a developer laptop might need 6 seconds on an overloaded CI server. Rather than hardcoding generous timeouts everywhere, Relux provides a multiplier:

```bash
relux run --timeout-multiplier 3.0
relux run -m 3.0
```

The multiplier scales every **environmental tolerance** timeout by the given factor. With `-m 3.0` and a config of `match = "5s"`, every match operation defaults to 15 seconds. The config `test` and `suite` timeouts are scaled the same way.

Not every timeout is environmental tolerance — some timeouts are assertions about the system under test, and those must not be scaled. We will point out which timeouts are affected by the multiplier and which are not as we introduce each one.

## The `~` operator

The `~` operator sets the match timeout for the current shell, overriding the config default:

```relux
test "scoped timeout allows delayed output" {
    shell s {
        ~3s
        > sh -c 'sleep 1 && echo delayed'
        <? ^delayed$
    }
}
```

The `~3s` sets the timeout to 3 seconds. Every match operation after it — `<?`, `<=`, and their variants — uses 3 seconds instead of the config default. The change persists until another `~` replaces it:

```relux
test "scoped timeout overrides previous timeout" {
    shell s {
        ~200ms
        ~3s
        > sh -c 'sleep 1 && echo delayed'
        <? ^delayed$
    }
}
```

The first `~200ms` would be too short for the 1-second sleep, but the second `~3s` replaces it before the match runs.

Like the config `match` timeout, `~` is an environmental tolerance — it says "wait this long for output." It is affected by `--timeout-multiplier`.

## Inline `<~` overrides

Sometimes a single operation needs a different timeout without changing the shell's default. The `<~` prefix adds a one-shot timeout to any match operator:

```relux
test "inline timeout overrides scoped timeout for regex" {
    shell s {
        ~200ms
        > sh -c 'sleep 1 && echo delayed_regex'
        <~3s? ^delayed_regex$
    }
}
```

The shell timeout is 200ms — far too short for a command that takes a full second. But `<~3s?` overrides the timeout for just this one match. The next match after it reverts to the 200ms shell timeout:

```relux
test "inline timeout is one-shot" {
    shell s {
        ~200ms
        > sh -c 'sleep 1 && echo delayed'
        <~3s? ^delayed$
        > echo immediate
        <? ^immediate$
    }
}
```

The `<~3s?` match waits up to 3 seconds. The `<? ^immediate$` that follows uses the shell's 200ms timeout — the override did not persist.

The `<~` prefix works with both match operators:

| Operator | Meaning |
|----------|---------|
| `<~[duration]?` | [Regex match](07-regex-matching.md) with timeout override |
| `<~[duration]=` | [Literal match](03-send-match-and-logs.md) with timeout override |

The prefix only changes the timeout. Everything else about the operator stays the same: `<~3s?` returns the same value as `<?`, `<~3s=` returns the same value as `<=`. You can use [captures](07-regex-matching.md), [variable interpolation](06-variables.md), and all the other features exactly as before:

```relux
test "inline timeout with variable interpolation" {
    shell s {
        ~200ms
        let word = "interp_val"
        > sh -c 'sleep 1 && echo interp_val'
        <~3s? ^${word}$
    }
}
```

Unlike the `~` operator, inline `<~` overrides are **not** affected by `--timeout-multiplier`. An inline override is a precise, deliberate choice for a specific operation — it expresses exact test intent, not environmental tolerance.

## Test-level timeout

A test can declare its own timeout directly in the definition:

```relux
test "must complete quickly" ~5s {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The `~5s` after the test name sets a hard boundary: if the entire test — all shell blocks, all matches, all waits — takes longer than 5 seconds, Relux aborts it and reports a timeout failure. This overrides the `test` value from `Relux.toml`.

This looks similar to the shell-scoped `~`, but the semantics are fundamentally different. The config `test` timeout and the shell `~` operator are environmental tolerances — they say "be patient for this long." The test-level timeout is an **assertion about the system under test**.

Consider testing Relux's own timeout mechanism. You want to verify that a shell-level timeout of 1 second actually fires:

```relux
test "shell timeout fires within bound" ~5s {
    shell s {
        ~1s
        > sleep 999
        <? ^this will never appear$
    }
}
```

The inner `~1s` timeout should fire after 1 second when the match fails. The outer `~5s` test timeout is the assertion: if 5 seconds pass and the inner timeout somehow did not fire, the system is broken. Without the test-level timeout, a bug in the timeout mechanism would cause the test to hang forever. With it, the test fails fast and tells you exactly what went wrong.

Because it is an assertion, the test-level timeout is **not affected by `--timeout-multiplier`**. Scaling it would weaken the assertion. If you are testing that something completes within 5 seconds, doubling the multiplier should not give it 10 seconds — that would defeat the purpose of the test. The inner `~1s` gets scaled (it is environmental tolerance), but the outer `~5s` stays fixed (it is the assertion being tested).

This is the key distinction: environmental timeouts answer "how patient should we be?" and scale with the environment. Test-level timeouts answer "how fast must the system be?" and never scale.

## Timeout scoping across function calls

When you call a [function](08-functions.md), the function inherits the caller's current timeout. When the function returns, the timeout reverts to what the caller had before the call:

```relux
fn slow_operation() {
    ~10s
    > long_running_command
    <? ^done$
    match_ok()
}

test "timeout reverts after function call" {
    shell s {
        ~2s
        slow_operation()
        # Back to 2s here — the function's ~10s did not persist
        > echo quick
        <? ^quick$
    }
}
```

The caller sets `~2s`. Inside `slow_operation()`, `~10s` changes the timeout — but only within the function's scope. When the function returns, the caller's 2-second timeout is restored.

The timeout lives on the shell — it is part of the shell's own state, like the [output buffer](04-the-output-buffer.md) or the running processes. Reverting the timeout on function return is a convenience that prevents accidental side effects: a function can adjust the timeout for its own operations without forcing the caller to save and restore the previous value manually.

If a function does not set its own timeout, it uses whatever the caller had:

```relux
fn check_output() {
    > echo test
    <? ^test$
    match_ok()
}

test "function inherits caller timeout" {
    shell s {
        ~10s
        check_output()
        # check_output used the 10s timeout for its match
    }
}
```

## Precedence

When a match operation runs, Relux resolves the timeout using this precedence chain:

| Priority | Source | Example | Scaled by `-m`? |
|----------|--------|---------|-----------------|
| 1 (highest) | Inline override | `<~3s? pattern` | No |
| 2 | Shell scope | `~2s` | Yes |
| 3 (lowest) | Config default | `match = "5s"` | Yes |

The first one that applies wins. If there is no inline override, the shell scope is used. If no `~` has been set, the config default applies.

Separately, the test-level timeout (`test "name" ~5s`) and the config `test`/`suite` timeouts operate as outer boundaries — they cap the total duration of a test or run, independent of which match timeout is in effect.

## Best practices

### Use the multiplier for CI flakiness, not longer timeouts

When tests start failing on CI but pass locally, the tempting fix is to increase the timeouts in the test files. A `~2s` becomes `~5s`, then `~10s`, and soon every test has generous timeouts that mask real performance regressions.

The multiplier exists for this problem. Keep your timeouts tight — reflecting how fast the system *should* respond — and use `-m 2.0` or `-m 3.0` on slow environments. This way, timeouts still catch genuine slowdowns on the developer's machine while tolerating CI variability.

### Do not use test-level timeout as a safety net

You might set `test "name" ~5m` on every test thinking "this prevents any test from running forever." That is what the config `test` timeout is for — set it once in `Relux.toml` and it applies to every test.

The test-level `~` timeout is for tests where the duration is the assertion: "this operation must complete within X seconds, or the system under test is broken." Reserve it for those cases. If you put `~5m` on every test, you lose the ability to distinguish between "this test is slow" and "this test is verifying a time constraint."

### Match the timeout level to the intent

When choosing where to set a timeout, ask: "is this about the environment or about the system?"

- The system should respond within 2 seconds → use test-level `~2s` or inline `<~2s?`
- The CI server is slow → use `-m` or raise the config/shell timeouts
- One specific command is slower than the rest → use `~` before the command, or `<~` on the match

Mixing these up — using `<~` for environmental tolerance, or config timeouts for system assertions — leads to tests that are either fragile or meaningless.

## Try it yourself

Write a test that verifies a command completes within a time boundary:

1. Use `sleep` in a shell command to simulate a slow operation (e.g., `sh -c 'sleep 0.5 && echo done'`)
2. Set a shell-scoped timeout with `~` that is long enough for the command
3. Add a test-level timeout that acts as the outer assertion — the whole test must finish well within a reasonable bound
4. Add a second match using `<~` with a shorter inline timeout for a fast command that follows

Run the test, then try lowering the shell timeout below the sleep duration to see the timeout failure.

---

Next: [Fail Patterns](10-fail-patterns.md) — continuous monitoring for errors with `!?` and `!=`
