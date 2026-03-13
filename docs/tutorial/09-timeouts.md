# Timeouts

[Previous: Functions](08-functions.md)

Every match operation in Relux has a timeout — a maximum duration to wait for the expected output to appear. If the output does not arrive in time, the test fails. So far, the tutorials have relied on the default timeout from `Relux.toml` without thinking about it. That works for simple cases, but real test suites need more control: some commands respond in milliseconds, others take seconds, and some tests must enforce strict time boundaries on the system under test.

Relux draws a sharp line between two kinds of timeout. A **tolerance** timeout (`~`) says "be patient for this long" — it absorbs environmental variability and scales with the `--timeout-multiplier` flag. An **assertion** timeout (`@`) says "the system must respond within this time" — it is a correctness check and never scales. The prefix determines the intent, not the position: both `~` and `@` work at every level — config defaults, shell scope, inline overrides, and test definitions.

```relux
test "layered timeouts" @40s {
    shell s {
        ~10s
        > slow_startup_command
        <? ready

        @2s
        > fast_command
        <? done

        > very_slow_query --timeout 25
        <~28s? ^query complete$
    }
}
```

The config sets a default match timeout. Inside the shell, `~10s` raises the tolerance timeout to 10 seconds for the startup command — if CI is slow, the multiplier can stretch this further. Then `@2s` switches to an assertion timeout: the `fast_command` must respond within 2 seconds regardless of environment. The final match uses `<~28s?` to set a one-shot tolerance override for just that operation. The test itself has `@40s` — an assertion that the entire test must complete within 40 seconds, multiplier or not.

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

All three config timeouts are **tolerances** — they are scaled by `--timeout-multiplier`.

## `--timeout-multiplier`

Different environments run at different speeds. A test suite that passes in 2 seconds on a developer laptop might need 6 seconds on an overloaded CI server. Rather than hardcoding generous timeouts everywhere, Relux provides a multiplier:

```bash
relux run --timeout-multiplier 3.0
relux run -m 3.0
```

The multiplier scales every **tolerance** timeout (`~`) by the given factor. With `-m 3.0` and a config of `match = "5s"`, every match operation defaults to 15 seconds. A shell-scoped `~2s` becomes 6 seconds. Config `test` and `suite` timeouts are scaled the same way.

**Assertion** timeouts (`@`) are never scaled. They express exact intent about the system under test — stretching them would weaken the assertion. If a test says `@2s`, the system must respond within 2 seconds whether you are running on a laptop or a loaded CI box.

## The `~` operator

The `~` operator sets a tolerance timeout for the current shell, overriding the config default:

```relux
test "scoped timeout allows delayed output" {
    shell s {
        ~3s
        > sh -c 'sleep 1 && echo delayed'
        <? ^delayed$
    }
}
```

The `~3s` sets the timeout to 3 seconds. Every match operation after it — `<?`, `<=`, and their variants — uses 3 seconds instead of the config default. The change persists until another timeout operator replaces it:

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

The first `~200ms` would be too short for the command, but the second `~3s` replaces it before the match runs.

The `~` operator accepts milliseconds (`~200ms`), seconds (`~3s`), minutes (`~2m`), and compound durations (`~1m30s`).

Because `~` is a tolerance timeout, it is scaled by `--timeout-multiplier`. With `-m 2.0`, a `~3s` becomes 6 seconds.

## The `@` operator

The `@` operator sets an assertion timeout for the current shell. It works exactly like `~` in terms of scope and persistence, but it is never scaled:

```relux
test "assertion timeout in shell scope" {
    shell s {
        @2s
        > echo hello
        <? ^hello$
    }
}
```

The `@2s` sets a 2-second assertion timeout. Every match after it must be complete within 2 seconds — no multiplier adjustment, no environmental slack. Use `@` when the time boundary is part of what you are testing: "the system must respond within X."

You can switch between `~` and `@` freely within a shell. Each one replaces the previous timeout, regardless of kind:

```relux
test "mixing tolerance and assertion" {
    shell s {
        ~3s
        > startup_command
        <? ready

        @1s
        > echo fast
        <? ^fast$

        ~5s
        > slow_command
        <? ^done$
    }
}
```

The startup match uses a 3-second tolerance. The `echo fast` match uses a 1-second assertion. The final match switches back to a 5-second tolerance.

## Inline overrides

Sometimes a single operation needs a different timeout without changing the shell's default. The `<~` and `<@` prefixes add a one-shot timeout to any match operator:

```relux
test "inline timeout overrides scoped timeout for regex" {
    shell s {
        ~200ms
        > sh -c 'sleep 1 && echo delayed_regex'
        <~3s? ^delayed_regex$
    }
}
```

The shell timeout is 200ms — far too short for a command that takes over 100 milliseconds. But `<~3s?` overrides the timeout for just this one match. The next match after it reverts to the 200ms shell timeout:

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

Both prefixes work with both match operators:

| Operator | Meaning |
|----------|---------|
| `<~[duration]?` | [Regex match](07-regex-matching.md) with tolerance override (scaled) |
| `<~[duration]=` | [Literal match](03-send-match-and-logs.md) with tolerance override (scaled) |
| `<@[duration]?` | Regex match with assertion override (not scaled) |
| `<@[duration]=` | Literal match with assertion override (not scaled) |

The prefix only changes the timeout. Everything else about the operator stays the same — you can use [captures](07-regex-matching.md), [variable interpolation](06-variables.md), and all other features exactly as before:

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

Use `<@` when a single match is an assertion about response time:

```relux
test "assertion timeout inline regex match" {
    shell s {
        ~200ms
        > sh -c 'sleep 1 && echo assert_regex'
        <@3s? ^assert_regex$
    }
}
```

The `<@3s?` asserts the system responds within 3 seconds. The multiplier will not stretch it.

## Test-level timeout

A test can declare its own timeout directly in the definition, using either prefix:

```relux
test "tolerance on test" ~30s {
    shell s {
        > echo hello
        <? ^hello$
    }
}

test "assertion on test" @3s {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The `~30s` is a tolerance — scaled by the multiplier, it says "be patient for 30 seconds." The `@3s` is an assertion — never scaled, it says, "this test must complete within 3 seconds or the system is broken."

Consider testing Relux's own timeout mechanism. You want to verify that a shell-level timeout of 1 second actually fires:

```relux
test "shell timeout fires within bound" @5s {
    shell s {
        ~1s
        > sleep 999
        <? ^this will never appear$
    }
}
```

The inner `~1s` timeout should fire after 1 second when the match fails. The outer `@5s` test timeout is the assertion: if 5 seconds pass and the inner timeout somehow did not fire, the system is broken. Without the test-level assertion timeout, a bug in the timeout mechanism would cause the test to hang forever.

If neither prefix is used on the test definition, the config `test` timeout applies (if set). A test-level timeout — whether `~` or `@` — overrides the config value.

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

The timeout lives on the shell — it is part of the shell's own state, like the [output buffer](04-the-output-buffer.md) or the running processes. Reverting the timeout on function return prevents accidental side effects: a function can adjust the timeout for its own operations without forcing the caller to save and restore the previous value manually.

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

This scoping applies equally to `~` and `@` timeouts. A function that sets `@1s` does not change the caller's timeout kind when it returns — the caller gets back exactly what it had, whether that was a tolerance or an assertion.

## Precedence

When a match operation runs, Relux resolves the timeout using this precedence chain:

| Priority | Source | Example | Scaled by `-m`? |
|----------|--------|---------|-----------------|
| 1 (highest) | Inline tolerance | `<~3s? pattern` | Yes |
| 1 (highest) | Inline assertion | `<@3s? pattern` | No |
| 2 | Shell scope tolerance | `~2s` | Yes |
| 2 | Shell scope assertion | `@2s` | No |
| 3 (lowest) | Config default | `match = "5s"` | Yes |

The first one that applies wins. If there is no inline override, the shell scope is used. If no `~` or `@` has been set, the config default applies.

Separately, the test-level timeout (`test "name" ~5s` or `test "name" @3s`) and the config `test`/`suite` timeouts operate as outer boundaries — they cap the total duration of a test or run, independent of which match timeout is in effect.

## Best practices

### Use the multiplier for CI flakiness, not longer timeouts

When tests start failing on CI but pass locally, the tempting fix is to increase the timeouts in the test files. A `~2s` becomes `~5s`, then `~10s`, and soon every test has generous timeouts that mask real performance regressions.

The multiplier exists for this problem. Keep your timeouts tight — reflecting how fast the system *should* respond — and use `-m 2.0` or `-m 3.0` on slow environments. This way, timeouts still catch genuine slowdowns on the developer's machine while tolerating CI variability.

### Choose the prefix, not the position

The `~` vs `@` prefix is what determines whether a timeout is environmental tolerance or a system assertion. Both prefixes work at every level — shell scope, inline override, and test definition. Ask yourself: "is this about the environment or about the system?"

- The CI server is slow → use `~` (tolerance), let `-m` scale it
- One specific command is slower than the rest → use `~` with a larger value, or `<~` on the match
- The system must respond within 2 seconds → use `@2s` or `<@2s?`
- The entire test must complete within a bound → use `test "name" @5s`

### Reserve `@` for real assertions

If you put `@` on everything, the multiplier becomes useless — nothing scales, and slow environments fail. Use `@` only when the time boundary is genuinely part of what you are testing. Most timeouts in a typical test suite should be `~` tolerances, with `@` reserved for the few cases where timing is the assertion.

## Try it yourself

Write a test that exercises both kinds of timeout:

1. Use `~` to set a shell-scoped tolerance timeout long enough for a `sleep 0.5 && echo done` command
2. Add an `@` assertion timeout on the test definition — the whole test must finish within a strict bound
3. Add a second match using `<@` with an inline assertion timeout for a fast command
4. Run the test, then try adding `-m 0.5` to halve the tolerance timeouts — notice which timeouts shrink and which stay fixed

---

Next: [Fail Patterns](10-fail-patterns.md) — continuous monitoring for errors with `!?` and `!=`
