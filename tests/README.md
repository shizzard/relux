# Relux E2E Self-Tests

Relux tests itself using its own DSL. This directory is a standalone Relux project
(with its own `Relux.toml`) where `.relux` test files exercise the framework end-to-end.

## Running

```bash
just e2e              # run all e2e tests
just e2e relux/tests/operators/  # run a single group
```

## Test Groups

### `operators/` — Shell send/match operators

Core interaction primitives: sending input and matching output.

- `send.relux` — `>` and `=>` operators, interpolation, escaping
- `match_regex.relux` — `<?` regex matching, capture groups, cursor advancement
- `match_literal.relux` — `<=` literal matching, substring, special chars
- `neg_match.relux` — `<!?` and `<!=` negative match assertions
- `timed_match.relux` — `<~dur?` and `<~dur=` inline timeout overrides
- `timed_neg_match.relux` — `<~dur!?` and `<~dur!=` inline timeout on negative matches
- `fail_pattern.relux` — `!?` and `!=` fail pattern operators
- `timeout.relux` — `~dur` scoped timeout

### `variables/` — Variable system

- `let_and_assign.relux` — declaration, initialization, reassignment
- `scoping.relux` — block scoping, shadowing, mutation of outer scope
- `interpolation.relux` — `${var}` in strings and payloads
- `captures.relux` — `${1}`, `${2}` regex capture groups
- `env_vars.relux` — environment variable access

### `functions/` — Function definitions and calls

- `basic_call.relux` — defining and calling functions
- `return_value.relux` — last expression as return value, capturing result
- `side_effects.relux` — timeout/fail-pattern changes persist in caller
- `nested_calls.relux` — functions calling other functions

### `effects/` — Effect lifecycle

- `basic_setup.relux` — effect with exported shell
- `dependencies.relux` — `need` chains, topological execution order
- `deduplication.relux` — same identity tuple reuses instance
- `cleanup.relux` — cleanup blocks, execution order
- `exported_shell.relux` — using effect-provided shells in tests

### `markers/` — Condition markers

- `skip.relux` — `[skip if/unless VAR]`
- `run.relux` — `[run if/unless VAR]`
- `flaky.relux` — `[flaky if/unless VAR]`
- `stacking.relux` — multiple markers with AND semantics

### `bifs/` — Built-in functions

- `string_ops.relux` — `trim`, `upper`, `lower`, `replace`, `len`, `split`
- `generators.relux` — `uuid`, `rand`
- `control_chars.relux` — `ctrl_c`, `ctrl_d`, `ctrl_z`, `ctrl_l`, `ctrl_backslash`
- `sleep.relux` — `sleep` with duration
- `log.relux` — `log` and `annotate`

### `lexer/` — Tokenization edge cases

- `escapes.relux` — `$$` dollar escape, string escape sequences
- `interpolation.relux` — `${var}` and `${1}` in various positions
- `operators.relux` — operator token boundaries, whitespace sensitivity

### `parser/` — Syntax validation

- `error_messages.relux` — diagnostic output for malformed input
- `edge_cases.relux` — empty blocks, trailing commas, blank lines

### `resolver/` — Name resolution and imports

- `imports.relux` — wildcard, selective, aliases
- `circular_deps.relux` — circular import detection
- `undefined_names.relux` — undefined function/effect/variable diagnostics
- `effect_graph.relux` — effect dependency graph construction

### `cli/` — Binary interface

- `subcommands.relux` — `new`, `run`, `check`, `dump`
- `flags.relux` — `--tap`, `--junit`, `--progress`, `--strategy`
- `exit_codes.relux` — correct exit codes for pass/fail/error

### `reporting/` — Output formats

- `tap.relux` — TAP artifact generation
- `junit.relux` — JUnit XML output
- `html.relux` — HTML summary report
- `diagnostics.relux` — ariadne error annotation output
