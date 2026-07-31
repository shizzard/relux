# Interpolation

`${...}` substitutes a value into a string position at statement execution time.

## Bare vs interpolation

Use the form that matches the surrounding syntactic context:

| Context | Form |
|---|---|
| Inside a string literal, send/match/pattern payload, regex body | `${name}`, `${1}`, `${Alias.var}` |
| Bare expression: `let` RHS, BIF / function argument, marker condition, overlay value | `name`, `$1`, `Alias.var` |

```relux
// bare BIF call
let port := available_port()
// interpolation inside a string
let url  := "http://localhost:${port}/api"
// interpolation inside the send payload
> curl ${url}
// bare in BIF arg position
let host := default(HOST, "localhost")
// bare in marker condition (above a test/effect declaration)
# skip unless CI
```

Don't wrap a bare variable as `"${VAR}"` to "use it as a value" -- bare is already the value.

## Forms

| Form | Source |
|------|--------|
| `${name}` | Variable lookup: shell scope, then test/effect scope, then env. |
| `${1}`, `${2}`, ... | Capture groups from the most recent `<?` in the shell. |
| `${Alias.var}` | Exposed variable from an effect started as `Alias`. |
| `$$` | Literal `$` (escape for shell-side `${...}` etc.). |
| `name`, `$1`, `Alias.var` | Bare reference (used in `let` RHS, BIF args, marker conditions, overlay values). |

- Lookup order: shell-scope -> test/effect-scope -> environment. The environment itself is layered -- `.env` files over the host process env; see [environment](environment.md).
- Undefined names interpolate to the empty string -- no error, no warning.
- Environment variables can be shadowed by lowercase `let` bindings; uppercase env names have no valid shadowing local name and are always readable.

## Where interpolation runs

- In `>`, `=>` payloads -- evaluated at send time.
- In `<?`, `<=`, `!?`, `!=` payloads -- evaluated when the operator runs.
- In `let` right-hand sides and pure function arguments.
- In marker condition expressions.
- Inside multimatch inner lines.

## Regex interpolation is raw

Inside a `<?` or `!?` payload, the interpolated value is inserted as raw regex text. Metacharacters in the value act as metacharacters. If the value should match as-is, use `<=` / `!=` instead.

## Pitfalls and best practices

### `${...}` only accepts a name, not an expression

The braces hold exactly one reference: `${name}` (variable), `${1}` (capture index), or `${Alias.var}` (qualified var). Function calls, BIF calls, arithmetic, or any other expression inside the braces fail to parse. Bind the expression to a `let` first, then interpolate the binding.

Don't:

```relux
let db_path := "${__RELUX_RUN_ARTIFACTS}/users-${uuid()}.db"
> curl http://localhost:${available_port()}/health
<? port=${default(PORT, "8080")}
```

Do:

```relux
let suffix := uuid()
let db_path := "${__RELUX_RUN_ARTIFACTS}/users-${suffix}.db"

let port := available_port()
> curl http://localhost:${port}/health

let port := default(PORT, "8080")
<? port=${port}
```

### Empty or unset variables interpolate to empty string

A pattern with `${name}` where `name` is empty becomes a pattern with the surrounding text only. It can match unexpectedly.

Don't:

```relux
let host := ""
<? user=alice host=${host}
```

Do:

```relux
let host := default(HOST, "localhost")
<? user=alice host=${host}
```

### Regex interpolation is unescaped

Special regex characters in a let-bound value are still metacharacters when spliced into a `<?` pattern. Use literal match if the value should be matched verbatim.

Don't:

```relux
let path := "/api/v1.0/users"
<? GET ${path}
```

Do:

```relux
let path := "/api/v1.0/users"
<= GET ${path}
```

### Use `$$` to delegate to shell-side expansion

When you want the shell (not Relux) to expand `${...}`, escape with `$$`. The shell receives the literal `${...}` and does its own expansion.

Don't:

```relux
> echo "${HOME}/relux"
<= /home/me/relux
```

Do:

```relux
> echo "$${HOME}/relux"
<= /home/me/relux
```

### Bare `$name` (no braces) passes through to the shell

Relux only recognises the braced forms (`${name}`, `${1}`, `${Alias.var}`). Anything else starting with `$` -- bare `$i`, `$HOME`, `$1` outside a braced form -- is treated as literal text by the Relux parser and reaches the shell verbatim, where the shell does its own expansion. You don't need `$$` for bare `$name`; only for `${...}` you want the shell to handle.

```relux
// Relux sees `$i` as literal; the shell loop expands it
> for i in $(seq 1 3); do echo $i; done
<? ^1$
```

## See also

- [environment](environment.md) -- read for how `.env` layers form the environment at the bottom of the lookup chain
- [statements](statements.md) -- read if you need to know where interpolation appears
- [matching](matching.md) -- read if you need raw-splicing semantics in `<?` patterns
- [bifs](bifs.md) -- read if you need `default`, `trim`, `replace` for safe value derivation
