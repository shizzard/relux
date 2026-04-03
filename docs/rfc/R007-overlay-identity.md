# R007: Overlay Identity

- **Status**: rejected
- **Created**: 2026-03-30
- **Superseded by**: R008

## Motivation

Effect identity determines when two `need` declarations share a single effect instance versus creating separate ones. The current identity is **(effect name, canonical overlay)**, where the canonical overlay is built from the AST expression structure of each overlay entry. This representation is broken.

### The pass-through problem

Consider a test that needs both a database and an auth service, where the auth service itself depends on the database:

```relux
effect StartDb -> db {
    shell db {
        > start-db --port ${DB_PORT}
        <? listening on port ${DB_PORT}
    }
}

effect StartAuth -> auth {
    need StartDb {
        DB_PORT = DB_PORT
    }
    shell auth {
        > start-auth --db-port ${DB_PORT} --port ${AUTH_PORT}
        <? auth ready
    }
}
```

The test wants a single shared database instance:

```relux
let db_port = available_port()
let auth_port = available_port()

need StartDb { DB_PORT = db_port }
need StartAuth { DB_PORT = db_port, AUTH_PORT = auth_port }
```

The test's `need StartDb` and StartAuth's internal `need StartDb` should deduplicate — they refer to the same database on the same port. But the current AST-based canonical form produces:

- Test: `DB_PORT=V:db_port`
- Inside StartAuth: `DB_PORT=V:DB_PORT`

Different strings, no deduplication. The user gets two database instances when they intended one.

### Why not just fix the canonical form?

The canonical overlay representation cannot be based on AST expressions (variable names are local to their scope, so the same value reached through different names never matches). But it also cannot naively use evaluated values, because that introduces a different problem:

```relux
need StartDb { DB_PORT = FOO }
need StartDb { DB_PORT = BAR }
```

The user's intent is clear: two separate database instances. But if `FOO` and `BAR` happen to evaluate to the same port number at runtime (`FOO=1100 BAR=1100 relux run`), value-based identity would silently merge them into one instance. This is wrong — the user wrote two different expressions precisely because they wanted two instances.

## Design

### Two representations

Each overlay entry `KEY = expr` produces two representations:

- **Canonical representation**: the AST expression structure, independent of runtime values. Built from the expression syntax: `"5432"` → `S:L:5432`, `db_port` → `V:db_port`, `get_port()` → `C:get_port()`. Entries are sorted by key and joined. Two overlay blocks with identical source expressions produce identical canonical representations. This is the current implementation.

- **Evaluated representation**: the concrete string value obtained by evaluating the pure expression at effect setup time. Built by executing the expression in the current scope and using the resulting string. Entries are sorted by key and joined. Two overlay blocks that evaluate to the same key-value pairs produce identical evaluated representations, regardless of how the expressions were written.

### Effect instance identity

Effect instance identity switches from **(effect name, canonical overlay)** to **(effect name, evaluated overlay)**.

The evaluated representation is computed at effect setup time. Overlay expressions are already constrained to be pure, so evaluation is safe and deterministic within a given scope.

This fixes the pass-through problem: the test's `need StartDb { DB_PORT = db_port }` and StartAuth's `need StartDb { DB_PORT = DB_PORT }` both evaluate to the same port string, producing the same identity — correct deduplication.

### Accidental-collision warning

The canonical representation is retained for a safety check. Within a single test or effect body, the runtime compares all `need` declarations that target the same effect and emits a warning when their canonical representations differ but their evaluated representations are identical.

This catches cases where the user intended separate instances but the runtime values happen to collide.

#### Algorithm

```
for each test or effect body:
    group all need declarations by (effect name, evaluated overlay)
    for each group with more than one need:
        if all needs in the group have the same canonical overlay:
            continue   # identical expressions, intentional sharing
        for each key in the overlay:
            spans_for_key = []
            for each need in the group:
                spans_for_key.append((need.overlay[key].span,
                                      need.evaluated[key]))
            # only report keys where at least two canonical exprs differ
            if count of distinct canonical[key] values > 1:
                colliding_keys.append((key, spans_for_key))
        emit ariadne warning with:
            message: "different overlay expressions evaluate to the
                      same values — these will share a single
                      {effect} instance"
            for each (key, spans) in colliding_keys:
                for each (span, value) in spans:
                    label at span: "evaluated to: {value}"
            help: "if you intended separate instances, use distinct
                   values"
```

The check is **local to a single scope** — it only compares `need` declarations within the same test or effect body. Cross-scope deduplication (a test's `need` matching an effect's internal `need`) is the intended behavior and never triggers a warning.

#### Example output

```
warning: different overlay expressions evaluate to the same values — these
         will share a single StartDb instance
   ┌─ tests/app.relux:5:27
   │
 5 │     need StartDb { DB_PORT = FOO } as db1
   │                              ^^^ evaluated to: 1100
 6 │     need StartDb { DB_PORT = BAR } as db2
   │                              ^^^ evaluated to: 1100
 7 │     need StartDb { DB_PORT = BAZ } as db3
   │                              ^^^ evaluated to: 1100
   │
   = help: if you intended separate instances, use distinct values
```

## Examples

### Pass-through: correct dedup

```relux
let db_port = available_port()

need StartDb { DB_PORT = db_port }
need StartAuth { DB_PORT = db_port, AUTH_PORT = auth_port }
```

Inside StartAuth, `need StartDb { DB_PORT = DB_PORT }` evaluates `DB_PORT` to the same port value. Same evaluated overlay → same identity → one shared `StartDb` instance.

### Separate instances: correct separation

```relux
let port_a = available_port()
let port_b = available_port()

need StartDb { DB_PORT = port_a } as db1
need StartDb { DB_PORT = port_b } as db2
```

`port_a` and `port_b` evaluate to different values → different identities → two `StartDb` instances. No warning because the values differ.

### Accidental collision: warning

```relux
need StartDb { DB_PORT = FOO } as db1
need StartDb { DB_PORT = BAR } as db2
```

If `FOO == BAR` at runtime: warning. The user wrote different expressions (suggesting intent for separate instances) but got the same evaluated overlay. They should either use the same expression (if they want sharing) or ensure the values differ (if they want separation).
