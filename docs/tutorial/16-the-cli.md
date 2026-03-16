# The CLI

[Previous: Condition Markers](15-condition-markers.md)

This is the final article in the tutorial series. You have come a long way — from your [first test](02-getting-started.md) through [send and match](03-send-match-and-logs.md), [variables](06-variables.md), [functions](08-functions.md), [effects](11-effects-and-dependencies.md), [modules](14-modules-and-imports.md), and [condition markers](15-condition-markers.md). You now know the entire Relux DSL. Congratulations — that is a real achievement.

This article covers the tool that drives everything: the `relux` binary itself. You have already used `relux new`, `relux check`, and `relux run` throughout the series. Here we go deeper into every subcommand, every flag, and the workflows they enable.

Here is a typical development cycle, end to end:

```bash
relux new                           # scaffold a project
relux new --test smoke/login        # create a test module
# ... write the test ...
relux check                         # validate without running
relux run                           # execute the full suite
relux run --rerun                   # re-run only the failures
relux history --flaky               # spot intermittent tests
```

Each of these commands has options that give you precise control over what runs, how it runs, and what output you get.

## `relux new`

The `new` subcommand scaffolds projects and modules. Without any flags, it initializes a new Relux project in the current directory:

```bash
relux new
```

This creates:

```
Relux.toml
relux/
  .gitignore
  tests/
  lib/
```

The generated [`Relux.toml`](02-getting-started.md) has all values commented out, showing the defaults. The `.gitignore` excludes `out/` — the directory where test run output goes.

Running `relux new` in a directory that already has a `Relux.toml` is an error. The command will not overwrite an existing project.

### Scaffolding modules

To create a test module:

```bash
relux new --test auth/login
```

This creates `relux/tests/auth/login.relux` with a starter test you can run immediately. The path you provide maps directly to the filesystem under `relux/tests/`. Intermediate directories are created automatically.

To create an [effect](11-effects-and-dependencies.md) module:

```bash
relux new --effect services/database
```

This creates `relux/lib/services/database.relux` with a skeleton effect definition. Effect modules go under `relux/lib/`, matching the [module resolution rules](14-modules-and-imports.md) you already know.

Module paths must follow snake_case rules: lowercase letters, digits, and underscores. Each segment must start with a letter or underscore. The `.relux` extension is added automatically — you do not need to include it.

The `--test` and `--effect` flags are mutually exclusive. You can create one or the other per invocation.

## `relux check`

The `check` subcommand validates test files without executing them. It runs the full front end of the pipeline — lexer, parser, and resolver — catching syntax errors, unresolved names, invalid imports, and circular dependencies. No shells are spawned.

```bash
relux check
```

Without arguments, it checks everything under `relux/tests/`. You can also target specific files or directories:

```bash
relux check relux/tests/auth/
relux check relux/tests/smoke/login.relux
```

On success, it prints `check passed` to stderr. On failure, it prints diagnostic errors with source locations and exits with status 1.

| Flag              | Purpose                                                     |
|-------------------|-------------------------------------------------------------|
| `--manifest PATH` | Use a specific `Relux.toml` instead of auto-discovering one |

## `relux run`

The `run` subcommand executes tests. This is the main event — everything else in the CLI exists to support it.

```bash
relux run
```

Without arguments, it runs all tests under `relux/tests/`. You can target specific files or directories:

```bash
relux run relux/tests/smoke/
relux run relux/tests/auth/login.relux relux/tests/auth/signup.relux
```

### Progress and strategy

Two flags control the experience during a run:

```bash
relux run --progress verbose --strategy fail-fast
```

**`--progress`** controls how much output you see while tests are running:

- `quiet` — minimal output, just the final summary
- `basic` — standard progress reporting (default)
- `verbose` — detailed real-time output including shell I/O

**`--strategy`** controls what happens when a test fails:

- `all` — run every test regardless of failures (default)
- `fail-fast` — stop at the first failure

### Timeout multiplier

The `-m` (or `--timeout-multiplier`) flag scales [tolerance timeouts](09-timeouts.md):

```bash
relux run -m 2.0
```

This doubles every tolerance (`~`) timeout in the suite. If a shell-scoped `~10s` would normally wait 10 seconds, with `-m 2.0` it waits 20 seconds.

Critically, [assertion (`@`) timeouts](09-timeouts.md) are **never scaled**. An `@2s` timeout means "the system must respond within 2 seconds" — that is a correctness check, and stretching it would defeat its purpose.

The default multiplier is `1.0`. It must be a positive finite number.

### Re-running failures

After a run with failures, you can re-run only the failed tests:

```bash
relux run --rerun
```

This loads the latest run summary from `relux/out/latest`, identifies which tests failed, and runs only those. It ignores any path arguments you provide — the filter comes entirely from the previous run.

If there are no previous runs or no failed tests, the command exits cleanly with status 0.

### Output artifacts

Every run creates a timestamped directory under `relux/out/`:

```
relux/out/
  run-2026-03-16-14-30-00-a1b2c3d4e5/
    artifacts/
    run_summary.toml
    index.html
    ...
  latest -> run-2026-03-16-14-30-00-a1b2c3d4e5/
```

The `latest` symlink always points to the most recent run. The `run_summary.toml` file stores the run summary — this is what `--rerun` and `relux history` read.

Two flags generate additional artifacts in the `artifacts/` subdirectory:

```bash
relux run --tap --junit
```

**`--tap`** generates a [TAP](https://testanything.org/) (Test Anything Protocol) file — a plain-text format understood by many CI systems.

**`--junit`** generates a JUnit XML file — the de facto standard for CI test result ingestion. Most CI platforms (Jenkins, GitHub Actions, GitLab CI) can parse JUnit XML to display test results in their UI.

Both flags can be used together. They are independent of each other and of the console output.

### All `run` flags

| Flag                   | Short | Default       | Purpose                                       |
|------------------------|-------|---------------|-----------------------------------------------|
| `--progress`           |       | `basic`       | Output verbosity: `quiet`, `basic`, `verbose` |
| `--strategy`           |       | `all`         | Run strategy: `all` or `fail-fast`            |
| `--timeout-multiplier` | `-m`  | `1.0`         | Scale tolerance (`~`) timeouts by this factor |
| `--rerun`              |       |               | Re-run only failed tests from the latest run  |
| `--tap`                |       |               | Generate TAP artifact file                    |
| `--junit`              |       |               | Generate JUnit XML artifact file              |
| `--manifest`           |       | auto-discover | Path to `Relux.toml`                          |

## `relux history`

The `history` subcommand analyzes data from previous runs. It reads the `run_summary.toml` files stored in each run directory under `relux/out/` and computes statistics across them.

You must specify exactly one analysis type:

### `--flaky`

Shows the flakiness rate per test — how often each test alternates between passing and failing:

```bash
relux history --flaky
```

This is your first stop when a test starts intermittently failing. 

### `--failures`

Shows failure frequency and distribution by failure mode (timeout, assertion, etc.):

```bash
relux history --failures
```

This helps you spot patterns. If most failures are timeouts, you may need to adjust your timeout strategy. If they cluster around a specific assertion, there is a targeted bug.

### `--first-fail`

Shows the most recent pass-to-fail regression per test:

```bash
relux history --first-fail
```

Useful for pinpointing when a test started breaking. Combined with your version control history, this helps trace failures back to specific changes.

### `--durations`

Shows duration trends and statistics — min, max, mean, and trend across runs:

```bash
relux history --durations
```

Use this to catch tests that are getting progressively slower, or to identify outliers that might benefit from tighter timeouts.

### Filters

All four analysis types support the same set of filters:

```bash
relux history --flaky --tests relux/tests/auth/ --last 10 --top 5
```

| Flag              | Purpose                                                                                     |
|-------------------|---------------------------------------------------------------------------------------------|
| `--tests PATH...` | Filter to specific test files or directories                                                |
| `--last N`        | Limit to the N most recent runs                                                             |
| `--top N`         | Show only the top N results                                                                 |
| `--format`        | Output format: `human` (default, formatted tables) or `toml` (structured, machine-readable) |
| `--manifest`      | Path to `Relux.toml`                                                                        |

The `--format toml` option is particularly useful for scripting — pipe the output into another tool or parse it programmatically.

## Best practices

### Use `--rerun` after fixing a failure

When a run has failures and you think you have fixed the issue, use `relux run --rerun` instead of re-running the full suite. This targets only the tests that failed last time, giving you faster feedback. Once the reruns pass, do a full `relux run` to confirm nothing else broke.

### Match strategy to context

Use `--strategy fail-fast` during local development — you want to know about the first failure quickly so you can fix it. Use `--strategy all` in CI — you want a complete picture of the suite's health, not just the first problem.

### Start flakiness investigation with `history`

When a test starts failing intermittently, run `relux history --flaky` before digging into the test code. The flakiness rate tells you whether you are dealing with an environment issue (sporadic) or a logic bug (consistent). If the test passes 95% of the time, you are probably looking at a timing issue. If it passes 50% of the time, there may be a race condition or uncontrolled dependency.

---

Next: Appendix A1 — Patterns and Recipes, a practical cookbook for common testing scenarios
