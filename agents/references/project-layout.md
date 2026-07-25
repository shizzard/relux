# Project Layout

The `Relux.toml` manifest, the directory convention, and how `.relux` files are discovered.

## Project root

```text
suite-root/
|-- Relux.toml
|-- .env             # optional; committed env values (see environment.md)
`-- relux/
    |-- tests/       # test files (*.relux), discovered recursively; a dir may hold its own .env
    |-- lib/         # reusable functions and effects (always loaded)
    `-- out/         # run output (auto-generated, gitignored)
```

- The suite root is the directory containing `Relux.toml`.
- The CLI discovers `Relux.toml` by walking upward from the current directory; `--manifest <path>` overrides.
- `relux init` scaffolds the layout in the current directory.

## `Relux.toml`

An empty `Relux.toml` is valid; every field has a default.

### Sections

| Section | Field | Default | Notes |
|---|---|---|---|
| root | `name` | parent directory name | Suite name. |
| `[shell]` | `command` | `/bin/sh` | Shell executable per spawned shell. |
| `[shell]` | `prompt` | `relux> ` | PS1 prompt set on shell init. |
| `[timeout]` | `match` | `5s` | Default match-op timeout (tolerance). |
| `[timeout]` | `test` | `5m` | Per-test wall-clock budget (tolerance). |
| `[timeout]` | `suite` | `10m` | Whole-run budget (tolerance). |
| `[run]` | `jobs` | `1` | Number of parallel test workers. |
| `[flaky]` | `max_retries` | `0` | Retries for `# flaky` tests; 0 disables retry. |
| `[flaky]` | `timeout_multiplier` | `1.5` | Base for exponential timeout scaling on retry; must be > 1.0. |

Durations use humantime: `500ms`, `2s`, `1m30s`.

## File discovery

- `relux run` (no `--file`) discovers `*.relux` recursively under `relux/tests/`.
- `--file <path>` (repeatable) overrides discovery; directories are walked recursively.
- `relux/lib/` is always loaded -- functions and effects defined there are available to tests.
- A nested `Relux.toml` is a sub-project boundary; discovery stops at it.

## Environment files

A `.env` beside `Relux.toml`, and a `.env` in any directory on a test's path, feed the test's environment. Deeper files win. See [environment](environment.md) for discovery, precedence, and provenance.

## Module paths

A file at `relux/lib/utils/greeter.relux` has the module path `utils/greeter`. The `lib/` prefix and `.relux` extension are stripped. Import the file with `import utils/greeter` (or selective form). See [imports](imports.md).

## Built-in environment variables

The runtime injects five env vars into every shell on top of the inherited process env. Each is readable like any other env var: `${__RELUX_RUN_ID}` inside a string, or bare in BIF/marker positions.

| Name | Value |
|---|---|
| `__RELUX_RUN_ID` | Identifier of the current run -- the same value used in the `relux/out/<run>/` directory name. Useful as a per-run discriminator for temp paths. |
| `__RELUX_RUN_ARTIFACTS` | Absolute path to the per-test artifacts directory under the current run. Files dropped here are picked up by `scan_artifacts` and listed in the test's `artifacts[]`. |
| `__RELUX_SHELL_PROMPT` | The configured shell prompt (default `relux> `). Mirrors `[shell].prompt`. |
| `__RELUX_SUITE_ROOT` | Absolute path to the suite root (the directory containing `Relux.toml`). |
| `__RELUX` | Absolute path to the `relux` binary itself. Useful for sub-invocations from within a test. |

## Pitfalls and best practices

### Keep `Relux.toml` at the suite root

The CLI walks up to find `Relux.toml`. A stray manifest in `relux/tests/` or `relux/lib/` becomes the new root and breaks discovery.

Don't:

```text
suite-root/
|-- relux/
|   |-- tests/
|   |   `-- Relux.toml          # now tests/ is the root; lib/ disappears
|   `-- lib/
```

Do:

```text
suite-root/
|-- Relux.toml
`-- relux/
    |-- tests/
    `-- lib/
```

## See also

- [imports](imports.md) -- read if you need module paths and import syntax
- [environment](environment.md) -- read if you need `.env` discovery, precedence, or provenance
- [cli-reference](cli-reference.md) -- read if you need `--manifest`, `--file`, or `--jobs`
- [timeouts](timeouts.md) -- read if you need `[timeout]` defaults or the multiplier
- [ci-integration](ci-integration.md) -- read if you need output-directory layout or artifact-upload rules
