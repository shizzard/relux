# Appendix A1: Best Practices

[Previous: The CLI](16-the-cli.md)

This appendix collects every best-practices guideline from the tutorial series into a single reference, grouped by topic. Each item links back to the article where it was originally introduced.

## Project setup

*From [Getting Started](02-getting-started.md):*

### Keep `/bin/sh` as the default shell

You might be tempted to configure your favorite shell — `zsh`, `fish`, `bash` — as the default in `Relux.toml`. After all, you use it every day and know its features well.

Resist the temptation. A custom shell means every developer on the team and every CI machine needs that shell installed and configured. `/bin/sh` is available everywhere, and the operations you need in integration tests — running commands, checking output, setting environment variables — work the same across POSIX shells. The interactive niceties of fancier shells (tab completion, syntax highlighting, advanced globbing) don't matter when Relux is driving the terminal.

Only switch away from `/bin/sh` if your system under test genuinely requires a specific shell to function.

### Leave timeouts at their defaults

The default match timeout of 5 seconds is generous for most commands. You might think "I'll set timeout to 500 ms to speed up failure detection". Don't — not yet.

Timeout tuning is one of those things that should be driven by actual pain, not preemptive optimization. Tight timeouts cause flaky tests on slower machines or under CI load. The defaults are deliberately conservative. When you encounter a specific situation where the default is genuinely wrong — a command that reliably takes 30 seconds — that's the time to tune. Relux provides fine-grained timeout control at the operator, shell, and test level, as covered in [Timeouts](09-timeouts.md).

### The shell prompt must be static

The prompt configured in `Relux.toml` (default: `relux> `) must be a fixed, unchanging string. Do not include dynamic elements like timestamps, git branch names, hostnames, or user-specific paths.

Relux uses the prompt as a reliable marker in the shell output stream. A prompt that changes between commands — or between machines — makes that marker unpredictable, which leads to flaky or outright broken tests. The default `relux> ` is a good choice: short, distinctive, and the same everywhere.

## The output buffer

*From [The Output Buffer](04-the-output-buffer.md):*

### Always match the prompt

If you are done examining a command's output, match the prompt. Every time. This is the single most effective habit for avoiding flaky tests.

Without a prompt match, the cursor floats somewhere in the middle of the output. When the buffer contains output from a previous command that was not fully consumed, any pattern that appears in that leftover output will match there first. The cursor advances to an unexpected position, and subsequent matches silently verify stale data.

Matching the prompt anchors the cursor at a command boundary. It is cheap, it is predictable, and it eliminates an entire class of timing-related failures.

### Check the exit code

A command can produce the expected output and still fail. Or it can fail silently, producing no output at all, while the match picks up something else entirely. Checking the exit code after a command catches these problems early:

```relux
test "verify success" {
    shell s {
        > mkdir -p /tmp/test-dir
        <= relux>
        > echo ==$?==
        <= ==0==
        <= relux>
    }
}
```

The `echo ==$?==` / `<= ==0==` pair is a cheap way to verify the previous command succeeded. The `==` delimiters are distinctive enough to avoid accidental substring matches. Without this check, a failing `mkdir` would go unnoticed — the test would continue with a missing directory and fail later with a confusing, unrelated error.

### Buffer reset does not respect causality

The buffer reset operator (`<=` with no pattern) consumes everything currently in the buffer. That "currently" depends on timing — how much output the shell has printed by the instant Relux executes the reset.

If output is still arriving — a command is running, a log line is being flushed — the cursor might land in the middle of a line, or before a line that is about to appear. This creates a race condition: the test might pass on your machine and fail in CI, or pass nine times and fail on the tenth.

In almost every case, there is a better anchor than a buffer reset. Match the prompt. Match a specific log line. Match any known text that marks the boundary you actually care about. These anchors are causal — they mean "this specific thing happened" — rather than temporal — "this is where the buffer happened to be at this moment."

Only use buffer reset when you are certain all relevant output has already arrived and there is no meaningful boundary to match against.

## Regex matching

*From [Regex Matching](07-regex-matching.md):*

### Use regex only when you need it

You might default to `<?` everywhere since it is strictly more powerful than `<=` — any literal match can be written as a regex. But regex matches are harder to read, easier to get wrong, and can match more than you intended.

Literal match `<=` is a simple substring search. It does exactly one thing and it is obvious what it matches. When you do not need capture groups, anchors, or wildcards, `<=` is the better choice. Reserve `<?` for when you genuinely need regex capabilities: extracting values, matching variable output, or anchoring to line boundaries.

### Always save captures to named variables

Capture groups like `$1` are convenient — you match a pattern, and the extracted value is right there. It is tempting to use `$1` directly in several places without saving it to a named variable first.

The problem is not with the code as you write it today. The problem is with the code as someone changes it five years from now. Test code is still code — it evolves, gets refactored, gets extended. Capture groups are silently replaced on every `<?` match. If someone inserts a new regex match between your capture and its use — a perfectly reasonable edit — `$1` now refers to something completely different. No error, no warning, just a test that fails in a confusing way that takes hours to debug.

Save the capture to a named variable immediately after the match, before doing anything else. Then use the named variable everywhere:

```relux
// Fragile — $1 can be silently replaced by a later edit:
<? ^port=(\d+)$
> curl http://localhost:${1}/health

// Durable — the port is safe no matter what happens next:
<? ^port=(\d+)$
let port = $1
> curl http://localhost:${port}/health
```

The named variable survives any number of subsequent matches. It makes the code self-documenting (the name `port` says more than `$1`), and it insulates the test from future edits.

### Anchor your patterns

A regex without anchors will match anywhere in the remaining buffer — the echoed command, a fragment of the prompt, leftover output from a previous step. This is the same problem as with literal match, but worse, because regex metacharacters like `.` and `*` match more broadly.

Use `^` and `$` to pin your match to a specific line:

```relux
// Might match the echoed command or something unexpected:
<? version=\d+

// Matches exactly one complete line:
<? ^version=\d+$
```

This does not mean you should anchor every pattern — sometimes a substring regex is what you need. But when you have a choice, anchoring is safer: it documents your intent and prevents accidental matches.

### Be careful with interpolated regex patterns

Variable interpolation in `<?` patterns lets you define reusable regex fragments — declare a pattern once at the test level and use it in multiple matches. This is handy for repeated patterns like timestamps, UUIDs, or version strings.

The catch is that after interpolation, the variable's value becomes part of the regex. If the value contains regex metacharacters — `.`, `*`, `+`, `(`, `[`, and so on — they are interpreted as regex syntax, not as literal text. A variable holding `192.168.1.1` does not match the literal IP address; the `.` matches any character, so it also matches `192X168Y1Z1`.

When the variable comes from your own `let` and you know the value, this is fine — just be aware of what you are putting into the pattern. When the variable comes from captured output or an environment variable, the content is unpredictable and the regex may compile into something you did not intend, or fail to compile entirely.

## Functions

*From [Functions](08-functions.md):*

### Captures do not survive function calls

You might call a function that internally uses `<?` and expect the capture groups (`$1`, `$2`, ...) to be available in the caller afterward. This seems reasonable — the function ran a regex match, and captures are normally available after `<?`.

But captures are part of the variable scope. When a function returns, its entire scope — including captures — is discarded. The caller's captures are restored to whatever they were before the call:

```relux
fn extract_port() {
    > echo "port=8080"
    <? ^port=(\d+)$
    // The last expression is match_ok(), whose return value is the
    // prompt string — not the captured port number.
    match_ok()
}

test "captures do not survive function calls" {
    shell s {
        // Wrong — $1 holds the caller's capture state, not the function's:
        extract_port()
        > echo "port=${1}"
        <? ^port=8080          // $1 is empty

        // Also wrong — the return value is the prompt string, because
        // match_ok() is the last expression in extract_port():
        let result = extract_port()
        > echo "result=${result}"
        <? ^result=8080        // result is the prompt, not "8080"
    }
}
```

The fix is to design the function to explicitly return what you need. Save the capture to a local variable before calling `match_ok()`, then return that variable as the last expression:

```relux
fn extract_port() {
    > echo "port=8080"
    <? ^port=(\d+)$
    let port = $1
    match_ok()
    port
}
```

Now `let port = extract_port()` in the caller gives you `"8080"`.

This is consistent with the scoping model: functions cannot modify the caller's variable state. Return values are the explicit, reliable channel for passing data back.

### Leave the shell clean

When a function interacts with the shell — sending commands and matching output — it should leave the shell in a known state before returning. That means: consume the prompt and verify the exit code with `match_ok()` (or the appropriate `match_not_ok` variant) after the last command.

```relux
// Leaves the shell in an unknown state — the caller must
// know what output is left in the buffer:
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
}

// Leaves the shell clean — prompt consumed, exit code verified:
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
    match_ok()
}
```

A function that leaves unconsumed output or an unchecked exit code forces every caller to clean up after it. That coupling is invisible and fragile — it works until someone adds a new caller that forgets, or the function's output changes slightly. Close every shell interaction with a clean handoff.

### Do not rely on shared shell state

The caller and the function share a shell session. This means the function can read shell-side environment variables set by the caller, and the caller can read shell-side state left behind by the function. Both directions are tempting shortcuts — and both lead to brittle tests.

A function cannot predict the shell state of all its callers. Some callers have not been written yet. If a function depends on a shell-side variable that the caller must set beforehand, the requirement is invisible — nothing in the function signature or call site reveals it. Pass the value as a parameter instead.

In the other direction, a caller that depends on shell-side state set by a function is coupled to the function's implementation details. If the function's internals change — a different variable name, a different order of commands — the caller silently breaks.

If you genuinely cannot avoid relying on shared shell state, make it explicit with a comment at both the definition and call site explaining the dependency. But first, consider whether a parameter or return value would work instead.

### Keep functions small

A function runs in the caller's shell, so a long function body means a long sequence of sends and matches executing in someone else's shell session. When something fails halfway through a 30-line function, the error points to a line inside the function — but understanding *why* it failed requires knowing what the caller's shell looked like at the time of the call.

Prefer small functions that do one thing: check a status code, verify a service is running, send a login sequence. If you find a function growing beyond a handful of operations, consider splitting it into smaller pieces — so each has a clear, narrow purpose.

## Pure functions

*From [Pure Functions](12-pure-functions.md):*

### Prefer `pure fn` when a function has no shell operators

You might write a helper as a regular function out of habit, because you first use it inside a shell block:

```relux
fn format_url(host, port) {
    "${host}:${port}/api"
}
```

This works fine in shell context. But later, when you want to use the same helper in a test-scope `let` or an overlay value, you discover it does not work — regular functions require a shell. You then have to go back and add the `pure` keyword.

Save yourself the trip: if a function body contains no shell operators, define it as `pure fn` from the start. It works in all the same places a regular function works, plus everywhere else.

### Extract complex interpolation into a pure function

When string interpolation gets deeply nested, the intent can become hard to read:

```relux
test "nested interpolation" {
    let host = "localhost"
    let port = "5432"
    let db = "myapp"
    shell s {
        > psql "postgres://${host}:${port}/${db}?sslmode=disable"
        <? ^connected$
        match_prompt()
    }
}
```

This is manageable, but as the string grows — multiple parameters, conditional segments, repeated patterns — readability suffers. A pure function gives the construction a name and keeps the test body focused on intent:

```relux
pure fn pg_url(host, port, db) {
    "postgres://${host}:${port}/${db}?sslmode=disable"
}

test "extracted into pure function" {
    let host = "localhost"
    let port = "5432"
    shell s {
        let url = pg_url(host, port, db)
        > psql "${url}"
        <? ^connected$
        match_prompt()
    }
}
```

## Timeouts

*From [Timeouts](09-timeouts.md):*

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

## Fail patterns

*From [Fail Patterns](10-fail-patterns.md):*

### Set fail patterns early

Place your `!?` or `!=` as the first statement in a shell block, before any commands. This maximizes coverage — the pattern is active from the very first command output. A fail pattern set after several commands has no protection over the output those commands already produced (the immediate rescan will catch it if it's in the buffer, but that turns a background monitor into a retroactive check, which is harder to reason about).

### Use fail patterns for long-running services

Fail patterns are at their most valuable when testing long-running services that produce logs you don't exhaustively match on. A web server, a database, a background worker — these emit output continuously, and you only match the specific lines that tell you the service is ready or responding correctly. A fail pattern like `!? FATAL|panic|Segfault` acts as a safety net across all that unmatched output. You focus your `<=` and `<?` operators on expected behavior; the fail pattern catches unexpected crashes in the background.

### Don't use fail patterns as assertions

Fail patterns are background monitors, not replacements for match operators. If you *expect* specific output, use `<=` or `<?` to match it. If you want to ensure something *doesn't* appear, that's what fail patterns are for. The distinction matters: match operators advance the output buffer cursor and participate in the test's flow; fail patterns operate silently in the background and only surface when something goes wrong.

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

## Effects and dependencies

*From [Effects and Dependencies](11-effects-and-dependencies.md):*

### Set fail patterns early in effects

Effects that start long-running services should set a fail pattern before the startup command, just like in a regular shell block. This maximizes coverage — any crash output during startup or during the test body triggers an immediate failure:

```relux
effect Service {
    expose service

    shell service {
        !? FATAL|ERROR|panic
        > start-my-service --foreground
        <? listening on port 8080
    }
}
```

The fail pattern is active from the first line. If the service crashes during startup, the fail pattern catches it before the readiness match even runs.

### Deduplication and shared state

Because deduplication means two aliases can point to the same shell, mutations through one alias are visible through the other. This is by design — it is how effects like the database chain work, where each layer builds on the state left by the previous one. But it means you should be aware: if two unrelated parts of a test both alias the same effect instance, they share a single PTY session. Commands sent through one alias affect the shell the other alias sees.

If you need truly independent instances, give them different overlay values — even a dummy key is enough to create separate identities:

```relux
start MyEffect as a { INSTANCE = "1" }
start MyEffect as b { INSTANCE = "2" }
```

## Cleanup

*From [Cleanup](13-cleanup.md):*

### Do not use cleanup to stop services

It is natural to think of cleanup as the place to stop a database or kill a service you started during setup. But Relux already handles this: when a test ends, it terminates all effect and test shells, which kills any processes running in them. Services started in a shell block die automatically with the shell — they are children of the PTY, so when Relux terminates the shell, the process goes with it. Even if Relux itself is killed, the OS cleans up the PTY and its children.

Using cleanup to stop services is actually worse than relying on shell termination. Cleanup runs in a **separate** shell — it has no connection to the process running in the effect's shell. If Relux crashes or is killed, cleanup never runs, and any service you expected cleanup to stop is left orphaned.

For the same reason, avoid starting daemonized or background services (processes that detach from the shell) during setup. A daemonized process is no longer a child of the PTY — it survives shell termination. If Relux is killed or terminated abnormally, neither shell termination nor cleanup can reach it, and it stays running indefinitely. Always run services in the foreground so they remain tied to the shell's lifecycle.

Reserve cleanup for things that shell termination does not handle: removing files, cleaning up directories, collecting logs, or any other filesystem side effects that outlive the shell.

### Keep cleanup self-contained

Cleanup can see top-level `let` variables, overlay variables (for effects), and environment variables — but it cannot see variables declared inside shell blocks or call functions. Shell-level `let` bindings and regex captures from the test body are not available.

Plan your cleanup around top-level variables. If a path or identifier is needed in both setup and cleanup, declare it with `let` at the effect or test level rather than inside a shell block.

### Make cleanup idempotent

Cleanup runs regardless of whether setup completed successfully. If an effect's shell block fails halfway through — the database started but the migration crashed — cleanup still runs. This means cleanup commands may encounter a partially initialized state: a file that was never created, a process that was never started, a directory that is already empty.

Write cleanup commands defensively. Assume nothing about what actually happened during setup — cleanup should be safe to run in any state, including when setup did nothing at all.

## Condition markers

*From [Condition Markers](15-condition-markers.md):*

### Markers assert, effects provision

The distinction is:

- **Markers** assert what the environment *already has* — an installed binary, a particular OS, a running CI server. These are things outside the test's control.
- **Effects** provision what the test *needs* — starting a service, creating a temp directory, seeding a database. These are things the test can set up and tear down.

If you can set it up, use an effect. If you can only check for it, use a marker. A test that needs a PostgreSQL database running should have an effect that starts one. A test that needs `psql` to be installed should have a marker that checks for it.

### Choose the marker that reads like intent

`# run if "${CI}"` and `# skip unless "${CI}"` are logically identical — both skip the test when `CI` is not set. The difference is how they communicate intent to someone reading the test file.

Use `# run if ...` when the condition describes the *target environment*: "this test runs in CI." Use `# skip unless ...` when the condition describes a *requirement*: "skip this test unless docker is available." The marker should read like a sentence that explains *why* the test might not run.

### Understand effect skip propagation

Putting a marker on an effect skips every test that depends on it. This is powerful but can be surprising. If an effect is shared by many tests, a single marker on that effect gates a large part of the suite. Before adding a marker to a widely-used effect, consider whether the marker belongs on the individual tests instead.

## The CLI

*From [The CLI](16-the-cli.md):*

### Use `--rerun` after fixing a failure

When a run has failures and you think you have fixed the issue, use `relux run --rerun` instead of re-running the full suite. This targets only the tests that failed last time, giving you faster feedback. Once the reruns pass, do a full `relux run` to confirm nothing else broke.

### Match strategy to context

Use `--strategy fail-fast` during local development — you want to know about the first failure quickly so you can fix it. Use `--strategy all` in CI — you want a complete picture of the suite's health, not just the first problem.

### Start flakiness investigation with `history`

When a test starts failing intermittently, run `relux history --flaky` before digging into the test code. The flakiness rate tells you whether you are dealing with an environment issue (sporadic) or a logic bug (consistent). If the test passes 95% of the time, you are probably looking at a timing issue. If it passes 50% of the time, there may be a race condition or uncontrolled dependency.
