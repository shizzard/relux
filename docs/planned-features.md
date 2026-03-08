# Planned Features

- Pure functions: functions that disallow send and match operators, can only call other pure functions, and can be called outside of a shell context
- Built-in functions: more string utilities
- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- TAP and JUnit output: machine-readable test result formats for CI integration (flags registered, generation not yet implemented)
- Run history: assemble timelines of test results across multiple runs with revision tracking
- Re-run failed tests: `--rerun` flag to re-run only failed tests from the latest run (requires run history)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults

## Bugs

- Function scope leaks to caller: assignment (`x = "val"`) inside a function walks the frame stack and mutates variables in the caller's scope. Functions should execute in an isolated scope where assignment cannot reach the caller. Discovered via e2e test `tests/relux/tests/variables/scoping.relux` — "function assignment mutates outer scope" currently passes but documents incorrect behavior.
- Bare `$N` triggers variable interpolation: the lexer treats `$` followed by a digit (e.g. `$1`) as a capture group interpolation, even without braces. Only the `${N}` form should trigger interpolation. This causes problems when `$` appears naturally in operator payloads (e.g. `\$10` in a regex pattern has `$1` consumed as capture group 1). Discovered via e2e test `tests/relux/tests/variables/interpolation.relux`.

## Lux features not yet in Relux

### Matching
- Buffer reset: bare match operator with no argument resets (dumps) the output buffer, discarding all unmatched output (Lux `?` with no argument)
- Multi-line match: match a block of expected output preserving indentation (Lux `"""?..."""`)
- Permutation match: match a set of patterns in any order (Lux `?+`)
- Success pattern: end the test early as passed when a pattern matches (Lux `+`)

### Shell control
- PTY size control: set terminal dimensions for a shell (Lux `[shell name width=80 height=24]`)

### Execution control
- Loop: repeat a block of statements (Lux `[loop var items]...[endloop]`)

### Debugging
- Interactive debugger: step through test scripts interactively with breakpoints (Lux `--debug`)
- Expected vs actual diff: show diff between expected and actual output on failure
