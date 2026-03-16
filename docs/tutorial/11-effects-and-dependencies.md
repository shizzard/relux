# Effects and Dependencies

[Previous: Fail Patterns](10-fail-patterns.md)

The previous articles covered everything you need to test a single program in a single shell: [sending commands](03-send-match-and-logs.md), [matching output](07-regex-matching.md), [reusable functions](08-functions.md), [timeouts](09-timeouts.md), and [fail patterns](10-fail-patterns.md). For a self-contained CLI tool, that is enough. But most real systems do not run in isolation.

Consider a web service that depends on a database and a message queue. Before you can test the service, the database needs to be running and migrated, the queue needs to be up, and maybe you want to tail the service's logs in a separate shell with a fail pattern watching for crashes. Every test that exercises this service needs all of that infrastructure in place.

Without effects, you would set up everything manually in each test:

```relux
fn start_db() {
    > start-db --data-dir /tmp/test-db
    <? listening on port 5432
    match_prompt()
}

fn run_migrations() {
    > migrate --db localhost:5432
    <? migrations complete
    match_prompt()
}

test "user signup" {
    shell db {
        start_db()
        run_migrations()
    }
    shell svc {
        > start-my-service --db localhost:5432
        <? ready on :8080
    }
    shell client {
        > curl -s http://localhost:8080/signup -d 'user=alice'
        <? 201 Created
        match_prompt()
    }
}

test "user login" {
    shell db {
        start_db()
        run_migrations()
    }
    shell svc {
        > start-my-service --db localhost:5432
        <? ready on :8080
    }
    shell client {
        > curl -s http://localhost:8080/login -d 'user=alice'
        <? 200 OK
        match_prompt()
    }
}
```

Two tests, and the database and service startup is already duplicated. Functions reduce some repetition, but they run in the caller's shell — they cannot spin up separate, independent services declaratively. And there is no way to share a running database across tests or control the teardown order when things go wrong.

Effects solve this. An effect is a named, reusable piece of test infrastructure. You define it once — what to start, how to verify it is ready — and each test declares what it needs. Relux resolves the dependency graph, starts everything in the right order, and tears it down when the test is done:

```relux
effect StartDb -> db {
    shell db {
        > start-db --data-dir /tmp/test-db
        <? listening on port 5432
        match_prompt()
    }
}

effect MigratedDb -> db {
    need StartDb as db
    shell migrations {
        > migrate --db localhost:5432
        <? migrations complete
        match_prompt()
    }
}

test "user signup" {
    need MigratedDb
    shell svc {
        > start-my-service --db localhost:5432
        <? ready on :8080
    }
    shell client {
        > curl -s http://localhost:8080/signup -d 'user=alice'
        <? 201 Created
        match_prompt()
    }
}

test "user login" {
    need MigratedDb
    shell svc {
        > start-my-service --db localhost:5432
        <? ready on :8080
    }
    shell client {
        > curl -s http://localhost:8080/login -d 'user=alice'
        <? 200 OK
        match_prompt()
    }
}
```

The infrastructure is defined once. Each test says `need MigratedDb` — Relux figures out that `MigratedDb` depends on `StartDb`, starts both in order, and hands the test a shell with a fully migrated database.

A particularly common pattern is monitoring: tail a log file with a fail pattern so any crash in the background aborts the test immediately. This combination is useful enough to deserve its own alias — call it a fail tail:

```relux
effect FailTail -> log {
    shell log {
        !? ${FAILTAIL_TRIGGER}
        > tail -f ${FAILTAIL_LOG}
    }
}

test "service handles load" {
    need FailTail {
        FAILTAIL_TRIGGER = panic|error
        FAILTAIL_LOG = /var/log/service.log
    }
    need Service as svc
    shell client {
        > curl http://localhost:8080/heavy-endpoint
        <? 200 OK
        match_prompt()
    }
}
```

The `FailTail` effect starts tailing the log and sets a fail pattern. If anything fatal appears in the log while the test runs its requests, the test fails on the spot. The `{ FAILTAIL_TRIGGER = ... }` syntax passes configuration into the effect — we will cover these **overlay variables** later in this article. Without effects, you would duplicate this tail-and-fail-pattern setup in every test that exercises the service.

## Defining an effect

An effect definition starts with the `effect` keyword, followed by a CamelCase name, an arrow (`->`), the name of the exported shell, and a body in braces:

```relux
effect StartService -> svc {
    shell svc {
        > echo "service ready"
        <? ^service ready$
        match_prompt()
    }
}
```

The name must be CamelCase — this is how Relux distinguishes effects from functions, which are always `snake_case`. The `-> svc` declares that this effect **exports** a shell called `svc`. The shell block inside the body runs the setup: whatever commands are needed to get the service into a ready state.

The exported shell is the bridge between the effect and the test. When a test needs this effect, it gets access to the `svc` shell — the same PTY session, in the same state it was left after setup. Environment [variables](06-variables.md) set during setup, working directory changes, running processes — all persist into the test.

An effect body can contain multiple shell blocks, `let` declarations, and `need` statements. The shell blocks execute in order, and the shell named in the effect declaration head is the one exported to tests.

## Needing an effect

A test declares its infrastructure requirements with the `need` keyword:

```relux
test "effect sets up shell before test runs" {
    need StartService as worker
    shell worker {
        > echo "test using effect shell"
        <? ^test using effect shell$
    }
}
```

The `need StartService as worker` does two things: it ensures the `StartService` effect runs before the test body, and it makes the effect's exported shell available under the alias `worker`. Inside the test, `shell worker` refers to the same PTY session that the effect's `shell svc` block used during setup — with all the state from setup intact.

The `as` alias lets you choose the name for the shell within your test. The effect exports `svc`, but the test calls it `worker` — the names do not need to match. This is useful when you need multiple effects that happen to export shells with the same name:

```relux
effect ServiceA -> svc {
    shell svc {
        > export SVC_ID=A
        match_ok()
        > echo "service A ready"
        <? ^service A ready$
    }
}

effect ServiceB -> svc {
    shell svc {
        > export SVC_ID=B
        match_ok()
        > echo "service B ready"
        <? ^service B ready$
    }
}

test "two effects with same export name both accessible via alias" {
    need ServiceA as a
    need ServiceB as b
    shell a {
        > echo $$SVC_ID
        <? ^A$
    }
    shell b {
        > echo $$SVC_ID
        <? ^B$
    }
}
```

Both effects export a shell called `svc`, but the test accesses them as `a` and `b`. Without aliases, there would be a name collision.

## Bare `need`

Sometimes you need an effect for its side effects — creating files, setting up external state, or just running the service in background — but do not need access to its shell. Use `need` without `as`:

```relux
effect SideEffectOnly -> svc {
    shell svc {
        > touch /tmp/side-effect-marker
        match_ok()
    }
}

test "bare need runs effect but does not expose shell" {
    need SideEffectOnly
    shell s {
        > test -f /tmp/side-effect-marker && echo "effect ran"
        <? ^effect ran$
    }
}
```

The effect runs — the file gets created — but the test cannot access the effect's shell. `shell s` creates a fresh local shell, not the effect's `svc`. Use bare `need` when you care about what the effect *does*, not the shell it leaves behind.

## Dependencies between effects

Effects can depend on other effects using `need` inside the effect body. This lets you build layered infrastructure where each piece builds on what came before:

```relux
effect SetupDb -> db {
    shell db {
        > export DB_STATUS=started
        match_ok()
    }
}

effect MigrateDb -> db {
    need SetupDb as db
    shell db {
        > export MIG_STATUS=applied
        match_ok()
    }
}

effect SeedData -> db {
    need MigrateDb as db
    shell db {
        > export SEED_STATUS=seeded
        match_ok()
    }
}
```

This creates a dependency chain: `SeedData` needs `MigrateDb`, which needs `SetupDb`. When a test needs `SeedData`, Relux resolves the full chain and executes in **topological order** — dependencies first:

1. `SetupDb` runs, exports its `db` shell
2. `MigrateDb` runs in that same shell (via `need SetupDb as db`), adds migration state
3. `SeedData` runs in the same shell again, adds seed data

```relux
test "transitive dependencies execute in order" {
    need SeedData as db
    shell db {
        > echo $$DB_STATUS
        <? ^started$
        > echo $$MIG_STATUS
        <? ^applied$
        > echo $$SEED_STATUS
        <? ^seeded$
    }
}
```

The test only says `need SeedData` — it does not need to know about `SetupDb` or `MigrateDb`. Relux resolves the transitive dependencies automatically. All three environment variables are present because all three effects ran, in order, on the same shell.

Circular dependencies are caught at check time. If effect A needs B and B needs A, `relux check` reports the cycle before any test runs.

## Effect identity and deduplication

What happens when two tests — or two `need` statements in the same test — request the same effect? Relux does not run it twice. It identifies each effect instance by its **identity** and deduplicates: if the identity matches, the effect runs once and all references share the same instance.

For effects without overlay variables (covered in the next section), the identity is simply the effect name. Two `need` statements for the same effect share one instance — the effect runs once, and both aliases point to the same shell:

```relux
effect Counter -> ctr {
    shell ctr {
        > export COUNT=0
        match_ok()
        > COUNT=$$(($$COUNT + 1)) && echo $$COUNT
        <? ^1$
    }
}

test "same effect needed twice shares one instance" {
    need Counter as c1
    need Counter as c2
    shell c1 {
        > echo $$COUNT
        <? ^1$
    }
    shell c2 {
        > echo $$COUNT
        <? ^1$
    }
}
```

Both `c1` and `c2` are aliases for the **same** shell. The `Counter` effect ran once — the counter was incremented to 1. If it had run twice, the count would be 2.

## Overlay variables

So far, every effect has been a fixed recipe — the same setup every time. But what if you need two databases with different names, or the same service on different ports? The `FailTail` example in the introduction hinted at the answer: the `{ FAILTAIL_TRIGGER = ... }` syntax passed configuration into the effect. These are **overlay variables** — key-value pairs passed at the `need` site that parameterize the effect:

```relux
effect Parameterized -> svc {
    shell svc {
        > export SVC_LABEL=${LABEL}
        match_ok()
    }
}

test "different overlays create separate instances" {
    need Parameterized as a {
        LABEL = "alpha"
    }
    need Parameterized as b {
        LABEL = "beta"
    }
    shell a {
        > echo $$SVC_LABEL
        <? ^alpha$
    }
    shell b {
        > echo $$SVC_LABEL
        <? ^beta$
    }
}
```

The `Parameterized` effect references `${LABEL}` — a variable that comes from the overlay, not from a `let` declaration. Each `need` site provides its own value for `LABEL`, and Relux creates **separate instances** of the effect — one with `LABEL = "alpha"`, another with `LABEL = "beta"`. Each instance gets its own shell, its own setup run, its own state.

This is where overlays connect to deduplication. The full identity of an effect instance is **(effect name, overlay values)**. Same name with same overlay = shared instance. Same name with different overlay = separate instances. Two `need Parameterized as x { LABEL = "alpha" }` with the same overlay value would share one instance, regardless of the alias name.

Overlay variables are the mechanism for reusing a single effect definition across different configurations — like the `FailTail` example from the introduction, where the trigger pattern and log path are passed in as overlays.

## Best practices

### Set fail patterns early in effects

Effects that start long-running services should set a fail pattern before the startup command, just like in a regular shell block. This maximizes coverage — any crash output during startup or during the test body triggers an immediate failure:

```relux
effect StartService -> svc {
    shell svc {
        !? FATAL|ERROR|panic
        > start-my-service --foreground
        <? listening on port 8080
    }
}
```

The fail pattern is active from the first line. If the service crashes during startup, the fail pattern catches it before the readiness match even runs.

### Overlay isolation

A child effect's shell does **not** inherit environment variables from a parent effect's shell. Each effect's shell starts fresh:

```relux
effect Parent -> p {
    shell p {
        > export PARENT_VAR=from_parent
        match_ok()
    }
}

effect Child -> c {
    need Parent as p
    shell c {
        > echo "parent_var='$$PARENT_VAR'"
        <? ^parent_var=''$
    }
}
```

The `Child` effect needs `Parent`, but `Child`'s own `shell c` does not see `PARENT_VAR`. If you need a value from the parent, pass the value through an overlay variable.

### Deduplication and shared state

Because deduplication means two aliases can point to the same shell, mutations through one alias are visible through the other. This is by design — it is how effects like the database chain work, where each layer builds on the state left by the previous one. But it means you should be aware: if two unrelated parts of a test both alias the same effect instance, they share a single PTY session. Commands sent through one alias affect the shell the other alias sees.

If you need truly independent instances, give them different overlay values — even a dummy key is enough to create separate identities:

```relux
need MyEffect as a { INSTANCE = "1" }
need MyEffect as b { INSTANCE = "2" }
```

## Try it yourself

Write a two-effect dependency chain that simulates a database setup:

1. Define an effect `StartDb` that exports a shell `db`, sets an environment variable `DB_STATUS=running`, and echoes a readiness message
2. Define an effect `Migrate` that needs `StartDb`, runs in the same shell, and sets `MIG_STATUS=done`
3. Write a test that needs `Migrate` and verifies both variables are present
4. As a bonus: use overlay variables to create two database instances with different `DB_NAME` values, and verify each instance has its own name

---

Next: [Cleanup](12-cleanup.md) — teardown blocks for effects and tests
