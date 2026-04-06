# R008: Effects Rework

- **Status**: implemented
- **Created**: 2026-04-03
- **Supersedes**: R007

## Motivation

The current effects system has several design problems that compound as test suites grow in complexity.

### Opaque interfaces

An effect's contract is invisible from its signature. The current `effect StartDb -> db` tells you the name and the exported shell, but nothing about what configuration the effect requires. The only way to discover which environment variables must be set is to read through the effect body. This became apparent when presenting the framework to new users — they consistently struggled to understand what an effect expects.

For example, the current `StartDb` effect:

```relux
effect StartDb -> db {
    shell db {
        > start-db --port ${DB_PORT} --data-dir ${DB_DATA_DIR}
        <? listening on ${DB_PORT}
    }
}
```

Nothing in the signature tells the caller that `DB_PORT` and `DB_DATA_DIR` must be set.

### Single-shell export

An effect can only export one shell via `-> shell_name`. This works for leaf effects that start a single service, but breaks down for composed effects. Consider a `Cluster` effect that starts three database nodes — the caller needs access to all three shells, but the effect can only hand back one.

### Shell name collisions

If two effects export shells with the same name and a test needs both, there is no namespace separation. The current aliasing (`need E as alias`) renames the single exported shell, but doesn't help when an effect internally needs to expose multiple shells.

### Overlay opacity

The overlay syntax `need StartDb { DB_PORT = db_port }` served as both parameterization and env remapping, but the effect didn't declare which keys it expected. The overlay was a bag of key-value pairs with no contract — the caller guessed, and mistakes were only caught at runtime (or not at all, if the service silently used a default).

### Broken identity (R007)

Effect instance identity is based on the AST canonical form of overlay expressions. This means the same runtime value reached through different variable names produces different identities, breaking deduplication in pass-through scenarios. R007 proposed switching to evaluated-value-based identity, but that introduces a determinism problem: if overlay values come from environment variables, the same test suite can produce different effect topologies depending on the environment it runs in. With no conditional operations in the DSL, this is an unexpected source of non-determinism.

## Design

### Effect contract

An effect has three explicit interface components:

1. **`expect`** — required environment variables the effect reads
2. **`expose`** — shells the effect makes available to callers
3. **`start`** — dependency effects with env remapping

```relux
effect Node {
    expect NODE_PORT, NODE_NAME, NODE_DATA_DIR
    start DependencyEffect
    expose node

    shell node {
        > start-node --port ${NODE_PORT} --name ${NODE_NAME} --data-dir ${NODE_DATA_DIR}
        <? ready
    }
}
```

`expect` declares the effect's required inputs. These must be present in the effect's environment when it starts — either inherited from the parent environment or provided via an overlay in the `start` declaration. The resolver validates that all `expect` variables are satisfiable by walking the effect graph top-down from each test.

`start` declares dependency effects (see below).

`expose` declares which shells are part of the effect's public interface. Internal shells not listed in `expose` are implementation details — they are started, used during setup, and terminated when setup completes.

Note that none of these are mandatory: an effect may not need any environment variables, may not have any dependencies, and may not expose any shells. Such an effect would be called purely for its side effects.

### Env inheritance

Effects inherit the full parent environment. This is a change from the current behavior where effects only see the base environment plus their explicit overlay.

The rationale: real services have dozens of configuration parameters. Listing every one in `expect` and threading every one through overlays would be impractical. The common case is that most configuration comes from the environment and only a few values need to be overridden per-instance.

At the same time, some particular test might need a dependency started with test-specific configuration that is not and should not be listed in the effect interface. `expect` declares the variables that the effect *requires* — the ones that must be set and that the resolver should validate. It does not prevent the effect from reading other environment variables; it is a contract, not a sandbox.

### Overlay as env remapping

The overlay in a `start` declaration remaps the caller's environment into the dependency's environment:

```relux
start Node as n1 {
    NODE_PORT = PORT1
    NODE_NAME = "node1"
    NODE_DATA_DIR = "${ARTIFACTS}/n1"
}
```

Each entry `KEY = expr` means: "in the dependency's environment, set `KEY` to the evaluated value of `expr`." The expression is evaluated in the caller's scope. Entries can reference caller variables, caller env vars, literals, and pure function calls.

The shorthand form `KEY` (without `= expr`) binds the variable to the same-named variable in the caller's scope, equivalent to `KEY = KEY`:

```relux
start Node as n1 {
    NODE_PORT    // equivalent to: NODE_PORT = NODE_PORT
    NODE_NAME = "node1"
    NODE_DATA_DIR = "${ARTIFACTS}/n1"
}
```

### Dot-access for exposed shells

Effects are accessed through aliases, and their exposed shells are accessed via dot notation:

```relux
test "node health" {
    let port = available_port()

    start Node as n {
        NODE_PORT = port
        NODE_NAME = "test-node"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/node"
    }

    shell n.node {
        > health
        <? ok
    }
}
```

For composed effects, dot-access chains through the alias path:

```relux
effect Cluster {
    expect PORT1, PORT2, PORT3

    start Node as n1 {
        NODE_PORT = PORT1
        NODE_NAME = "node1"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n1"
    }
    start Node as n2 {
        NODE_PORT = PORT2
        NODE_NAME = "node2"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n2"
    }
    start Node as n3 {
        NODE_PORT = PORT3
        NODE_NAME = "node3"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n3"
    }

    expose n1.node as primary
    expose n2.node as secondary
    expose n3.node as arbiter
}

test "cluster ops" {
    let p1 = available_port()
    let p2 = available_port()
    let p3 = available_port()

    start Cluster as c {
        PORT1 = p1
        PORT2 = p2
        PORT3 = p3
    }

    shell c.primary {
        > rs.status()
        <? ok
    }
}
```

`expose n1.node as primary` means: "take the `node` shell exposed by the `n1` dependency and re-expose it as `primary`." The test accesses it as `shell c.primary { ... }`.

Effects are opaque by default. Internal shells and nested dependency shells that are not listed in `expose` are not accessible to callers.

### Effect identity

Effect instance identity is **(effect name, evaluated overlay)**.

The overlay is evaluated at effect setup time. All overlay expressions are pure, so evaluation is deterministic within a given scope. Two `start` declarations that produce the same effect name and the same evaluated overlay values share a single instance.

This fixes the pass-through problem from R007: a test's `start Node { NODE_PORT = port }` and a Cluster's internal `start Node { NODE_PORT = PORT1 }` both evaluate to the same port value, producing the same identity — correct deduplication.

### Accidental-collision warning

Within a single scope (test or effect body), the runtime detects when two `start` declarations target the same effect with different overlay expressions that happen to evaluate to the same values. This suggests the user intended separate instances but the runtime values accidentally collide.

```
warning: different overlay expressions evaluate to the same values — these
         will share a single Node instance
   ┌─ tests/app.relux:5:20
   │
 5 │     start Node as n1 { NODE_PORT = FOO }
   │                                    ^^^ evaluated to: 9000
 6 │     start Node as n2 { NODE_PORT = BAR }
   │                                    ^^^ evaluated to: 9000
   │
   = help: if you intended separate instances, use distinct values
```

The check compares canonical (AST-based) overlay representations within the same scope. If two `start` declarations have different canonical forms but identical evaluated forms, a warning is emitted. Cross-scope deduplication (a test's `start` matching an effect's internal `start`) is intentional and never triggers a warning.

### Start ordering via runtime instrumentation

The effect dependency graph's start ordering is derived from runtime execution, not from the IR. The runtime already walks the graph in topological order via recursive `acquire` calls. By emitting structured events during `bootstrap_effect`, the start sequence is captured naturally.

Effects that start concurrently share the same phase number (the listing below is illustrative and subject to change):

```
phase 1: Node (NODE_PORT=9000), Node (NODE_PORT=9001)
phase 2: Cluster (PORT1=9000, PORT2=9001)
```

Deduplicated instances that are reused by a later `acquire` emit a reuse event instead of a start event, making sharing visible in the log.

This approach requires no IR changes — the ordering is a runtime observation, not a static computation.

### Resolver validation

The resolver validates the effect graph at check time (`relux check`):

- Every `expect` variable declared by an effect must be satisfiable: either present in the base environment or provided by an overlay in the `start` declaration.
- Every `expose` reference must point to a shell that exists in the effect body or is itself exposed by a dependency alias.
- Circular dependencies remain a parse error.
- Overlays can only reference variables in the caller's scope (existing behavior).

Validation walks the effect graph top-down from each test, tracking which variables are available at each level. A missing `expect` variable produces an error pointing at both the `start` site (where the overlay should provide it) and the `expect` declaration (where the requirement is defined).

## Migration

### Syntax changes

| Current                                          | New                                                            |
|--------------------------------------------------|----------------------------------------------------------------|
| `effect E -> shell { ... }`                      | `effect E { expect ...; expose shell; ... }`                   |
| `need E { KEY = val }`                           | `start E { KEY = val }` (shorthand syntax added)               |
| `need E as alias` (shell access via `alias { }`) | `start E as alias` (shell access via `shell alias.shell { }`)  |

### Behavioral changes

- Effects inherit the parent environment (currently they only see base env + overlay).
- Effects are opaque — only `expose`d shells are accessible (currently the single `-> shell` is always accessible).
- Effect identity switches from canonical (AST-based) to evaluated overlay.

## Examples

### Leaf effect

```relux
effect StartDb {
    expect DB_PORT
    expose db

    shell db {
        let db_root = "${__RELUX_TEST_ARTIFACTS}/database"

        > mkdir ${db_root}
        match_ok()

        !? ^error:

        > start-db --port ${DB_PORT} --data-dir ${db_root}
        <~10s? ^listening on ${DB_PORT}$
    }
}
```

### Composed effect with pass-through

```relux
effect StartAuth {
    expect DB_PORT, AUTH_PORT
    expose auth
    expose db.db as db

    start StartDb as db // DB_PORT is inherited from local environment

    shell auth {
        !? ^error:

        > start-auth --db-port ${DB_PORT} --port ${AUTH_PORT}
        <~10s? ^listening on ${AUTH_PORT}$
    }
}
```

The test's `start StartDb { DB_PORT = port }` and StartAuth's internal `start StartDb as db` both evaluate to the same port — one shared StartDb instance.

### Multi-instance effect

```relux
effect Cluster {
    expect PORT1, PORT2, PORT3
    expose n1.node as primary
    expose n2.node as secondary
    expose n3.node as arbiter

    start Node as n1 {
        NODE_PORT = PORT1
        NODE_NAME = "node1"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n1"
    }
    start Node as n2 {
        NODE_PORT = PORT2
        NODE_NAME = "node2"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n2"
    }
    start Node as n3 {
        NODE_PORT = PORT3
        NODE_NAME = "node3"
        NODE_DATA_DIR = "${__RELUX_TEST_ARTIFACTS}/n3"
    }
}

test "cluster failover" {
    let p1 = available_port()
    let p2 = available_port()
    let p3 = available_port()

    start Cluster as c {
        PORT1 = p1
        PORT2 = p2
        PORT3 = p3
    }

    shell c.primary {
        > rs.stepDown()
        <? ok
    }

    shell c.secondary {
        > rs.status()
        <? primary
    }
}
```
