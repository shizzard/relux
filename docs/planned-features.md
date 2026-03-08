# Planned Features

- Pure functions: functions that disallow send and match operators, can only call other pure functions, and can be called outside of a shell context
- Built-in functions: more string utilities
- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- TAP and JUnit output: machine-readable test result formats for CI integration (flags registered, generation not yet implemented)
- Run history: assemble timelines of test results across multiple runs with revision tracking
- Re-run failed tests: `--rerun` flag to re-run only failed tests from the latest run (requires run history)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults

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
