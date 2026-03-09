# Relux E2E Self-Tests

Relux tests itself using its own DSL. This directory is a standalone Relux project
(with its own `Relux.toml`) where `.relux` test files exercise the framework end-to-end.

## Running

```bash
just e2e              # run all e2e tests
just e2e relux/tests/operators/  # run a single group
```

## Implemented Test Groups

### `operators/` — Shell send/match operators

Core interaction primitives: sending input and matching output.

- `send.relux` — `>` and `=>` operators, interpolation, escaping
- `match_regex.relux` — `<?` regex matching, capture groups, cursor advancement
- `match_literal.relux` — `<=` literal matching, substring, special chars
- `neg_match.relux` — `<!?` and `<!=` negative match assertions
- `timed_match.relux` — `<~dur?` and `<~dur=` inline timeout overrides
- `timed_neg_match.relux` — `<~dur!?` and `<~dur!=` inline timeout on negative matches
- `fail_pattern.relux` — `!?` and `!=` fail pattern operators
- `clear_fail_pattern.relux` — bare `!?` and `!=` to clear active fail patterns
- `buffer_reset.relux` — bare `<?` and `<=` to discard unmatched output and advance cursor
- `timeout.relux` — `~dur` scoped timeout

### `variables/` — Variable system

- `let_and_assign.relux` — declaration, initialization, reassignment
- `scoping.relux` — block scoping, shadowing, mutation of outer scope
- `interpolation.relux` — `${var}` in send/match payloads, `$$` dollar escaping, undefined variables
- `capture_groups.relux` — `${0}`, `${1}`, `${N}` regex capture groups, overwriting, let-from-match
- `environment.relux` — host env vars, `__RELUX_*` special vars, undefined env var fallback
- `bif_returns.relux` — capturing built-in function return values into variables, chaining

### `functions/` — Function definitions and calls

- `basic_call.relux` — calling functions, arity dispatch, parameter/local isolation, caller shell context
- `return_value.relux` — expression and let as return value, match capture return, empty function
- `side_effects.relux` — timeout isolation across function calls, fail pattern leak (documented bug)
- `nested_calls.relux` — chained calls, return value threading, parameter scope independence

### `effects/` — Effect lifecycle

- `basic_setup.relux` — effect with exported shell, alias, no-alias default, state retention
- `dependencies.relux` — `need` chains, transitive dependencies, topological execution order
- `deduplication.relux` — same identity tuple reuses instance, different overlays create separate instances
- `cleanup.relux` — effect cleanup blocks, reverse dependency order, test-level cleanup

### `bifs/` — Built-in functions

- `string_ops.relux` — `trim`, `upper`, `lower`, `replace`, `len`, `split`
- `generators.relux` — `uuid`, `rand`
- `control_chars.relux` — `ctrl_c`, `ctrl_d`, `ctrl_z`, `ctrl_l`, `ctrl_backslash`
- `log.relux` — `log` and `annotate`

### `imports/` — Module imports

- `wildcard.relux` — wildcard import bringing all names into scope
- `selective.relux` — selective import of functions and effects by name
- `aliases.relux` — `as` aliases for imported functions and effects

### `cli/` — Binary interface (meta-tests)

Each subdirectory is a self-contained mini-project with a `.relux` meta-test
that invokes the `relux` binary and verifies its output or exit code.

#### `cli/check/` — Static analysis diagnostics

- `parse_bad_fn_def/` — rejects function definitions without a name
- `parse_bad_import/` — rejects import statements without a module path
- `parse_bad_operator/` — rejects invalid operators in shell blocks
- `parse_missing_brace/` — rejects test bodies with missing closing braces
- `parse_missing_test_name/` — rejects test definitions without a name string
- `undefined_name/` — detects calls to undefined functions
- `undefined_variable/` — detects use of undefined variables
- `wrong_arity/` — detects function calls with wrong number of arguments
- `invalid_regex/` — detects invalid regex patterns in condition markers
- `invalid_timeout/` — detects unparseable timeout durations
- `module_not_found/` — detects import of nonexistent modules
- `circular_import/` — detects circular module imports
- `circular_effect_dep/` — detects circular effect dependencies
- `duplicate_definition/` — detects duplicate function definitions via imports
- `import_not_exported/` — detects selective import of names not exported by the module

#### `cli/run/` — Test execution behaviour

- `valid_check/` — exits with code 0 and prints "check passed" for a valid project
- `exit_code_pass/` — exits with code 0 when all tests pass
- `exit_code_fail/` — exits with code 1 when any test fails
- `match_timeout/` — reports match timeout when a pattern is never found
- `fail_pattern/` — reports when a fail pattern matches output
- `negative_match_failed/` — reports when a negative match finds the unwanted pattern
- `fail_fast/` — stops after the first test failure with `--strategy fail-fast`
- `file_not_found/` — reports a clear error for non-existent test file paths
- `tap_output/` — generates valid TAP14 results file with `--tap`
- `junit_output/` — generates valid JUnit XML results file with `--junit`
- `test_timeout/` — fails when a test exceeds its inline per-test timeout (`test "name" ~100ms { ... }`)

#### `cli/marker/` — Condition markers

- `skip_unconditional/` — bare `@skip` unconditionally skips the test
- `skip_if/` — skips when condition variable is set; runs when unset
- `skip_if_eq/` — skips when variable equals value; runs when different or unset
- `skip_unless/` — runs when condition variable is set; skips when unset
- `skip_unless_regex/` — runs when variable matches pattern; skips when not matching or unset
- `run_if/` — runs when condition variable is set; skips when unset
- `run_unless/` — skips when condition variable is set; runs when unset
- `flaky/` — flaky marker causes the test to be reported as skipped
- `multiple/` — multiple markers with AND semantics

## Covered Elsewhere

The following empty directories exist in `tests/` but their coverage lives in
unit tests or the `cli/` meta-tests above. They are kept as placeholders but
have no `.relux` files:

| Directory | Coverage |
|---|---|
| `lexer/` | 66 unit tests in `src/dsl/lexer/mod.rs` |
| `parser/` | 70 unit tests in `src/dsl/parser/mod.rs` |
| `resolver/` | 29 unit tests in `src/dsl/resolver/mod.rs` + `cli/check/` meta-tests |
| `markers/` | `cli/marker/` meta-tests (see above) |
| `reporting/` | `cli/run/tap_output`, `cli/run/junit_output` + unit tests in `src/runtime/tap.rs`, `src/runtime/junit.rs` |
