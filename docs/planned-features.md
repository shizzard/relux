# Planned Features

- Pure functions: functions that disallow send and match operators, can only call other pure functions, and can be called outside of a shell context
- Run logs: per-shell stdin/stdout/stderr logs, test log, match logs, written to the run output directory
- Unified binary: single `relux` binary combining all dump tools (token, AST, IR) and the test runner
- Built-in functions: runtime-provided functions available without import, covering string manipulation, prompt matching, sleep, progress annotations, and other common utilities
- Custom shell command: configurable executable for shell spawn, with a global default and per-shell override; introduces new syntax for shell block attributes
- Timeout multiplier: CLI flag to scale all timeouts by a factor for slow CI environments
- Suite and case timeouts: cap total wall-clock time for an entire run and per test case
- Conditional test skips: skip tests based on environment or platform checks, with relux-specific logic
- Progress output levels: configurable verbosity for real-time test execution feedback
- HTML annotated logs: hyperlinked event logs with source cross-references
- TAP and JUnit output: machine-readable test result formats for CI integration (JUnit via `quick-junit`, TAP hand-rolled)
- Run history: assemble timelines of test results across multiple runs with revision tracking
- `latest_run` symlink: always points to the most recent run directory in `relux-out/`

## Lux features not yet in Relux

### Matching
- Multi-line match: match a block of expected output preserving indentation (Lux `"""?..."""`)
- Negative match: assert that a pattern does NOT appear within the timeout (Lux `?-`)
- Permutation match: match a set of patterns in any order (Lux `?+`)
- Success pattern: end the test early as passed when a pattern matches (Lux `+`)

### Shell control
- PTY size control: set terminal dimensions for a shell (Lux `[shell name width=80 height=24]`)

### Execution control
- Loop: repeat a block of statements (Lux `[loop var items]...[endloop]`)

### Debugging
- Interactive debugger: step through test scripts interactively with breakpoints (Lux `--debug`)
- Expected vs actual diff: show diff between expected and actual output on failure
