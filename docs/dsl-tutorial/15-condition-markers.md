# Condition Markers

[Previous: Modules and Imports](14-modules-and-imports.md)

Integration tests exercise real systems, and real systems have prerequisites. Some tests only make sense on a particular operating system. Some require a tool like `docker` or `psql` to be installed. Some are too slow to run locally on every test run, and belong exclusively to CI.

Without a way to express these assumptions, a missing prerequisite looks the same as a broken test. If a test needs `docker` and docker is not installed, the test fails — and the person reading the results cannot tell whether the system under test is broken or the machine simply was not set up for that test. The failure is ambiguous and unhelpful.

Condition markers solve this in two ways. First, they let you **categorize tests by environment** — this group runs on macOS, that group runs on Linux, these long-running tests only run in CI. Second, they let you **guard against missing preconditions** — if the required tool is not available, the test is skipped with an informative reason instead of failing with a confusing error.

Here is a test that only runs when `docker` is available:

```relux
# skip unless which("docker")
test "build container image" {
    shell s {
        > docker build -t myapp .
        <? ^Successfully built
        match_prompt()
    }
}
```

When `docker` is in PATH, the test runs normally. When it is not, Relux skips the test and reports exactly why — no shell is spawned, no confusing failure appears.

And here is a test that only runs in CI:

```relux
# run if "${CI}"
test "full regression suite" {
    shell s {
        > ./run-all-benchmarks.sh
        <? ^All benchmarks passed$
        match_prompt()
    }
}
```

Locally, where `CI` is not set, this test is silently skipped. On the build server, it runs.

## Unconditional markers

The simplest form of a marker has no condition at all. There are three kinds:

**`# skip`** unconditionally skips the test. This is useful for temporarily disabling a test without deleting or commenting it out:

```relux
# skip
test "work in progress" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The test appears in the results as skipped. When you are ready to re-enable it, remove the marker.

**`# flaky`** marks a test as known-unstable. When `[flaky].max_retries` is set in `Relux.toml`, a failing flaky test is retried from scratch with exponentially increasing tolerance timeouts. With the default `max_retries = 0`, the marker is documentary only and the test runs normally:

```relux
# flaky
test "timing sensitive" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

Configure retry behavior in `Relux.toml`:

```toml
[flaky]
max_retries = 3           # retry up to 3 times on failure
timeout_multiplier = 1.5   # tolerance timeouts scale by 1.5^retry
```

Or override from the command line:

```text
relux run --flaky-retries 3 --flaky-multiplier 2.0
```

Each retry runs the test from scratch — fresh shell, fresh effects. Tolerance timeouts (`~`) are scaled by `multiplier^(retry-1)`; assertion timeouts (`@`) are never scaled. If any retry passes, the test is reported as passed. If all retries are exhausted, it is reported as failed.

**`# run`** without a condition is a no-op — the test runs as it normally would. On its own it has no effect, but it becomes useful with a condition attached, as shown below.

## Conditional markers

A condition adds an `if` or `unless` modifier and an expression to the marker. The expression is evaluated before any shells are spawned.

### Truthiness checks

The simplest conditional form checks whether an [environment variable](06-variables.md#relux-environment-variables) is set and non-empty:

```relux
# skip if "${MY_VAR}"
test "skipped when MY_VAR is set" {
    shell s {
        > echo hello
        <? hello
    }
}
```

The truthiness rule is straightforward: an empty string or an unset variable is **false** (falsy). Any non-empty string is **true** (truthy).

The `unless` modifier inverts the check:

```relux
# skip unless "${CI}"
test "only runs in CI" {
    shell s {
        > echo hello
        <? hello
    }
}
```

This skips the test unless `CI` is set — the common pattern for CI-only tests.

The `run` kind works the other way around. Where `skip` says "do not run this test when the condition is met", `run` says "only run this test when the condition is met":

```relux
# run if "${MY_VAR}"
test "only runs when MY_VAR is set" {
    shell s {
        > echo hello
        <? hello
    }
}
```

And its inverse:

```relux
# run unless "${MY_VAR}"
test "runs when MY_VAR is not set" {
    shell s {
        > echo hello
        <? hello
    }
}
```

Note that `# run if "${X}"` and `# skip unless "${X}"` are logically equivalent — both skip the test when `X` is unset. The choice between them is about readability, which the best practices section below discusses.

### Equality comparisons

When truthiness is not enough, you can compare a variable against a specific value using `=`:

```relux
# skip if "${MY_VAR}" = "yes"
test "skipped when MY_VAR is exactly yes" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

Both sides of the `=` support [variable interpolation](06-variables.md#string-interpolation). You can build compound values:

```relux
# run if "${HOST}:${PORT}" = "localhost:8080"
test "only on local dev server" {
    shell s {
        > curl localhost:8080/health
        <? ^ok$
        match_prompt()
    }
}
```

Numbers are allowed too — they are compared as strings:

```relux
# run if "${COUNT}" = 0
test "only when count is zero" {
    shell s {
        > echo "starting fresh"
        <? ^starting fresh$
    }
}
```

### Regex matching

For more flexible matching, the `?` operator tests a value against a regex pattern:

```relux
# skip unless "${MY_VAR}" ? ^(yes|true)$
test "requires MY_VAR to be yes or true" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The regex pattern supports variable interpolation as well:

```relux
# skip unless "${ARCH}" ? ^(x86_64|aarch64)$
test "only on 64-bit architectures" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

## Pure function calls in markers

Marker expressions are not limited to variable interpolation. You can call [pure functions](12-pure-functions.md) to compute values or perform checks. This is where markers become truly powerful for asserting environment preconditions.

The [built-in function](05-built-in-functions.md) `which()` checks whether an executable exists in PATH — it returns the path if found, or an empty string (falsy) if not:

```relux
# skip unless which("docker")
test "needs docker" {
    shell s {
        > docker ps
        <? ^CONTAINER ID
        match_prompt()
    }
}
```

You can also define your own pure functions for more complex checks:

```relux
pure fn always_true() {
    "yes"
}

# skip if always_true()
test "always skipped by custom function" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

Pure functions combine naturally with regex matching. Here, `normalize` lowercases the value before the comparison:

```relux
pure fn normalize(val) {
    lower(val)
}

# skip unless normalize("${TARGET_OS}") ? ^(linux|darwin)$
test "only on Linux or macOS" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The function argument uses variable interpolation, and the regex tests the lowercased result. This handles cases where the environment variable might be `"Linux"`, `"LINUX"`, or `"linux"`.

## Multiple markers

A test or effect can carry more than one marker:

```relux
# skip unless "${CI}"
# skip if "${SKIP_ME}"
test "CI only, unless explicitly skipped" {
    shell s {
        > echo hello
        <? ^hello$
    }
}
```

The exact combination semantics for multiple markers are not yet established and are the subject of an upcoming RFC. For now, keep things simple: use a single marker per test or effect when possible, and use regex patterns to express complex conditions within one marker.

## Markers on functions

Markers work on [functions](08-functions.md) and [pure functions](12-pure-functions.md) too:

```relux
# skip unless which("jq")
fn parse_json(input) {
    > echo '${input}' | jq -r '.name'
    <? ^.+$
    let name = $0
    match_prompt()
    name
}

test "extract name from JSON" {
    shell s {
        let name = parse_json('{"name": "alice"}')
        > echo "${name}"
        <? ^alice$
        match_prompt()
    }
}
```

The key behavior: **when a function is skipped, all tests that call it are also skipped.** In the example above, if `jq` is not installed, the `parse_json` function is skipped, which propagates to every test that calls it. The test is reported as skipped — no shell is spawned, no confusing failure appears. This works the same way for both `fn` and `pure fn`.

## Markers on effects

Markers work on [effects](11-effects-and-dependencies.md) too. This is particularly useful for effects that provision heavy infrastructure:

```relux
# skip if "${SKIP_EFFECT}"
effect Guarded {
    expose service

    shell service {
        > echo "effect ran"
        <? ^effect ran$
    }
}

test "depends on conditionally skipped effect" {
    start Guarded as g
    shell g.service {
        > echo "test body ran"
        <? ^test body ran$
    }
}
```

There is one important rule: **when an effect is skipped, all tests that depend on it are also skipped.** This cascades through the dependency graph. If effect `A` is skipped and test `X` needs `A`, test `X` is skipped too — even if test `X` has no markers of its own. The reasoning is straightforward: if the effect could not set up the infrastructure the test requires, running the test would be meaningless.

## Evaluation timing and scope

Markers evaluate **before** any shells are spawned. For test-level markers, this happens before the test's effects are even set up. For effect-level markers, it happens before the effect's own shells are created.

Because of this early evaluation, marker expressions can only see **environment variables** — the base environment that Relux inherits from the system plus any variables set in `Relux.toml`. Variables declared with `let` inside tests or effects do not exist yet at marker evaluation time. This is why marker syntax uses `"${VAR}"` to reference the environment, the same [interpolation syntax](06-variables.md#string-interpolation) you already know.

## Best practices

### Markers assert, effects provision

The distinction is:

- **Markers** assert what the environment *already has* — an installed binary, a particular OS, a running CI server. These are things outside the test's control.
- **Effects** provision what the test *needs* — starting a service, creating a temp directory, seeding a database. These are things the test can set up and tear down.

If you can set it up, use an [effect](11-effects-and-dependencies.md). If you can only check for it, use a marker. A test that needs a PostgreSQL database running should have an effect that starts one. A test that needs `psql` to be installed should have a marker that checks for it.

### Choose the marker that reads like intent

`# run if "${CI}"` and `# skip unless "${CI}"` are logically identical — both skip the test when `CI` is not set. The difference is how they communicate intent to someone reading the test file.

Use `# run if ...` when the condition describes the *target environment*: "this test runs in CI." Use `# skip unless ...` when the condition describes a *requirement*: "skip this test unless docker is available." The marker should read like a sentence that explains *why* the test might not run.

### Understand effect skip propagation

Putting a marker on an effect skips every test that depends on it. This is powerful but can be surprising. If an effect is shared by many tests, a single marker on that effect gates a large part of the suite. Before adding a marker to a widely-used effect, consider whether the marker belongs on the individual tests instead.

## Try it yourself

1. Write a test that only runs on macOS. Use a pure function that calls `which("sw_vers")` (a macOS-specific binary) to detect the platform, and a `# skip unless ...` marker.

2. Write an effect `DockerReady` that guards itself with `# skip unless which("docker")`. Have it start a container in its shell block. Then write a test that `start`s `DockerReady` — verify that the test is skipped when docker is not available, without needing its own marker.

3. Write a test with two markers: one that restricts it to CI (`# run if "${CI}"`) and one that skips it when a feature flag is disabled (`# skip unless "${ENABLE_SLOW_TESTS}"`). Think about what happens in each combination of those two variables.

---

Next: [The CLI](16-the-cli.md) — complete coverage of `relux new`, `check`, `run`, and `history`
