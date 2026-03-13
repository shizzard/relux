# Planned Features

- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults


## Lux features not yet in Relux

### Matching
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
