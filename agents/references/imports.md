# Imports

`import <module-path>` brings names from another `.relux` module into scope.

## Forms

| Form | Meaning |
|---|---|
| `import path/to/module` | wildcard: all exports |
| `import path/to/module { foo }` | selective: one name |
| `import path/to/module { foo, bar }` | selective: multiple |
| `import path/to/module { foo, bar as b }` | with alias |
| `import path/to/module { StartDb as Db, greet as hi }` | multiple aliases |

Multi-line and trailing commas are allowed:

```relux
import path/to/module {
    foo,
    StartDb as Db,
}
```

## Module paths

- Path is the file's location under `relux/lib/`, with the `lib/` prefix and `.relux` extension stripped.
- File `relux/lib/utils/greeter.relux` -> module path `utils/greeter`.
- Path separator is `/`. No `./` or `../`. No `::`.
- Resolution is absolute from the suite root (a directory containing the `Relux.toml` file), regardless of where the importing file lives.

## Aliases

- `name as alias` renames the import in this file.
- Aliases must preserve casing kind: `snake_case -> snake_case`, `CamelCase -> CamelCase`.
- A mismatched alias (`foo as Bar` or `StartDb as db`) is a parse error -- the casing is the kind tag.

## What is exported

- `fn`, `pure fn`, `effect` definitions are exported.
- `test` blocks are not exported (tests are local to their file).
- No visibility modifier exists; everything defined is exported.

## Other rules

- Each module is loaded once regardless of import count.
- Circular imports are a parse error.

## Pitfalls and best practices

### Use absolute paths from the suite root; no relative imports

There is no `./` or `../` form. Paths always resolve from `relux/lib/`.

Don't:

```relux
import ../services/db
```

Do:

```relux
import services/db
```

### Match case to the kind you import

CamelCase imports the effect; snake_case imports the function or pure function. If a module exports both, the case picks one.

Don't:

```relux
// imports function `db`; fails if only effect Db exists
import services/db { db }
```

Do:

```relux
// imports effect Db
import services/db { Db }
// imports function greet
import services/db { greet }
// imports both
import services/db { Db, greet }
```

### Group library modules by scope

Do not park every shared helper in a flat `relux/lib/helpers.relux`. Place each helper in the module that names its natural scope:

- **Tied to a specific effect** -- inline the helper in the same module as the effect (a `parse_pid` used only with the `Postgres` effect lives in `relux/lib/postgres.relux`). Callers import `from "lib/postgres" import parse_pid`; the effect and its helpers travel together.
- **Scoped by protocol or domain** -- group under a protocol-named module (`relux/lib/http.relux`, `relux/lib/ssh.relux`, `relux/lib/strings.relux` for URL / shell-quoting helpers).
- **Cross-cutting with no natural home** -- stop and pick a name before creating a new top-level module. A premature `relux/lib/misc.relux` becomes a magnet for unclassified helpers and silently degrades discoverability.

The grouping is what import lines read like at the caller: `from "lib/postgres" import parse_pid` immediately tells the reader the helper is in the Postgres orbit.

### Prefer selective imports for shared library modules

Wildcards bring in everything and obscure which dependencies the file actually uses. For multi-export modules, list the names you depend on.

Don't:

```relux
import services/db
```

Do:

```relux
import services/db { Db, run_migrations }
```

## See also

- [project-layout](project-layout.md) -- read if you need `relux/lib/` layout or module-path derivation
- [block-structure](block-structure.md) -- read if you need effect vs function naming rules
