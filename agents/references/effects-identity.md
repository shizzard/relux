# Effects: Identity and Lifecycle

An effect's body shape, its `expect` contract, and the identity tuple that drives deduplication.

## Body sections (fixed order)

1. `expect` -- required overlay variables.
2. `let` -- local bindings; can reference expected vars.
3. `start` -- sub-dependencies; overlay expressions can reference let-bound vars and expected vars.
4. `expose` -- public interface (`expose shell`, `expose var`). See [effects-expose](effects-expose.md).
5. `shell` blocks -- setup logic.
6. `cleanup` -- optional teardown. See [cleanup](cleanup.md).

Out-of-order sections are a parse error. All sections are optional.

## The `expect` contract

```relux
effect Service {
    expect PORT, LOG_LEVEL
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
    expose shell s
}
```

- `expect` lists the overlay variables the effect declares as required.
- The resolver validates that every `start Service { ... }` site supplies them.
- `expect` is a contract, not a sandbox: the effect can still read any environment variable. It only restricts what the resolver enforces and what participates in identity.

## Overlay

```relux
start Service as A {
    PORT = 8080
    LOG_LEVEL = "info"
}
// shorthand for PORT = PORT
start Service as B {
    PORT
}
```

- Provided at the `start` site, evaluated at setup time.
- Layered on top of the inherited environment for the effect's shells.
- Unspecified overlay keys fall through to the inherited env.
- Each overlay entry sits on its own line. No comma separators; no single-line `{ K = v, K2 = v2 }` form. The DSL is line-oriented throughout.

## Identity tuple

`(effect-name, evaluated overlay restricted to expect-declared vars)`.

- Only `expect`-listed values participate in identity.
- Same tuple -> single instance is created; subsequent `start` sites get a `reused` setup span and share the same shells and exposed vars.
- Different tuple -> separate instances, separate setup runs, separate exposed shells.
- Overlay expressions are evaluated before comparison; identity is based on evaluated string values, not AST form.

## Lifecycle

- Resolve dependency graph -> run effects in topological order.
- For each unique identity: instantiate, run setup shells, keep exposed shells alive.
- Multiple `start` sites with the same identity share one instance via refcounted guards.
- Cleanup runs once per instance at last-guard release, in reverse topological order, under uncancellable tokens. See [cleanup](cleanup.md).
- Failed setup propagates: dependent tests fail.

## Pitfalls and best practices

### Keep `expect` to identity-relevant variables

Every variable in `expect` fragments dedup. Listing a `LOG_LEVEL` you don't actually want to dedupe on means two tests with different log levels each pay the full setup cost.

Don't:

```relux
effect WebService {
    expect PORT, LOG_LEVEL
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
    expose shell s
}
```

Do:

```relux
effect WebService {
    expect PORT
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
    expose shell s
}
```

(The effect still reads `LOG_LEVEL` from inherited env; it just doesn't participate in identity.)

### Identity is sensitive to the evaluated overlay, not the start-site shape

Two `start` sites with the same overlay values share an instance even when written differently. Different overlay values produce separate instances even with the same alias name.

Don't:

```relux
// two instances; full setup twice
start WebService {
    PORT = 8080
}
start WebService {
    PORT = 8081
}
```

Do:

```relux
// one instance; second start is reused
start WebService {
    PORT = 8080
}
start WebService {
    PORT = 8080
}
```

### Force a fresh instance with a dummy overlay key

To get two independent instances of the same effect with otherwise identical config, add a discriminator to `expect` and set distinct values per start site.

Don't:

```relux
// both alias the same instance
start Db as A
start Db as B
```

Do:

```relux
// in the effect:
effect Db {
    expect INSTANCE, DB_NAME
    ...
}

// in the test:
start Db as A {
    INSTANCE = 1
    DB_NAME = "test"
}
start Db as B {
    INSTANCE = 2
    DB_NAME = "test"
}
```

## See also

- [block-structure](block-structure.md) -- read if you need the overall effect block shape
- [effects-expose](effects-expose.md) -- read if you need `expose shell`, `expose var`, or re-export rules
- [cleanup](cleanup.md) -- read if you need teardown rules
- [statements](statements.md) -- read if you need `start` syntax and overlay semantics
