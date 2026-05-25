# Effects: Expose

How an effect publishes shells and computed variables to its callers.

## Forms

| Form | Meaning |
|---|---|
| `expose shell s` | publish a local shell |
| `expose shell s as svc` | publish a local shell under a different name |
| `expose var port` | publish a let-bound value |
| `expose var port as DB_PORT` | publish a let-bound value under a different name |
| `expose shell Dep.s` | re-export a dependency's shell under original name |
| `expose var Dep.port` | re-export a dependency's var under original name |
| `expose shell Dep.s as svc` | re-export a dependency's shell under a new name |
| `expose var Dep.port as db_port` | re-export a dependency's var under a new name |

- `expose` requires a discriminator: `expose shell <name>` or `expose var <name>`. A bare `expose <name>` is a parse error.
- `expose shell` declares a setup-phase shell as part of the public interface; it survives setup and is accessible to callers.
- `expose var` declares a `let`-bound value (computed during setup) as part of the public interface. The target **must** be a `let` name in the same effect; the resolver rejects `expose var X` when `X` is an `expect` / overlay var (or anything else). To re-publish an overlay var, introduce a let shim first: `let port = PORT` then `expose var port`.
- Cross-effect re-export uses dot-access: `expose shell Dep.shell as new_name`.

## Caller access

After `start Service as Svc`:

- `shell Svc.s { ... }` -- operate on the exposed shell.
- `Svc.port` -- read the exposed variable in a bare expression (`let` RHS, BIF/function argument).
- `${Svc.port}` -- read the exposed variable inside a string (send/match payload, interpolated string).

```relux
shell client {
    // bare in let RHS
    let port = Svc.port
    // interpolation inside a string
    let url  = "http://localhost:${Svc.port}"
    // interpolation in send payload
    > curl http://localhost:${Svc.port}/health
    // interpolation in match pattern
    <? listening on ${Svc.port}
}
```

`Alias.var` (bare or interpolated) is only valid in **shell-context code** -- inside a `shell` block, `fn` body, or `cleanup` block. Test-level and effect-level `let` run before any `start` in the same block fires, so the exposed value isn't bound yet when those lets evaluate.

Don't:

```relux
test "uses Svc" {
    // test-level let runs before `start Service` fires;
    // Svc.port has no value yet -- empty interpolation
    let url = "http://localhost:${Svc.port}"
    start Service as Svc {
        PORT = 8080
    }
    shell client {
        > curl ${url}
    }
}
```

Reordering won't save it -- [block-structure](block-structure.md) requires `let` before `start`, so the let cannot be moved after the `start` that would populate `Svc.port`.

Do:

```relux
test "uses Svc" {
    start Service as Svc {
        PORT = 8080
    }
    shell client {
        // shell-context let: setup has populated Svc.port
        let url = "http://localhost:${Svc.port}"
        > curl ${url}
    }
}
```

Exposed variables are read-only from the caller's perspective.

## Non-exposed shells terminate

A `shell` block inside an effect body that is not in any `expose shell` declaration runs during setup, then terminates when setup completes. Only exposed shells survive into the test body.

## Wrapper effects re-expose

When effect `Outer` does `start Inner as Dep`, `Inner`'s exposed shells and vars are NOT automatically surfaced to callers of `Outer`. Outer must re-export with `expose shell Dep.s as ...` (or rename) to make them visible. If a wrapper does not re-export a dep shell, that shell is unreachable from the test that starts the wrapper.

## Pitfalls and best practices

### Wrapper effects must re-expose dependency shells

`start Inner as Dep` runs Inner and gives Outer dot-access to `Dep.s`, but does not surface `Dep.s` to whoever starts Outer. Without `expose shell Dep.s as ...`, callers of Outer cannot reach Inner's shell -- it's still alive (held by Outer's guard) but invisible from outside.

Don't:

```relux
effect SeededDb {
    start Db as Dep
    shell s {
        > psql -c "INSERT INTO ..."
    }
    expose shell s
    // Db.service unreachable to callers of SeededDb
}
```

Do:

```relux
effect SeededDb {
    start Db as Dep
    expose shell Dep.service as service
    shell s {
        > psql -c "INSERT INTO ..."
    }
}
```

### Don't expose setup-only shells

Exposing a shell keeps it alive past setup. If the shell exists only to run `init`-style commands, leave it out of `expose` so it terminates and frees resources.

Don't:

```relux
effect Db {
    expect DB_URL
    expose shell init
    expose shell db
    shell init {
        > pg-init
    }
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
    shell init {
        > pg-init
    }
    shell db {
        > psql ${DB_URL}
    }
}
```

### Re-export with a clean caller-facing name

When a wrapper re-exposes a dep shell, choose the name the caller should read in their own test body, not the internal one.

Don't:

```relux
effect SeededDb {
    start Db as Dep
    expose shell Dep.db as db_internal
}
```

Do:

```relux
effect SeededDb {
    start Db as Dep
    // exposed under original 'db' name
    expose shell Dep.db
}
```

## See also

- [effects-identity](effects-identity.md) -- read if you need effect body structure and identity rules
- [block-structure](block-structure.md) -- read if you need section-ordering rules
- [statements](statements.md) -- read if you need `start`, `shell Alias.name { ... }`, or `${Alias.var}` syntax
