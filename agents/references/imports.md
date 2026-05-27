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

### Group tests by SUT then scope

Tests under `relux/tests/` nest at minimum two levels: `relux/tests/<sut>/<scope>/`. The first level is the **service under test** -- one subdirectory per distinct SUT the suite covers (a project that tests several binaries or services gets one each: `relux/tests/api/`, `relux/tests/worker/`, `relux/tests/cli/`). The second level is a **scope** within that SUT -- a categorical bucket the tests share.

The only universal scope is `smoke/`: collects smoke tests across the SUT's various surfaces. Every SUT subdirectory gets a `smoke/` for the "does the service come up and serve its primary surface" tests. After `smoke`, scope choices depend on the service and should be **proposed to the user** before authoring -- the right categories vary by what the SUT exposes and how the suite groups them naturally. Common candidates worth proposing (pick 3-5 to surface, do not commit any silently): `auth/`, `api/`, `crud/`, `cli/`, `db/`, `errors/`, `migrations/`, `lifecycle/`, `streaming/`, `config/`, `compat/`.

All names use `snake_case`. SUT subdirectories, scope subdirectories, and the test filenames inside them all share the convention. Kebab-case is not viable: the import-path parser rejects `-` in segment names, so any `relux/lib/` module must be `snake_case` -- and the test tree follows the same convention for consistency (no source-level enforcement, but no precedent in the workspace either). CamelCase is reserved for effect-kind identifiers and reads wrong on a path component.

The filename inside the chosen scope reads as the assertion the test makes, in `snake_case.relux` (e.g. `starts_and_serves_health.relux`, not `smoke.relux` or `test_1.relux`). The filename and the test's `"name string"` are independent: the name string is full prose, the filename is its snake_case condensation.

Don't (flat layout; cross-SUT and cross-category confusion at the top level):

```text
relux/tests/
|-- starts_up.relux
|-- api_creates_user.relux
|-- worker_drains_queue.relux
|-- auth_rejects_expired_token.relux
```

Do (SUT first, scope second; `smoke` is the always-present bucket):

```text
relux/tests/
|-- api/
|   |-- smoke/
|   |   `-- starts_and_serves_health.relux
|   |-- auth/
|   |   |-- rejects_expired_token.relux
|   |   `-- refresh_extends_session.relux
|   `-- crud/
|       |-- creates_user.relux
|       `-- updates_user.relux
`-- worker/
    |-- smoke/
    |   `-- starts_and_drains.relux
    `-- queue/
        `-- drains_in_fifo_order.relux
```

### Group library modules by scope

Do not park every shared helper in a flat `relux/lib/helpers.relux`. Place each helper in the module that names its natural scope:

- **Tied to a specific effect** -- inline the helper in the same module as the effect (a `parse_pid` used only with the `Postgres` effect lives in `relux/lib/postgres.relux`). Callers import `import lib/postgres { parse_pid }`; the effect and its helpers travel together.
- **Scoped by protocol or domain** -- group under a protocol-named module (`relux/lib/http.relux`, `relux/lib/ssh.relux`, `relux/lib/strings.relux` for URL / shell-quoting helpers).
- **Cross-cutting with no natural home** -- stop and pick a name before creating a new top-level module. A premature `relux/lib/misc.relux` becomes a magnet for unclassified helpers and silently degrades discoverability.

The grouping is what import lines read like at the caller: `import lib/postgres { parse_pid }` immediately tells the reader the helper is in the Postgres orbit.

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
