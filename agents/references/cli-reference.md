# CLI Reference

`relux` subcommands and the flags an agent needs.

## `relux init`

Scaffold `Relux.toml` and `relux/{tests,lib,out}/` in the current directory. Errors if `Relux.toml` exists.

## `relux new`

Create a module file from a template.

| Flag | Behavior |
|---|---|
| `--test <module>` | Create `relux/tests/<module>.relux`. |
| `--effect <module>` | Create `relux/lib/<module>.relux` with an effect template. |
| `--lib <module>` | Create `relux/lib/<module>.relux` with a function template. |

Module path uses `/` separators; each segment matches `[a-z_][a-z0-9_]*`. The `.relux` extension is optional.

## `relux check [paths...]`

Parse and resolve the suite without running. Reports diagnostics; exits non-zero on any error. Always run before `run` -- it catches resolve/syntax errors in seconds.

| Flag | Behavior |
|---|---|
| `--manifest <path>` | Override `Relux.toml` discovery. |

## `relux run [flags]`

Run the suite. Discovers `Relux.toml` by walking upward.

| Flag | Behavior |
|---|---|
| `-f`, `--file <path>` | Test file or directory (repeatable). Default: `relux/tests/`. |
| `-t`, `--test <name>` | Filter by test name (repeatable; requires exactly one `--file`). |
| `--manifest <path>` | Override `Relux.toml` discovery. |
| `-j`, `--jobs <N>` | Parallel workers. Default: `1`. |
| `-m`, `--timeout-multiplier <N>` | Scale tolerance (`~`) timeouts. Assertion (`@`) timeouts never scale. |
| `--strategy <mode>` | `all` (default) or `fail-fast`. |
| `--rerun` | Re-run only non-passing tests from the latest run. |
| `--flaky-retries <N>` | Retries for `# flaky` tests (overrides `[flaky].max_retries`). |
| `--flaky-multiplier <N>` | Exponential timeout base for retries (must be > 1.0). |
| `--test-timeout <dur>` | Override per-test timeout (humantime). |
| `--suite-timeout <dur>` | Override suite timeout (humantime). |
| `--progress <mode>` | `auto` (TTY: TUI), `plain`, `tui`. |
| `--tap` | Emit TAP artifact in the run directory. |
| `--junit` | Emit JUnit XML artifact in the run directory. |

Exits non-zero if any test fails or is cancelled.

## `relux history [flags]`

Analyze prior runs from `relux/out/`.

| Flag | Behavior |
|---|---|
| `--manifest <path>` | Override `Relux.toml` discovery. |
| `--flaky` | Tests that have both passed and failed. |
| `--failures` | Tests that have failed. |
| `--first-fail` | First failure per test. |
| `--durations` | Duration statistics per test. |
| `--tests <path>...` | Filter to test files or directories. |
| `--last <N>` | Limit to the N most recent runs. |
| `--top <N>` | Show only the top N results. |
| `--format <fmt>` | `human` (default) or `toml`. |

## `relux dump <subcommand> <files...>`

Subcommands: `tokens`, `ast`, `ir`. Emits internal representations to stdout for debugging.

## `relux completions [flags]`

Install shell completions. Without `--install`, prints a dry-run.

| Flag | Behavior |
|---|---|
| `--shell <shell>` | `bash`, `zsh`, `fish` (default: from `$SHELL`). |
| `--install` | Write the completion script. |
| `--path <path>` | Override install path (required for zsh). |

## Exit semantics

- `0` -- all tests passed (or skipped).
- non-zero -- any test failed, cancelled, or any diagnostic on `check`.

## Pitfalls and best practices

### Run `check` before `run`

`check` catches resolve/syntax errors in seconds. `run` discovers them too, but only after spawning workers and starting shells.

Don't:

```bash
relux run
```

Do:

```bash
relux check && relux run
```

### Use `--rerun` to iterate on the failing subset

After a partial failure, `--rerun` runs only the non-passing tests from the previous run. Much faster than re-running the whole suite.

Don't:

```bash
relux run                  # re-runs everything
```

Do:

```bash
relux run --rerun
```

### Use `--strategy fail-fast` for early-exit CI

Cancels in-flight and queued tests on the first failure. Cancelled tests carry `CancelReason::FailFast` in the structured log.

Don't:

```bash
relux run                  # runs all, even when first one fails
```

Do:

```bash
relux run --strategy fail-fast
```

## See also

- [project-layout](project-layout.md) -- read if you need `Relux.toml` or `relux/out/` layout
- [events-recipes](events-recipes.md) -- read if you need to inspect structured output from a run
- [ci-integration](ci-integration.md) -- read if you need `--junit`, `--tap`, or multiplier usage in CI
