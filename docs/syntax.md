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
effect <EffectName> -> shell <exported_shell> {
    need <EffectName> as <alias>
    need <EffectName> as <alias> { KEY = "value" }
    shell <name> { <body> }
    cleanup { <body> }
}
```

- `-> shell <name>` declares the exported shell
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

- `<?` matches regex against shell output; sets `${1}`, `${2}`, etc. for capture groups
- `<=` matches literal with variable substitution
- Both block until match or timeout

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

- Humantime format: `~10s`, `~1m`, `~500ms`
- Sets timeout for subsequent match operations in the current shell
- Overrides previous timeout

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
