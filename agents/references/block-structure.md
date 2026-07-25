# Block Structure

The top-level forms of a `.relux` file and the rules that govern how each block's body is laid out.

## Test block

```relux
test "name" {
    let x = "..."
    start Effect
    shell s { ... }
    cleanup { ... }
}
```

- Top-level form. Name is a string literal.
- Body sections in fixed order: optional `"""doc"""`, `let` bindings, `start` declarations, `shell` blocks, optional `cleanup`.
- Must contain at least one `shell` block (or be entirely skipped by a marker).
- Tests are not exported across modules.
- Multiple `shell <name> { ... }` blocks with the same name re-enter the same shell process: the first occurrence spawns it, subsequent occurrences resume in its existing buffer/cursor/state. Use this when the test body is easier to read as several distinct phases against the same shell.
- See [cleanup](cleanup.md) for teardown.

## Effect block

```relux
effect Name {
    expect VAR1, VAR2
    let x = "..."
    start Other as O
    expose shell name
    shell name { ... }
    cleanup { ... }
}
```

- Name is CamelCase (parse-enforced).
- Section order is fixed and enforced by the parser:
  1. `expect`
  2. `let`
  3. `start`
  4. `expose`
  5. `shell` blocks
  6. `cleanup` (at most one)
- Any section is optional; out-of-order is a parse error.
- See [effects-identity](effects-identity.md) for the identity contract, [effects-expose](effects-expose.md) for the public interface, [cleanup](cleanup.md) for teardown.

## Function block

```relux
fn name(a, b) { ... }
pure fn name(a, b) { ... }
```

- Name is snake_case (parse-enforced).
- Return value is the last expression in the body.
- `fn` runs in caller's shell context (no own shell); `pure fn` cannot use shell operators or impure BIFs.
- See [functions](functions.md).

## Naming rules (parse-enforced)

- Effects, effect aliases: CamelCase. Examples: `StartDb`, `Db`, `start Db as MyDb`.
- Functions, shells: snake_case. Examples: `start_server`, `my_shell`.
- Variables, parameters, expect/overlay keys: any alphanumeric + underscore starting with a letter or `_`. Convention: `snake_case` for locals, `UPPER_SNAKE_CASE` for env-derived.
- Import aliases must preserve the casing kind of the original: `foo as bar` (both lowercase), `StartDb as Db` (both uppercase).
- Case distinguishes effects from functions in imports and `start`/call sites.

## Pitfalls and best practices

### Effect section order is enforced

Out-of-order sections fail at parse time. The order reflects data flow: `expect` declares inputs, `let` derives values, `start` wires deps, `expose` declares the public surface, `shell` runs setup, `cleanup` undoes external side effects.

Don't:

```relux
effect Db {
    expose shell db
    expect DB_URL
    shell db {
        > psql ${DB_URL}
    }
}
```

Do:

```relux
effect Db {
    expect DB_URL
    expose shell db
    shell db {
        > psql ${DB_URL}
    }
}
```

### Case disambiguates kinds; mismatched aliases are parse errors

`import lib/services { Db }` brings in the effect `Db`. `import lib/services { db }` brings in a function or pure function named `db`. The same rule applies to aliases: `foo as Bar` is a parse error (kind mismatch).

Don't:

```relux
import lib/services { Db as db }
```

Do:

```relux
import lib/services { Db as MyDb }
import lib/services { greet as hello }
```

## See also

- [statements](statements.md) -- read if you need to know what goes inside shell blocks
- [effects-identity](effects-identity.md) -- read if you need the effect contract or dedup rules
- [functions](functions.md) -- read if you need fn vs pure fn semantics
- [imports](imports.md) -- read if you need import syntax or resolution rules
