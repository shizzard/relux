# Planned Features

- Pure functions: functions that disallow send and match operators, can only call other pure functions, and can be called outside of a shell context
- Unified binary: single `relux` binary combining all dump tools (token, AST, IR) and the test runner
- Built-in functions: control character BIFs (ctrl-c, ctrl-d, ctrl-z, etc.), prompt matching, more string utilities
- Custom shell command: configurable executable for shell spawn, with a global default and per-shell override; introduces new syntax for shell block attributes
- Timeout multiplier: CLI flag to scale all timeouts by a factor for slow CI environments
- Suite and case timeouts: cap total wall-clock time for an entire run and per test case
- Conditional test skips: skip tests based on environment or platform checks, with relux-specific logic
- Progress output levels: configurable verbosity for real-time test execution feedback (basic progress implemented)
- TAP and JUnit output: machine-readable test result formats for CI integration (JUnit via `quick-junit`, TAP hand-rolled)
- Run history: assemble timelines of test results across multiple runs with revision tracking

## Lux features not yet in Relux

### Matching
- Buffer reset: bare match operator with no argument resets (dumps) the output buffer, discarding all unmatched output (Lux `?` with no argument)
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
