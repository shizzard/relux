# Planned Features

- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults


## Bugs

- Cleanup ordering: all test and effect shells should be terminated before any cleanup block runs. Currently cleanup runs interleaved with shell termination. The correct order is: terminate all test shells, terminate all effect shells, then run cleanup blocks (test cleanup first, then effect cleanup in reverse topological order).
- Cleanup variable scope: test-level and effect-level `let` variables should be available in their respective cleanup blocks. Currently cleanup blocks get a fresh empty scope and can only see overlay variables (for effects) and environment variables.

## Planned RFCs

- Interactive debugger: step through test scripts interactively with breakpoints
