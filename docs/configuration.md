# Configuration

## Relux.toml

Every Relux project requires a `Relux.toml` file at the project root. The `relux` binary discovers this file by searching the current directory and all parent directories.

### Scaffold a new project

```
relux new
```

Creates `Relux.toml` and the conventional directory structure in the current directory.

### Minimal example

An empty `Relux.toml` is valid — all fields have defaults:

```toml
```

The `name` defaults to the directory containing `Relux.toml`. Override it explicitly if needed:

```toml
name = "my-test-suite"
```

### Full example

```toml
name = "my-test-suite"

[shell]
command = "/bin/sh"
prompt = "relux> "

[timeout]
match = "5s"
test = "5m"
suite = "30m"
```

### Root-level fields

| Field  | Type   | Default                      | Description            |
|--------|--------|------------------------------|------------------------|
| `name` | string | directory containing Relux.toml | Suite name          |

### `[shell]` section

| Field     | Type   | Default      | Description                                 |
|-----------|--------|--------------|---------------------------------------------|
| `command` | string | `/bin/sh`    | Shell executable spawned for each shell      |
| `prompt`  | string | `relux> `    | PS1 prompt set on shell init                 |

### `[timeout]` section

All durations use `humantime` format (e.g. `5s`, `1m30s`, `2h`).

| Field     | Type             | Default | Description                                  |
|-----------|------------------|---------|----------------------------------------------|
| `match`   | duration         | `5s`    | Per-match timeout                            |
| `test`    | duration or null | —       | Max wall-clock time per test                 |
| `suite`   | duration or null | —       | Max wall-clock time for the entire test run  |

## Project structure

```
project-root/
├── Relux.toml
└── relux/
    ├── tests/       # test files (*.relux)
    ├── lib/         # reusable functions and effects
    ├── out/         # run output (auto-generated)
    │   ├── run-2025-03-05-…/
    │   └── latest -> run-2025-03-05-…
    └── .gitignore   # ignores out/
```

- **`relux/tests/`** — test files are discovered recursively when `relux run` is invoked without explicit paths.
- **`relux/lib/`** — library files are always loaded alongside tests to make functions and effects available. May be empty or absent.
- **`relux/out/`** — run output directory. Each run creates a timestamped subdirectory. A `latest` symlink points to the most recent run.

## CLI reference

### `relux new`

Scaffolds a new project in the current directory. Errors if `Relux.toml` already exists.

### `relux new --test <module_path>`

Creates a test module file from a template. The module path uses `/` separators and each segment must be lowercase alphanumeric with underscores (`[a-z_][a-z0-9_]*`). The `.relux` extension is optional.

```
relux new --test foo/bar/baz       # creates relux/tests/foo/bar/baz.relux
relux new --test foo/bar/baz.relux # same
```

### `relux new --effect <module_path>`

Creates an effect module file from a template in `relux/lib/`. Same path rules as `--test`.

```
relux new --effect network/tcp_server       # creates relux/lib/network/tcp_server.relux
```

### `relux run [paths...] [flags]`

Runs tests. Discovers `Relux.toml` by walking upward from the current directory.

**Path arguments** accept both files and directories. Directories are searched recursively for `*.relux` files. If no paths are given, tests are discovered from `relux/tests/`.

Library files from `relux/lib/` are always loaded regardless of which paths are specified.

Exits with code 1 if any test fails.

| Flag                          | Description                                                  |
|-------------------------------|--------------------------------------------------------------|
| `--tap`                       | Generate TAP artifact file in the run directory              |
| `--junit`                     | Generate JUnit XML artifact file in the run directory        |
| `-m`, `--timeout-multiplier`  | Scale all timeout values (default: `1.0`)                    |
| `--progress <level>`          | Output verbosity: `quiet`, `basic`, `verbose` (default: `basic`) |
| `--strategy <mode>`           | `all` (default) or `fail-fast`                               |
| `--rerun`                     | Re-run only failed tests from the latest run                 |

### `relux check [paths...]`

Validates test files without executing them. Runs the parser and resolver, reports diagnostics, and exits with code 1 if any diagnostics are found. Same path discovery as `run`.

### `relux dump tokens <file>`

Dumps lexer tokens for the given file.

### `relux dump ast <file>`

Dumps the parsed AST for the given file.

### `relux dump ir <files...>`

Dumps the resolved IR (execution plans) for the given files.
