# Relux Syntax Reference

## General

- Line-oriented, newline-terminated statements (no `;`)
- Comments: `//` to end of line
- All values are strings
- Every expression produces a string value
- Blocks use `{ }`

## Naming Conventions

Naming conventions are enforced at the syntactic level (parse error on violation):

- **Effect names** and **effect aliases** must start with an uppercase letter (`CamelCase`): `StartDb`, `Effect1`, `start Db as MyDb`
- **Function names** and **shell names** must be `snake_case`: `start_server`, `_helper`, `my_shell`
- **Variable names** and **parameters** are permissive (any alphanumeric + underscore, starting with letter or `_`): `port`, `DB_HOST`, `_private`
- **Import aliases** must preserve the casing kind of the original name: `foo as bar` (both lowercase), `StartDb as Db` (both uppercase)
- **Overlay keys** accept either casing (environment variables are conventionally `UPPER_SNAKE_CASE`)

## Imports

```relux
import <path> { <name>, <name> as <alias>, }
import <path>
```

- `<path>` resolves from project root (e.g. `lib/module1`)
- Selective: `import lib/m { foo, bar as b, StartDb as Db }` — trailing commas allowed
- Wildcard: `import lib/m` — imports all names

## Functions

```relux
fn <name>(<param>, <param>) {
    <body>
}
```

- Return value: last expression in body
- Execute in the caller's shell context
- Shell operators (`>`, `=>`, `<?`, `<=`, etc.) are valid inside body

## Pure Functions

```relux
pure fn <name>(<param>, <param>) {
    <body>
}
```

- Return value: last expression in body
- Cannot contain shell operators (`>`, `=>`, `<?`, `<=`, `!?`, `!=`, timeouts)
- Cannot call impure built-in functions or regular `fn` functions
- Only `let`, variable reassignment, and expressions (including pure BIF calls) are allowed
- Can be called from condition markers, overlay expressions, and regular shell blocks

## Effects

```relux
effect <EffectName> {
    expect <VAR>, <VAR>, <VAR>
    start <EffectName>
    start <EffectName> as <Alias>
    start <EffectName> as <Alias> { KEY := expr, KEY }
    let <name> := <expr>
    <expr> = <pattern>                       // setup pure match (asserts)
    <expr> ? <pattern>                        // setup pure match (binds $n)
    expose shell <shell_name>
    expose shell <Alias>.<shell_name> as <public_name>
    expose var <var_name>
    expose var <Alias>.<var_name> as <public_name>
    shell <name> { <body> }
    shell <Alias>.<shell_name> { <body> }
    cleanup { <body> }
}
```

- `expect` declares required environment variables (comma-separated)
- `start` declares dependencies (one per line)
- `start Effect` runs the dependency for side effects only — its shells are not accessible
- `start Effect as Alias` runs the dependency and makes its exposed shells/variables available via dot-access
- Effect aliases must be CamelCase
- `start Effect as Alias { KEY := expr }` provides an overlay; shorthand `KEY` is equivalent to `KEY := KEY`
- An overlay value may reference a sibling start's exposed variable: `KEY := Alias.var` (`Alias` must be a sibling's alias in the same start-list and must `expose var` that name — never `expose shell`, since overlay values are strings). This induces an implicit ordering dependency; see [Effects](00-semantics.md#effects) and [Effect Identity](#effect-identity) below
- `expose shell` declares which shells are part of the effect's public interface
- `expose var` declares which variables are part of the effect's public interface; these are `let`-bound values computed during setup
- `expose shell Alias.shell as name` re-exports a dependency's shell under a new name
- `expose var Alias.var as name` re-exports a dependency's variable under a new name
- `shell Alias.shell_name { ... }` — qualified shell block for operating on a dependency's exposed shell
- Internal shells not listed in `expose` are terminated after setup
- `cleanup` block: only `>`, `=>`, `let`, variable reassignment allowed (no match operators)

## Tests

```relux
test "<name>" ~<duration> {
test "<name>" @<duration> {
test "<name>" {
    """
    <doc string>
    """
    let <name>
    <expr> = <pattern>                  // preamble pure match (asserts)
    <expr> ? <pattern>                  // preamble pure match (binds $n)
    start <EffectName>
    start <EffectName> as <Alias>
    start <EffectName> as <Alias> { KEY := expr, KEY }
    shell <name> { <body> }
    shell <Alias>.<shell_name> { <body> }
    cleanup { <body> }
}
```

- Test-level `start` overlay rules (including the sibling-reference form `KEY := Alias.var`) are identical to an effect body's `start` — see [Effects](#effects) above
- A qualified reference `${Alias.var}` in a test's `shell` or `cleanup` body is validated the same way: `Alias` must be one of the test's own `start` dependency aliases, and that dependency must `expose var` the referenced variable

## Condition Markers

```relux
# kind                                  // unconditional
# kind modifier expr                    // truthiness check
# kind modifier expr = expr             // exact-equality comparison
# kind modifier expr ? regex            // regex match (unanchored)
```

Where:
- `kind`: `skip` | `run` | `flaky`
- `modifier`: `if` | `unless`
- `expr`: quoted string with interpolation (`"${VAR}"`, `"literal"`, `"${A}:${B}"`) or bare number (`42`)
- `regex`: regex pattern with `${var}` interpolation, to end of line
- `=` tests exact equality (LHS equals RHS); for a substring or pattern check use `?` (regex). Unlike the shell literal-match operators `<=`/`!=`, which scan a streaming buffer for a substring, a marker compares against a complete value.

Examples:
```relux
# skip
# skip unless "${CI}"
# run if "${OS}" = "linux"
# run if "${COUNT}" = 0
# skip unless "${ARCH}" ? ^(x86_64|aarch64)$
# flaky if "${CI}" = "true"
# run if "${HOST}:${PORT}" = "localhost:8080"
# skip unless "${VER}" ? ^${MAJOR}\..*$
# skip unless "${PATH}" ? bin           // substring / pattern match via regex
```

- A bare marker (kind only, no modifier) is unconditional
- One marker per line
- Multiple markers stack with AND semantics (all must pass or test is skipped)
- Placed immediately before `test`, `effect`, `fn`, or `pure fn` declarations (not inside the body)
- When a function is skipped, all tests that call it are also skipped
- A `# flaky` marker on a function or effect propagates too: the test is marked flaky when a function or effect it reaches is flaky
- Comments between markers and the declaration are allowed

| Marker    | Modifier | Condition | Meaning |
|-----------|----------|-----------|---------|
| `# skip`  | _(none)_ | _(unconditional)_ | always skip |
| `# skip`  | `if`     | truthy    | skip when condition is true |
| `# skip`  | `unless` | falsy     | skip when condition is false |
| `# run`   | _(none)_ | _(unconditional)_ | no-op (always run) |
| `# run`   | `if`     | falsy     | skip when condition is false |
| `# run`   | `unless` | truthy    | skip when condition is true |
| `# flaky` | _(none)_ | _(unconditional)_ | always mark as flaky |
| `# flaky` | `if`     | truthy    | mark as flaky when condition is true |
| `# flaky` | `unless` | falsy     | mark as flaky when condition is false |

### Truthiness

- Empty string or unset variable = false
- Any non-empty string = true
- `=` returns the LHS value if it equals RHS exactly, empty string otherwise (use `?` for a substring or pattern check)
- `?` returns the regex match if matched, empty string otherwise

## Shell Blocks

```relux
shell <name> {
    <statements>
}
shell <Alias>.<shell_name> {
    <statements>
}
```

- Unqualified form (`shell name`) creates or switches to a local shell; name must be snake_case
- Qualified form (`shell Alias.shell_name`) operates on a dependency's exposed shell; qualifier is a CamelCase effect alias, name is snake_case
- Valid inside `effect` and `test` blocks

## Variables

```relux
let <name>                   # declare, defaults to ""
let <name> := "<value>"      # declare with value
let <name> := <expression>   # declare from expression
<name> := <expression>       # reassign existing variable
```

- Binding uses `:=` — declaration (`let x := e`), reassignment (`x := e`), and overlay entries (`{ KEY := e }`) all use it. Bare `=` is no longer a binding operator; it is the exact-equality [pure-match](08-pure-matching.md) assertion at statement level (`x = e` asserts `x` equals `e`) and the literal-match arm inside a multimatch block (`= <literal>`, see below). `let x = e` is an error — use `let x := e`.
- Quoted values required for `let` assignments
- Interpolation inside strings: `"${name}"`, `"${1}"`, `"${2}"`, etc.
- Bare variable reference: `name`, `$1`, `$2`
- Escape `$` with `$$`
- Scoped to enclosing block; inner blocks can shadow outer variables
- Environment variables are readable (the layered base environment — host process plus any `.env` files — is available everywhere)

## Operators

All operators are followed by a space, then payload to end of line.

### Send

| Operator | Payload | Value |
|----------|---------|-------|
| `> `     | text to EOL | sent string |
| `=> `    | text to EOL | sent string |

- `>` sends with trailing newline
- `=>` sends without trailing newline (raw send)
- Variable interpolation applies in payload

### Match

| Operator | Payload | Value |
|----------|---------|-------|
| `<? `    | regex to EOL | full match (`$0`) |
| `<= `    | literal to EOL | matched text |

- `<?` matches regex against shell output; sets `$1`, `$2`, etc. for capture groups
- `<=` matches literal with variable substitution
- Both block until match or timeout

### Pure Match

Statement forms that assert a computed value against a pattern (see
[Pure Matching](08-pure-matching.md)). Distinct from `<?` / `<=`, which
scan a shell's output buffer; a pure match compares a complete value and
reads nothing from the PTY.

| Statement           | Meaning                                                        |
| ------------------- | -------------------------------------------------------------- |
| `<expr> = <pattern>`| assert `<expr>` equals `<pattern>` exactly (literal equality)  |
| `<expr> ? <pattern>`| assert `<expr>` matches the regex `<pattern>`; binds `$0`..`$n`|

- `<expr>` is any pure expression (identifier, quoted/interpolated string, function call, `Alias.var`, `$n`, number).
- `<pattern>` is an interpolated string to end of line (same shape as the `<=` / `<?` payload).
- `=` is exact equality (not a substring test); use `?` for a substring or pattern check.
- A no-match fails the test immediately — a pure match is an assertion and cannot time out. There is no negated form.
- Valid in `shell` blocks, `fn` bodies, `pure fn` bodies, and **test / effect preambles** (alongside `let`). Inside a `pure fn`, a `?` match binds captures into the function's own per-call frame, so `$1` can be returned to extract a value.
- In a preamble, a `?` match binds `$n` into a preamble capture frame that later preamble `let`s and `start` overlays read; a `shell` block does **not** inherit it (a shell owns its own frame). `$n` reads the ambient frame, `""` when unset. See [Pure Matching](08-pure-matching.md#preamble-captures-and-the-shell-boundary).
- Statement-only: cannot be a `let` right-hand side, an overlay value, or a cleanup value.

```relux
os = linux                 // passes only if os is exactly "linux"
"${HOST}:${PORT}" ? ^db\.local:\d+$
greeting ? (hello) (world) // on a hit: $1="hello", $2="world"
```

### Multi-Pattern Match

```
<{
    ? <regex>
    = <literal>
}
```

- Inner lines are `? <regex>` (regex) or `= <literal>` (literal), one per line
- The block waits for every pattern to match at least once, in any order
- The cursor advances once at block exit, to `max(end)` across all per-pattern matches
- Capture groups in inner regex patterns do not bind
- The empty form `<{ }` is a parse error
- Comments are permitted between inner lines

### Buffer Reset

```relux
<?
<=
```

- A match operator with no payload consumes all current output and resets the cursor
- Useful to skip past output you don't care about

### Inline Timeout Override

Any match operator can be prefixed with `~<duration>` (tolerance) or `@<duration>` (assertion) to set a one-shot timeout:

```relux
<~2s? regex pattern       # regex match with 2s tolerance timeout
<~500ms= literal text     # literal match with 500ms tolerance timeout
<@2s? regex pattern       # regex match with 2s assertion timeout
<@500ms= literal text     # literal match with 500ms assertion timeout
<~10s{ ? a  ? b }         # multi-pattern block with 10s tolerance timeout
<@500ms{ = a  = b }       # multi-pattern block with 500ms assertion timeout
```

- Duration uses compact humantime format (no spaces): `2s`, `500ms`, `1m30s`
- Applies only to that single operation — does not affect the scoped timeout
- Works with both match operators (`?`, `=`) and the multi-pattern form `<{ ... }`
- Tolerance (`~`) timeouts are scaled by `--timeout-multiplier`; assertion (`@`) timeouts are never scaled

### Fail Pattern

| Operator | Payload |
|----------|---------|
| `!? `    | regex to EOL |
| `!= `    | literal to EOL |

- One active fail pattern at a time (single slot)
- Setting a new one replaces the previous (regex or literal)
- An empty `!?` or `!=` (no payload) clears the active fail pattern

### Timeout

```relux
~<duration>
@<duration>
```

- Compact humantime format (no spaces): `~10s`, `@2s`, `~500ms`, `~2m30s`
- `~` sets a **tolerance** timeout — scaled by `--timeout-multiplier`
- `@` sets an **assertion** timeout — never scaled (asserts the system responds within a hard deadline)
- Sets timeout for subsequent match operations in the current shell
- Overrides previous timeout
- Scoped to the current function call — reverts when the function returns

## Expressions

Every expression produces a string value:

| Expression | Value |
|------------|-------|
| `"<text>"` | string literal |
| `name` | variable value |
| `Alias.var` | a dependency's exposed variable (dot-access) |
| `$1`, `$2` | regex capture group |
| `<fn>(<args>)` | function return value |
| `> <text>` / `=> <text>` | sent string |
| `<? <regex>` | full match (`$0`) |
| `<= <literal>` | matched text |
| `<~dur? <regex>` | full match with timeout override |
| `<~dur= <literal>` | matched text with timeout override |
| `let x := <expr>` | assigned value |

Last expression in a function body is the return value.

## Effect Identity

`(effect-name, evaluated overlay restricted to expect-declared vars)` determines instance identity:
- Same tuple = same instance (deduplicated)
- Different tuple = different instance
- Overlay expressions are evaluated at setup time; identity is based on evaluated values, not AST form
- A `KEY := Alias.var` sibling reference is an overlay value like any other: it is evaluated (the sibling is `Ready` by then) before identity is derived, so two dependents sourcing different sibling values get distinct identities

## Cleanup Blocks

```relux
cleanup {
    <statements>
}
```

- Runs in a fresh implicit shell
- Any statement valid in a shell block is valid in a cleanup block
- Always executes, regardless of pass/fail
