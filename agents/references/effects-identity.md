# Effects: Identity and Lifecycle

An effect's body shape, its `expect` contract, and the identity tuple that drives deduplication.

## Body sections (fixed order)

1. `expect` -- required overlay variables.
2. `let` -- local bindings; can reference expected vars. May interleave pure-match assertions (`=` / `?`); a `?` capture is readable by later preamble `let`s and by `start` overlay expressions. See [statements](statements.md) > Pure match.
3. `start` -- sub-dependencies; overlay expressions can reference let-bound vars and expected vars.
4. `expose` -- public interface (`expose shell`, `expose var`). See [effects-expose](effects-expose.md).
5. `shell` blocks -- setup logic.
6. `cleanup` -- optional teardown. See [cleanup](cleanup.md).

Out-of-order sections are a parse error. All sections are optional.

## The `expect` contract

```relux
effect Service {
    expect PORT, LOG_LEVEL
    expose shell s
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
}
```

- `expect` lists the overlay variables the effect declares as required.
- The resolver validates that every `start Service { ... }` site supplies them.
- `expect` is a contract, not a sandbox: the effect can still read any environment variable. It only restricts what the resolver enforces and what participates in identity.

### What goes in `expect`

`expect` names the **unique external resources** the service contends for -- the things two simultaneous instances would physically collide on. Three categories cover almost every effect:

- **Filesystem resources** -- data dirs, log dirs, socket paths, lockfile paths.
- **Network resources** -- ports, host bindings, named pipes.
- **Logical shared state** -- database names in a shared cluster, S3 prefixes, Redis keyspace prefixes.

Everything else stays out: log levels, debug flags, feature toggles, internal timeouts. Those ride inherited env transparently; the resolver does not gate on them and they do not fragment dedup.

When N independent instances are needed but the service has no natural resource differentiator, add an `INSTANCE` discriminator to `expect` -- see *Force a fresh instance with a dummy overlay key* below.

## Overlay

```relux
start Service as A {
    PORT := 8080
    LOG_LEVEL := "info"
}
// shorthand for PORT := PORT
start Service as B {
    PORT
}
```

- Provided at the `start` site, evaluated at setup time.
- Layered on top of the inherited environment for the effect's shells.
- Unspecified overlay keys fall through to the inherited env.
- Each overlay entry sits on its own line. No comma separators; no single-line `{ K := v, K2 := v2 }` form. The DSL is line-oriented throughout.

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
- Failed setup propagates: it fails the one test whose dependency graph contains this instance.

### Scope: per-test, not per-suite

The effect registry is constructed fresh for each test; identity and refcount apply only within a single test's dependency graph. Two tests that each `start Db` with identical overlays get **two independent `Db` instances** with two independent setups and two independent cleanups -- no sharing across tests, even when the overlay tuples match exactly. Sharing happens *inside* one test's DAG (a diamond like `Tasks -> Db` plus `Tasks -> Auth -> Db` reuses the same `Db` instance for both legs); it never crosses the test boundary.

This is what makes parallel execution safe: tests do not implicitly share state through an effect, even when they nominally `start` the same one. If you want cross-test reuse, model the shared resource outside the effect system (e.g. an externally-provisioned database the effects connect to).

## Pitfalls and best practices

### One service per effect

Every effect models a **single** service -- one binary, container, daemon, or external resource. Two peer services (an API plus its database) get two effects with a dep relationship, not one effect whose setup shell spawns both. Bundling makes the dep graph implicit, breaks dedup (the bundle has one identity even when its parts contend on different resources), and collapses cleanup into a single span.

Don't:

```relux
effect ApiWithDb {
    expose shell setup
    shell setup {
        > pg_ctl start
        <? ready
        > my-api &
        <? listening
    }
}
```

Do:

```relux
effect Db { expect DATA_DIR, PORT; expose shell db; shell db { ... } }

effect Api {
    expect API_PORT
    start Db as Dep { DATA_DIR; PORT := 5432 }
    expose shell Dep.db as db
    expose shell svc
    shell svc {
        > my-api --db-port=5432
        <? listening
    }
}
```

### Run the service in the foreground

PTY (shell) termination kills the process tree the shell launched. A backgrounded service (`&`, `nohup`, `setsid`, `docker run -d`) detaches from that tree and survives the shell's death -- the test ends but the service keeps running, the next test trips over leftover ports, sockets, lockfiles, or data files. Always launch the service foreground; the service runs until the PTY dies, and PTY death is how Relux guarantees teardown.

For containers the same rule applies: foreground mode (`-i`) plus auto-removal (`--rm`), no `-d`. If a service self-daemonises by default, pass the flag that keeps it foreground (`-F`, `-D`, `--foreground`, etc.).

Don't:

```relux
shell svc {
    > ./run.sh &
    > docker run -d --name pg ...
    > nohup my-service &
}
```

Do:

```relux
shell svc {
    > ./run.sh
    > docker run --rm -i ...
}
```

### Keep `expect` to identity-relevant variables

Every variable in `expect` fragments dedup within a test. Listing a `LOG_LEVEL` you don't actually want to dedupe on means a test that starts the effect twice with different log levels pays the full setup cost twice instead of reusing one instance. Across tests there is nothing to fragment -- see *Scope: per-test, not per-suite* above.

Don't:

```relux
effect WebService {
    expect PORT, LOG_LEVEL
    expose shell s
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
}
```

Do:

```relux
effect WebService {
    expect PORT
    expose shell s
    shell s {
        > app --port=${PORT} --log=${LOG_LEVEL}
    }
}
```

(The effect still reads `LOG_LEVEL` from inherited env; it just doesn't participate in identity.)

### Identity is sensitive to the evaluated overlay, not the start-site shape

Two `start` sites with the same overlay values share an instance even when written differently. Different overlay values produce separate instances even with the same alias name.

Don't:

```relux
// two instances; full setup twice
start WebService {
    PORT := 8080
}
start WebService {
    PORT := 8081
}
```

Do:

```relux
// one instance; second start is reused
start WebService {
    PORT := 8080
}
start WebService {
    PORT := 8080
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
    INSTANCE := 1
    DB_NAME := "test"
}
start Db as B {
    INSTANCE := 2
    DB_NAME := "test"
}
```

## See also

- [block-structure](block-structure.md) -- read if you need the overall effect block shape
- [effects-expose](effects-expose.md) -- read if you need `expose shell`, `expose var`, or re-export rules
- [cleanup](cleanup.md) -- read if you need teardown rules
- [statements](statements.md) -- read if you need `start` syntax and overlay semantics
