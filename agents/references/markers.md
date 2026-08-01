# Condition Markers

`# skip`, `# run`, `# flaky` -- attached to `test`, `effect`, `fn`, or `pure fn` declarations to gate or annotate them.

## Forms

| Form | Meaning |
|---|---|
| `# skip` | unconditional skip |
| `# run` | no-op |
| `# flaky` | always mark flaky |
| `# skip if <expr>` | skip when expr is truthy |
| `# skip unless <expr>` | skip when expr is falsy |
| `# run if <expr>` | skip when expr is falsy (inverse skip) |
| `# run unless <expr>` | skip when expr is truthy (inverse skip) |
| `# flaky if <expr>` | mark flaky when expr is truthy |
| `# flaky unless <expr>` | mark flaky when expr is falsy |

- Placement: one or more lines immediately before the `test`/`effect`/`fn`/`pure fn` declaration.
- Multiple markers stack with AND semantics: all must pass or the target is skipped.
- Comments and blank lines between markers and the declaration are allowed.

## Expression shapes

Conditions accept bare identifiers and quoted strings (with optional interpolation). For a single-variable truthiness or comparison check, prefer the bare form. Use the quoted/interpolated form when you need to compose multiple values into one string.

```relux
// truthiness: bare identifier
# skip if VAR

// equality: bare LHS, string RHS
# run if OS = "linux"

// equality: bare number (compared as string)
# run if COUNT = 0

// equality: compound LHS must be quoted/interpolated
# skip if "${A}:${B}" = "x:y"

// substring / pattern check: use ? (regex)
# skip if "${PATH}" ? bin

// regex: interpolated LHS
# skip unless "${ARCH}" ? ^(x86_64|aarch64)$
// regex: bare LHS
# skip unless ARCH ? ^(x86_64|aarch64)$

// pure function call (bare arg)
# skip unless which("docker")
// pure function call (matched with regex)
# skip unless which("docker") ? ^/var/lib/(.*)$
```

- Truthiness: empty string or unset variable is false; any non-empty string is true.
- `=` returns the LHS value if it equals RHS exactly, empty string otherwise -- unlike the shell literal-match operators `<=`/`!=`, which scan a streaming buffer for a substring; a marker compares against a complete value.
- For a substring or pattern check, use `?` (regex) instead: `expr ? value`.
- `?` returns the regex match if it matched, empty string otherwise.
- A no-match (including one reached through a called `pure fn`) is a falsy condition, never an error -- a condition is evaluated to decide skip/run/flaky, not to assert. A **malformed** interpolated pattern is different: it is a hard error in every pure context, marker conditions included, so a broken regex fails the run rather than silently reading as an unmet condition.
- The bare form (`VAR`) is the variable's value -- no extra interpolation step.

## Evaluation timing

- Markers evaluate BEFORE any shells are spawned.
- Only environment variables are visible at evaluation time -- no `let` bindings exist yet. That environment is the test's fully resolved, layered env, including any `.env` values on the test's path (see [environment](environment.md)).
- Pure functions and pure BIFs (e.g. `which`, `default`) are callable in conditions.

## Propagation

- Marker on a `fn` / `pure fn` -- every test that calls it (transitively) is skipped/flaky.
- Marker on an `effect` -- every test that `start`s it (transitively) is skipped or marked flaky.
- Marker on a `test` -- only that test.
- `# flaky` propagates the same way `# skip` does: a `# flaky` on a `fn` or `effect` marks every reaching test flaky (retried only when `[flaky].max_retries` > 0).

## Pitfalls and best practices

### Prefer bare variable refs in truthiness checks

A bare identifier reads the env var directly. Wrapping in `"${VAR}"` re-introduces an interpolation layer and makes the intent less obvious for a truthiness check.

Don't:

```relux
# skip unless "${CI}"
test "ci only" { ... }
```

Do:

```relux
# skip unless CI
test "ci only" { ... }
```

### Pick the marker that reads like intent

`# run if X` and `# skip unless X` are logically identical -- both skip when `X` is falsy. Pick whichever reads like a sentence that explains why this test might not run.

- Use `# run if ...` when the condition describes the target environment ("runs in CI").
- Use `# skip unless ...` when the condition describes a missing precondition ("skip unless docker is available").

Don't:

```relux
# skip unless CI
test "full regression suite" { ... }
```

Do:

```relux
# run if CI
test "full regression suite" { ... }
```

Don't:

```relux
# run if which("docker")
test "build container image" { ... }
```

Do:

```relux
# skip unless which("docker")
test "build container image" { ... }
```

### Group many tests under one marker via a guarded `pure fn`

Skip propagation through functions ("if a `fn`/`pure fn` is skipped, every test that calls it is skipped") makes a single marker-guarded `pure fn` the right tool when many tests share the same gate. Define the marker once on the function; opt each test into the group with a `let` call. One place to edit when the condition changes, one grep target when auditing which tests belong to which group, and no copy-pasted marker lines drifting out of sync.

Don't (every test repeats the same marker):

```relux
# run if SMOKE
test "smoke: health" {
    shell s { ... }
}

# run if SMOKE
test "smoke: login" {
    shell s { ... }
}
```

Do (one guarded marker function, tests opt in with a `let`):

```relux
# run if SMOKE
pure fn mark_smoke() {
    "smoke"
}

# skip if SMOKE
pure fn mark_non_smoke() {
    "non-smoke"
}

test "smoke: health" {
    let _group := mark_smoke()
    shell s { ... }
}

test "regression: full" {
    let _group := mark_non_smoke()
    shell s { ... }
}
```

Skip propagation runs at resolve time: the resolver walks every `fn` / `pure fn` the test references and inherits their markers. The `let _group := mark_smoke()` counts as a reference, so a skip on `mark_smoke` skips the test before any runtime work fires. The returned string is discarded -- the call is purely a propagation hook. Use complementary guards (`# run if X` / `# skip if X`) to partition the suite cleanly.

### Skip propagates transitively; check upstream before adding more markers

If a test is already skipped because its effect carries `# skip unless ...`, adding another `# skip` to the test hides the real source. Triage by walking the dependency chain.

Don't:

```relux
# skip
test "uses Db" {
    start Db
}
```

Do:

```relux
# skip unless POSTGRES_AVAILABLE
effect Db { ... }
```

### Guard functions that depend on external commands

A `fn` that shells out to a command implicitly assumes that command is on the caller's PATH. For commands beyond the standard POSIX set -- `docker`, `kubectl`, `psql`, `jq`, `redis-cli`, custom tooling -- the function's runnability becomes host-dependent. Without a guard, a missing command surfaces as a timeout or as a fail-pattern match midway through the test, and the failure mode looks like "the test is broken" rather than "the host lacks the tool".

Guard the function with a marker that probes for the command via `which()`. Every test that calls the function inherits the skip via propagation -- one guard, N protected tests, no per-test boilerplate.

Don't:

```relux
fn start_postgres() {
    > docker run -d --name pg postgres:16
    <? ^[0-9a-f]{64}$
    match_ok()
}
```

Do:

```relux
# skip unless which("docker")
fn start_postgres() {
    > docker run -d --name pg postgres:16
    <? ^[0-9a-f]{64}$
    match_ok()
}
```

Standard POSIX tools (`ls`, `cat`, `grep`, `awk`, `echo`) are usually safe to leave unguarded. Minimal containers (Alpine without coreutils, `FROM scratch` images) can lack even these -- guard whenever there is real doubt the tool is on every target host.

When multiple functions depend on the same tool, place the marker on each `fn` directly. Do not hoist onto a shared `pure fn`.

### `# skip` does not shield a broken test

A marker only skips a *well-formed* target. A test whose body fails to lower -- an undefined function call, an import cycle, a bad marker expression -- is reported **Invalid**, not Skipped, even when a `# skip` sits on it. Fix the body; the marker is not a mute button.

Don't (assume the skip hides the broken call):

```relux
# skip
test "wip" {
    shell s { let x := no_such_fn() }   // Invalid, not Skipped
}
```

Do (remove or fix the broken reference, keep the skip if the gate is real):

```relux
# skip
test "wip" {
    shell s { > true; <? . }
}
```

## See also

- [effects-identity](effects-identity.md) -- read if you need effect-level marker propagation rules
- [functions](functions.md) -- read if you need function-level marker propagation rules
- [events-failures](events-failures.md) -- read if you need the `SkipRecord` shape in the structured log
- [environment](environment.md) -- read for the layered env markers evaluate against
