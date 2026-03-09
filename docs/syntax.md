# Relux Syntax Reference

## General

- Line-oriented, newline-terminated statements (no `;`)
- Comments: `#` to end of line
- All values are strings
- Every expression produces a string value
- Blocks use `{ }`

## Naming Conventions

Naming conventions are enforced at the syntactic level (parse error on violation):

- **Effect names** must start with an uppercase letter (`CamelCase`): `StartDb`, `Effect1`, `MyService`
- **Function names**, **variable names**, **shell names**, and **parameters** must start with a lowercase letter or underscore (`snake_case`): `start_server`, `_helper`, `my_shell`
- **Import aliases** must preserve the casing kind of the original name: `foo as bar` (both lowercase), `StartDb as Db` (both uppercase)
- **Overlay keys** accept either casing (environment variables are conventionally `UPPER_SNAKE_CASE`)

## Imports

```
import <path> { <name>, <name> as <alias>, }
import <path>
```

- `<path>` resolves from project root (e.g. `lib/module1`)
- Selective: `import lib/m { foo, bar as b, StartDb as Db }` — trailing commas allowed
- Wildcard: `import lib/m` — imports all names

## Functions

```
fn <name>(<param>, <param>) {
    <body>
}
```

- Return value: last expression in body
- Execute in the caller's shell context
- Shell operators (`>`, `=>`, `<?`, `<=`, etc.) are valid inside body

## Effects

```
effect <EffectName> -> <exported_shell> {
    need <EffectName> as <alias>
    need <EffectName> as <alias> { KEY = "value" }
    shell <name> { <body> }
    cleanup { <body> }
}
```

- `-> <name>` declares the exported shell
- `need` declares dependencies (one per line)
- `cleanup` block: only `>`, `=>`, `let`, variable reassignment allowed (no match operators)

## Tests

```
test "<name>" {
    """
    <doc string>
    """
    need <EffectName> as <alias>
    need <EffectName> as <alias> { KEY = "value" }
    let <name>
    shell <name> { <body> }
    cleanup { <body> }
}
```

## Condition Markers

```
[kind]                                  # unconditional
[kind modifier expr]                    # truthiness check
[kind modifier expr = expr]             # equality comparison
[kind modifier expr ? regex]            # regex match
```

Where:
- `kind`: `skip` | `run` | `flaky`
- `modifier`: `if` | `unless`
- `expr`: quoted string with interpolation (`"${VAR}"`, `"literal"`, `"${A}:${B}"`) or bare number (`42`)
- `regex`: regex pattern with `${var}` interpolation, up to closing `]`

Examples:
```
[skip]
[skip unless "${CI}"]
[run if "${OS}" = "linux"]
[run if "${COUNT}" = 0]
[skip unless "${ARCH}" ? ^(x86_64|aarch64)$]
[flaky if "${CI}" = "true"]
[run if "${HOST}:${PORT}" = "localhost:8080"]
[skip unless "${VER}" ? ^${MAJOR}\..*$]
```

- A bare marker (kind only, no modifier) is unconditional
- One marker per line
- Multiple markers stack with AND semantics (all must pass or test is skipped)
- Placed immediately before `test` or `effect` declarations (not inside the body)
- Comments between markers and the declaration are allowed

| Marker | Modifier | Condition | Meaning |
|--------|----------|-----------|---------|
| `skip` | _(none)_ | _(unconditional)_ | always skip |
| `skip` | `if`     | truthy    | skip when condition is true |
| `skip` | `unless` | falsy     | skip when condition is false |
| `run`  | _(none)_ | _(unconditional)_ | no-op (always run) |
| `run`  | `if`     | falsy     | skip when condition is false |
| `run`  | `unless` | truthy    | skip when condition is true |
| `flaky`| _(none)_ | _(unconditional)_ | always mark as flaky |
| `flaky`| `if`     | truthy    | mark as flaky when condition is true |
| `flaky`| `unless` | falsy     | mark as flaky when condition is false |

### Truthiness

- Empty string or unset variable = false
- Any non-empty string = true
- `=` returns the LHS value if LHS equals RHS, empty string otherwise
- `?` returns the regex match if matched, empty string otherwise

## Shell Blocks

```
shell <name> {
    <statements>
}
```

Valid inside `effect` and `test` blocks.

## Variables

```
let <name>                  # declare, defaults to ""
let <name> = "<value>"      # declare with value
let <name> = <expression>   # declare from expression
<name> = <expression>       # reassign existing variable
```

- Quoted values required for `let` assignments
- Interpolation: `${name}`, `${1}`, `${2}`, etc.
- Escape `$` with `$$`
- Scoped to enclosing block; inner blocks can shadow outer variables
- Environment variables are readable (base env available everywhere)

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
| `<!? `   | regex to EOL | empty string |
| `<!= `   | literal to EOL | empty string |

- `<?` matches regex against shell output; sets `${1}`, `${2}`, etc. for capture groups
- `<=` matches literal with variable substitution
- Both block until match or timeout
- `<!?` and `<!=` are negative matches: assert pattern does NOT appear within the timeout
  - Runs for the full timeout duration
  - Succeeds if timeout expires without finding the pattern
  - Fails if the pattern is found
  - Does NOT advance the output cursor
  - Returns empty string

### Inline Timeout Override

Any match operator can be prefixed with `~<duration>` to set a one-shot timeout:

```
<~2s? regex pattern       # regex match with 2s timeout
<~500ms= literal text     # literal match with 500ms timeout
<~30s!? error regex       # negative regex match with 30s timeout
<~1m30s!= bad stuff       # negative literal match with 1m30s timeout
```

- Duration uses compact humantime format (no spaces): `2s`, `500ms`, `1m30s`
- Applies only to that single operation — does not affect the scoped timeout
- Works with all four match operators: `?`, `=`, `!?`, `!=`

### Fail Pattern

| Operator | Payload |
|----------|---------|
| `!? `    | regex to EOL |
| `!= `    | literal to EOL |

- One active fail pattern at a time (single slot)
- Setting a new one replaces the previous (regex or literal)

### Timeout

```
~<duration>
```

- Compact humantime format (no spaces): `~10s`, `~1m`, `~500ms`, `~2m30s`
- Sets timeout for subsequent match operations in the current shell
- Overrides previous timeout
- Scoped to the current function call — reverts when the function returns

## Expressions

Every expression produces a string value:

| Expression | Value |
|------------|-------|
| `"<text>"` | string literal |
| `${name}` | variable value |
| `${1}`, `${2}` | regex capture group |
| `<fn>(<args>)` | function return value |
| `> <text>` / `=> <text>` | sent string |
| `<? <regex>` | full match (`$0`) |
| `<= <literal>` | matched text |
| `<!? <regex>` | empty string (negative match) |
| `<!= <literal>` | empty string (negative match) |
| `<~dur? <regex>` | full match with timeout override |
| `<~dur= <literal>` | matched text with timeout override |
| `<~dur!? <regex>` | empty string (neg match with timeout override) |
| `<~dur!= <literal>` | empty string (neg match with timeout override) |
| `let x = <expr>` | assigned value |

Last expression in a function body is the return value.

## Effect Identity

`(effect-name, arguments, overlay)` determines instance identity:
- Same tuple = same instance (deduplicated)
- Different tuple = different instance
- Overlays are explicit, never inherited

## Cleanup Blocks

```
cleanup {
    <send and let statements only>
}
```

- Runs in a fresh implicit shell
- No match operators (`<?`, `<=`), no fail patterns, no timeout
- No function calls
- Always executes, regardless of pass/fail
