# R009: Variable Match Operator

- **Status**: draft
- **Created**: 2026-04-07

## Motivation

Currently, asserting against a variable's string value requires a shell round-trip: echo the variable into the shell and match the output back. This is awkward and indirect for what is fundamentally a pure string operation.

```relux
// Current workaround: echo and match back
let version = get_version()
shell main {
    > echo ${version}
    <? (\d+)\.(\d+)\.(\d+)
}
```

This couples a pure assertion to a shell, adds noise to the test, and pollutes the shell's output buffer.

## Design

### Two new statement-level operators

| Operator | Form | Semantics |
|----------|------|-----------|
| `\|=` | `var \|= <literal>` | Partial literal match against variable value |
| `\|?` | `var \|? <regex>` | Partial regex match against variable value |

The variable's value acts as a buffer. The match is partial — the literal or regex must match a substring of the variable's value, not the entire value. This is consistent with how `<=` and `<?` match against shell output.

### Syntax

```relux
x |= expected text with ${interpolation}
x |? \d+(\.\d+)*
```

- The left-hand side is a variable name (must be in scope)
- `|=` is followed by literal text to end of line (with variable interpolation)
- `|?` is followed by a regex pattern to end of line (with variable interpolation)
- No quoting required for the literal form (same convention as `<=`)

### Semantics

- **Failure mode**: if the match fails, the test fails — same as `<=`/`<?` timeout failure
- **Capture groups**: `|?` sets `$0` (full match), `$1`, `$2`, etc. — same as `<?`
- **Return value**: `|=` returns the matched substring, `|?` returns `$0` — this makes them usable as the last statement in a function body to provide a return value
- **Interpolation**: variable interpolation (`${var}`) applies in both literal and regex payloads

### Purity

These operators are impure. Although the underlying operation is purely computational (no I/O, no shell), they can fail the test. Test failure requires a test execution context to report into. Pure functions can be called from markers and overlay expressions, which evaluate before any test exists — a match failure in that context would have nowhere to go.

Therefore `|=` and `|?` are:
- **Valid in**: shell blocks, `fn` bodies, cleanup blocks
- **Not valid in**: `pure fn` bodies, marker expressions, overlay expressions

This introduces a new dimension to the purity model: prior to this RFC, impurity was synonymous with "requires a shell." With variable match, impurity means "requires a test execution context" — a broader category that includes shell operations but is not limited to them. All purity definitions in the documentation must be updated to reflect this.

### Comparison with markers

Condition markers use `=` and `?` with similar but distinct semantics:

| | Markers (`=` / `?`) | Variable match (`\|=` / `\|?`) |
|---|---|---|
| Match extent | Full value | Partial (substring) |
| On mismatch | Controls skip/flaky | Fails the test |
| Capture groups | No | Yes (`\|?` sets `$1`, `$2`, ...) |
| Context | Before declarations | Inside blocks |

The `|` prefix distinguishes the partial-match assertion operators from the full-value marker comparisons.

### Example

```relux
test "version parsing" {
    shell main {
        let version = get_version()

        // Assert version matches semver pattern, extract components
        version |? (\d+)\.(\d+)\.(\d+)
        let major = $1
        let minor = $2
        let patch = $3

        // Assert a substring is present
        version |= SNAPSHOT
    }
}
```

## Impact

### Lexer

Two new composite tokens: `PipeLiteralMatch` (`|=`) and `PipeRegexMatch` (`|?`).

### Parser

New statement variants in `AstStmt` for variable literal match and variable regex match. The parser distinguishes these from variable reassignment (`x = expr`) by the `|` prefix.

### Resolver

New IR statement variants. The resolver validates that the left-hand side variable is in scope. Regex patterns go through the existing `regex_validate` pass. These operators are rejected in `pure fn` bodies.

### Runtime / VM

New statement execution paths. For `|?`, compile the regex and match against the variable's string value. For `|=`, search for the literal substring. On match: set capture groups (regex form), return the matched text. On mismatch: produce a test failure with diagnostic context (variable name, value, pattern).

### Observability

Variable match operations must be logged through the existing observability infrastructure:
- **Event sink**: new event variants for variable match attempts and results
- **Progress**: live progress updates during variable match execution
- **Shell log**: record variable match operations in the per-shell debug log
- **HTML report**: include variable match results in the rich test report

### Documentation

- Update `docs/reference/00-semantics.md` — add variable match section, update purity definitions
- Update `docs/reference/02-syntax.md` — add variable match operator syntax
- Update `docs/dsl-tutorial/` — relevant tutorial sections
- Update `docs/suite-tutorial/` — relevant tutorial sections
- Update all purity definitions across docs to reflect the expanded impurity model (not just "requires a shell" but "requires a test execution context")
