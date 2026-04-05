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
effect Db {
    expose service

    shell service {
        > start-db --data-dir /tmp/test-db
        <? listening on port 5432
        match_prompt()
    }
}

effect MigratedDb {
    start Db as db
    expose db.service as service

    shell migrations {
        > migrate --db localhost:5432
        <? migrations complete
        match_prompt()
    }
}

test "user signup" {
    start MigratedDb
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
    start MigratedDb
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

The infrastructure is defined once. Each test says `start MigratedDb` — Relux figures out that `MigratedDb` depends on `Db`, starts both in order, and hands the test a shell with a fully migrated database.

A particularly common pattern is monitoring: tail a log file with a fail pattern so any crash in the background aborts the test immediately. This combination is useful enough to deserve its own alias — call it a fail tail:

```relux
effect FailTail {
    expect FAILTAIL_TRIGGER, FAILTAIL_LOG
    expose tail

    shell tail {
        !? ${FAILTAIL_TRIGGER}
        > tail -f ${FAILTAIL_LOG}
    }
}

test "service handles load" {
    start FailTail {
        FAILTAIL_TRIGGER = "panic|error"
        FAILTAIL_LOG = "/var/log/service.log"
    }
    start Service as svc
    shell client {
        > curl http://localhost:8080/heavy-endpoint
        <? 200 OK
        match_prompt()
    }
}
```

The `FailTail` effect declares two required variables with `expect` and exposes its `tail` shell. It starts tailing the log and sets a fail pattern. If anything fatal appears in the log while the test runs its requests, the test fails on the spot. The `{ FAILTAIL_TRIGGER = ... }` syntax passes configuration into the effect — we will cover these **overlay variables** later in this article. Without effects, you would duplicate this tail-and-fail-pattern setup in every test that exercises the service.

## Defining an effect

An effect definition starts with the `effect` keyword, followed by a CamelCase name and a body in braces. Inside, `expose` declares which shells are part of the effect's public interface:

```relux
effect Service {
    expose service

    shell service {
        > echo "service ready"
        <? ^service ready$
        match_prompt()
    }
}
```

The name must be CamelCase — this is how Relux distinguishes effects from functions, which are always `snake_case`. The `expose service` declaration means the `service` shell is available to whoever starts this effect. The shell block inside the body runs the setup: whatever commands are needed to get the service into a ready state.

The exposed shell is the bridge between the effect and the test. When a test starts this effect with an alias, it can access the `service` shell via dot-access — the same PTY session, in the same state it was left after setup. Environment [variables](06-variables.md) set during setup, working directory changes, running processes — all persist into the test.

An effect body can contain `expect` declarations, `expose` declarations, `let` declarations, `start` statements, shell blocks, and a cleanup block. The shell blocks execute in order, and the shells named in `expose` declarations are accessible to callers. Shells that are *not* exposed are internal — they run during setup and are terminated when setup completes. Only exposed shells survive into the test body.

## Starting an effect

A test declares its infrastructure requirements with the `start` keyword:

```relux
test "effect sets up shell before test runs" {
    start Service as svc
    svc.service {
        > echo "test using effect shell"
        <? ^test using effect shell$
    }
}
```

The `start Service as svc` does two things: it ensures the `Service` effect runs before the test body, and it makes the effect's exposed shells available under the alias `svc`. Inside the test, `svc.service { ... }` accesses the shell that the effect exposed — the same PTY session that the effect's `shell service` block used during setup, with all the state from setup intact.

The `as` alias names the effect instance within your test. You access its exposed shells via dot-access: `alias.shell_name { ... }`. This is useful when you start multiple effects:

```relux
effect ServiceA {
    expose service

    shell service {
        > export SVC_ID=A
        match_ok()
        > echo "service A ready"
        <? ^service A ready$
    }
}

effect ServiceB {
    expose service

    shell service {
        > export SVC_ID=B
        match_ok()
        > echo "service B ready"
        <? ^service B ready$
    }
}

test "two effects both accessible via alias" {
    start ServiceA as a
    start ServiceB as b
    a.service {
        > echo $$SVC_ID
        <? ^A$
    }
    b.service {
        > echo $$SVC_ID
        <? ^B$
    }
}
```

Both effects expose a shell called `service`, but the test accesses them through different aliases — `a.service` and `b.service`. The alias disambiguates which effect instance you mean.

## Bare `start`

Sometimes you need an effect for its side effects — creating files, setting up external state, or just running the service in the background — but do not need access to its shells. Use `start` without `as`:

```relux
effect Scaffold {
    expose setup

    shell setup {
        > touch /tmp/side-effect-marker
        match_ok()
    }
}

test "bare start runs effect but does not expose shell" {
    start Scaffold
    shell s {
        > test -f /tmp/side-effect-marker && echo "effect ran"
        <? ^effect ran$
    }
}
```

The effect runs — the file gets created — but the test cannot access the effect's shell because there is no alias to qualify with. `shell s` creates a fresh local shell. Use bare `start` when you care about what the effect *does*, not the shells it leaves behind.

## Dependencies between effects

Effects can depend on other effects using `start` inside the effect body. This lets you build layered infrastructure where each piece builds on what came before:

```relux
effect Db {
    expose service

    shell service {
        > export DB_STATUS=started
        match_ok()
    }
}

effect MigratedDb {
    start Db as db
    expose db.service as service

    shell service {
        > export MIG_STATUS=applied
        match_ok()
    }
}

effect SeededDb {
    start MigratedDb as db
    expose db.service as service

    shell service {
        > export SEED_STATUS=seeded
        match_ok()
    }
}
```

This creates a dependency chain: `SeededDb` starts `MigratedDb`, which starts `Db`. When a test starts `SeededDb`, Relux resolves the full chain and executes in **topological order** — dependencies first:

1. `Db` runs, exposes its `service` shell
2. `MigratedDb` runs in that same shell (via `start Db as db`), adds migration state
3. `SeededDb` runs in the same shell again, adds seed data

Each effect re-exposes the `service` shell from its dependency using the qualified expose syntax `expose db.service as service`. This means whoever starts `SeededDb` can access the same shell that was built up through the entire chain.

```relux
test "transitive dependencies execute in order" {
    start SeededDb as db
    db.service {
        > echo $$DB_STATUS
        <? ^started$
        > echo $$MIG_STATUS
        <? ^applied$
        > echo $$SEED_STATUS
        <? ^seeded$
    }
}
```

The test only says `start SeededDb` — it does not need to know about `Db` or `MigratedDb`. Relux resolves the transitive dependencies automatically. All three environment variables are present because all three effects ran, in order, on the same shell.

Circular dependencies are caught at check time. If effect A needs B and B needs A, `relux check` reports the cycle before any test runs.

## Effect identity and deduplication

What happens when two tests — or two `start` statements in the same test — request the same effect? Relux does not run it twice. It identifies each effect instance by its **identity** and deduplicates: if the identity matches, the effect runs once and all references share the same instance.

For effects without overlay variables (covered in the next section), the identity is simply the effect name. Two `start` statements for the same effect share one instance — the effect runs once, and both aliases point to the same shell:

```relux
effect Counter {
    expose counter

    shell counter {
        > export COUNT=0
        match_ok()
        > COUNT=$$(($$COUNT + 1)) && echo $$COUNT
        <? ^1$
    }
}

test "same effect started twice shares one instance" {
    start Counter as c1
    start Counter as c2
    c1.counter {
        > echo $$COUNT
        <? ^1$
    }
    c2.counter {
        > echo $$COUNT
        <? ^1$
    }
}
```

Both `c1` and `c2` are aliases for the **same** effect instance. The `Counter` effect ran once — the counter was incremented to 1. If it had run twice, the count would be 2.

## Overlay variables

So far, every effect has been a fixed recipe — the same setup every time. But what if you need two databases with different names, or the same service on different ports? The `FailTail` example in the introduction hinted at the answer: `expect` declares what the effect requires, and the `{ FAILTAIL_TRIGGER = ... }` syntax at the `start` site provides it. These are **overlay variables** — key-value pairs passed at the `start` site that parameterize the effect:

```relux
effect Labeled {
    expect LABEL
    expose service

    shell service {
        > export SVC_LABEL=${LABEL}
        match_ok()
    }
}

test "different overlays create separate instances" {
    start Labeled as a {
        LABEL = "alpha"
    }
    start Labeled as b {
        LABEL = "beta"
    }
    a.service {
        > echo $$SVC_LABEL
        <? ^alpha$
    }
    b.service {
        > echo $$SVC_LABEL
        <? ^beta$
    }
}
```

The `Labeled` effect declares `expect LABEL` — a required variable that must be provided by the caller. Each `start` site provides its own value for `LABEL`, and Relux creates **separate instances** of the effect — one with `LABEL = "alpha"`, another with `LABEL = "beta"`. Each instance gets its own shell, its own setup run, its own state. If a caller forgets to pass a required variable, `relux check` reports the error before any test runs.

`expect` is a **contract**, not a sandbox. It declares which variables the effect *requires* — the ones the resolver validates. It does not prevent the effect from reading other variables. An effect always inherits the full parent environment: the base system environment, plus any variables set in the caller's scope. The overlay adds to or overrides specific entries in that inherited environment. This means most configuration flows through naturally, and only the values that vary per-instance need to be listed in `expect` and passed via overlays.

This is where overlays connect to deduplication. The full identity of an effect instance is **(effect name, evaluated overlay values)**. Same name with same overlay = shared instance. Same name with different overlay = separate instances. Two `start Labeled as x { LABEL = "alpha" }` with the same overlay value would share one instance, regardless of the alias name.

When the overlay key and the variable being passed have the same name, you can use the shorthand syntax — a bare key without `= value`:

```relux
let LABEL = "alpha"
start Labeled as a { LABEL }   // desugars to LABEL = LABEL
```

Overlay variables are the mechanism for reusing a single effect definition across different configurations — like the `FailTail` example from the introduction, where the trigger pattern and log path are passed in as overlays.

## Section ordering

The parser enforces a fixed ordering of sections inside an effect body:

1. `expect` — required overlay variables
2. `let` — local bindings (can reference expected vars)
3. `start` — sub-dependencies (overlay expressions can reference let-bound vars)
4. `expose` — which shells are visible to callers
5. `shell` blocks — setup logic
6. `cleanup` — teardown (optional, at most one)

Each section is optional, but they must appear in this order. Writing a `start` before a `let`, or an `expose` before a `start`, is a parse error. Comments and blank lines are allowed anywhere between sections.

This ordering reflects the data flow: expects declare what is available, lets compute derived values, starts wire those values into sub-dependencies, and exposes declare the public interface after all shells and dependencies are established.

## Best practices

### Set fail patterns early in effects

Effects that start long-running services should set a fail pattern before the startup command, just like in a regular shell block. This maximizes coverage — any crash output during startup or during the test body triggers an immediate failure:

```relux
effect Service {
    expose service

    shell service {
        !? FATAL|ERROR|panic
        > start-my-service --foreground
        <? listening on port 8080
    }
}
```

The fail pattern is active from the first line. If the service crashes during startup, the fail pattern catches it before the readiness match even runs.

### Deduplication and shared state

Because deduplication means two aliases can point to the same shell, mutations through one alias are visible through the other. This is by design — it is how effects like the database chain work, where each layer builds on the state left by the previous one. But it means you should be aware: if two unrelated parts of a test both alias the same effect instance, they share a single PTY session. Commands sent through one alias affect the shell the other alias sees.

If you need truly independent instances, give them different overlay values — even a dummy key is enough to create separate identities:

```relux
start MyEffect as a { INSTANCE = "1" }
start MyEffect as b { INSTANCE = "2" }
```

## Try it yourself

Write a two-effect dependency chain that simulates a database setup:

1. Define an effect `Db` that exposes a shell `service`, sets an environment variable `DB_STATUS=running`, and echoes a readiness message
2. Define an effect `MigratedDb` that starts `Db`, re-exposes its shell, and sets `MIG_STATUS=done`
3. Write a test that starts `MigratedDb` and verifies both variables are present via dot-access
4. As a bonus: use overlay variables to create two database instances with different `DB_NAME` values, and verify each instance has its own name

---

Next: [Pure Functions](12-pure-functions.md) — functions that compute values without touching a shell
