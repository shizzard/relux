# Planned Features

- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- Run history analysis: timeline visualization, flakiness detection, and runtime trend analysis across historical runs (foundation: `run_summary.toml` per run)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults

## Known Bugs

### Inconsistent error for impure calls from pure context

Calling a user-defined impure function from a `pure fn` produces a clear resolver diagnostic:
`"name/arity (impure function cannot be called from pure context)"`. But using a shell operator
(`>`, `<?`, etc.) or an impure BIF (`match_prompt()`, `ctrl_c()`, etc.) inside a `pure fn` body
hits a different code path — the parser rejects it as `"unexpected token"` because the `pure_stmt`
grammar excludes operator tokens entirely.

From the user's perspective, both cases are the same mistake: impure code in a pure context. The
error should be uniform. Either:
- Push the check to the resolver so all three cases produce the same diagnostic, or
- Make the parser error message mention purity (e.g., "shell operators are not allowed in pure
  functions") so the user understands why.

### Imported functions cannot call siblings from their home module

When a function is imported from another module via selective import, calls to sibling functions
(defined in the same source module but not imported by the caller) fail at runtime with
`"undefined function"` even though `relux check` passes.

Example: module `lib/m.relux` defines `fn helper()` and `fn caller()` where `caller()` calls
`helper()`. If another file does `import lib/m { caller }` and calls `caller()`, the resolver
resolves the call to `helper()` using the home module's scope (check passes), but the reachability
walk that builds the runtime `Plan` does not follow cross-module call edges — so `helper()` never
makes it into the `CodeServer` and the call fails at runtime.

The fix should ensure the reachability walk transitively includes all functions called from imported
function bodies, using the home module's scope for resolution.

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
