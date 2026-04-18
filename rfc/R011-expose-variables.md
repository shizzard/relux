# R011: Expose Variables and Naming Conventions

- **Status**: draft
- **Created**: 2026-04-18
- **Depends on**: R008

## Abstract

Extend effect `expose` declarations to support variables in addition to shells. Introduce enforced naming conventions across the DSL — SCREAMING_SNAKE_CASE for all variables, CamelCase for effects and effect aliases, snake_case for functions and shells — which allows the parser to unambiguously determine whether an `expose` target is a shell or a variable by its casing.

## Motivation

### Passing computed values from effects to tests

Effects often compute values during setup that tests need: generated ports, resource IDs, temporary paths, session tokens. Currently, the only way to pass such data from an effect to a test is to export the value as an environment variable in the exposed shell, then pull it into a test-level variable:

```relux
effect CreateBucket {
    expose service

    shell service {
        > create-bucket --random-name
        <? bucket created: (.+)$
        > export BUCKET_NAME=${1}
        match_ok()
    }
}

test "use bucket" {
    start CreateBucket as Bucket
    
    let BUCKET_NAME

    shell Bucket.service {
        # have to extract the bucket name from the shell that created it
        > echo ${BUCKET_NAME}
        <= echo
        <? ^(.*)$
        BUCKET_NAME = $1
    }

    shell client {
        > upload --bucket ${BUCKET_NAME} ...
    }
}
```

This is fragile: the effect must export the value into the shell's environment, and the test must switch into that shell, echo it out, capture it with a regex, and assign it to a local variable — all before the value can be used. And the value is only available after that extraction sequence, not from the moment the effect is started. What we want is for the effect to declare certain computed values as part of its public interface, just as it declares shells.

### Ambiguous naming conventions

The DSL currently enforces CamelCase for effects and snake_case for functions, but variables accept any casing — `my_var`, `MyVar`, and `MY_VAR` are all valid. This permissiveness means that `expose something` cannot be parsed unambiguously: is `something` a shell or a variable?

Establishing a strict three-way naming convention solves this and brings consistency to the language.

### Effect alias casing

Effect aliases (the name after `as` in `start Effect as alias`) currently use the permissive variable identifier rules. Since an alias names an effect instance — not a shell, function, or variable — it should follow the same convention as effects: CamelCase.

## Proposal

### Enforced naming conventions

All identifiers in the DSL must follow one of three naming conventions:

| Convention | Applies to | Examples |
|---|---|---|
| CamelCase | Effects, effect aliases | `Db`, `StartAuth`, `start Node as Primary` |
| snake_case | Functions, shells | `curl()`, `http_request()`, `shell service` |
| SCREAMING_SNAKE_CASE | Variables (`let`, `expect`, function parameters) | `let PORT`, `expect DB_HOST`, `fn curl(URL, METHOD)` |

This is a breaking change. All existing `let` bindings, function parameters, and `expect` declarations must be updated to SCREAMING_SNAKE_CASE. All effect aliases must be updated to CamelCase.

A side benefit: SCREAMING_SNAKE_CASE for variables places them in the same visual group as environment variables. Since environment variables form the bottom layer of the variable scope (the `LayeredEnv` chain), this makes the relationship transparent — `${DB_PORT}` looks the same whether it comes from a `let` binding, an `expect` declaration, or the process environment. The environment is a natural, invisible base layer of the scope rather than a separate naming world.

A trade-off: because `let` bindings and environment variables now share the same casing, the resolver cannot distinguish a typo in a variable name from an intentional read of a process environment variable. A static "undeclared variable" lint becomes impossible. However, this should still be a runtime warning — if a variable reference resolves to neither a `let` binding, an `expect` declaration, a function parameter, nor a set environment variable, the runtime can warn that the interpolation resolved to an empty string.

### Exposing variables from effects

The `expose` declaration is extended to support variables. The parser determines the type of the expose target by its casing:

- **snake_case** target = shell
- **SCREAMING_SNAKE_CASE** target = variable

```relux
effect CreateBucket {
    expect REGION
    let BUCKET_ID = uuid()
    expose service
    expose BUCKET_ID

    shell service {
        > create-bucket --id ${BUCKET_ID} --region ${REGION}
        <? bucket ready
    }
}
```

`expose service` exposes the shell (snake_case). `expose BUCKET_ID` exposes the `let`-bound variable (SCREAMING_SNAKE_CASE). No additional syntax or keywords are needed — casing is the disambiguator.

### Re-exposing from dependencies

Both shells and variables can be re-exposed from dependencies using the existing dot-access and `as` syntax:

```relux
effect FullStack {
    start CreateBucket as Storage
    start StartApi as Api

    expose Api.service as api
    expose Storage.service as storage
    expose Storage.BUCKET_ID as BUCKET_ID
}
```

The casing of the target after the dot determines the type: `Storage.service` (snake_case) is a shell, `Storage.BUCKET_ID` (SCREAMING) is a variable. The `as` alias must match the same convention as the target — shell aliases are snake_case, variable aliases are SCREAMING_SNAKE_CASE.

### Accessing exposed variables from tests

Tests access exposed variables through the effect alias using dot-notation in interpolation:

```relux
test "upload to bucket" {
    start FullStack as Stack

    shell Stack.api {
        > upload --bucket ${Stack.BUCKET_ID} file.txt
        <= uploaded
    }
}
```

`${Stack.BUCKET_ID}` reads the variable exposed by the `FullStack` effect instance aliased as `Stack`. The same dot-access syntax works in any interpolation context — send, match, let bindings:

```relux
    let MY_BUCKET = Stack.BUCKET_ID
```

### Immutability

Exposed variables are read-only from the caller's perspective. A test or parent effect can read `${Alias.VAR}` but cannot assign to it. The value was computed during effect setup and is fixed for the lifetime of the effect instance.

## Examples

### Leaf effect exposing a variable

```relux
effect StartDb {
    expect DB_NAME
    let PORT = available_port()
    expose db
    expose PORT

    shell db {
        > start-db --port ${PORT} --name ${DB_NAME}
        <~10s? listening on ${PORT}
    }
}

test "db health check" {
    start StartDb as Db {
        DB_NAME = "test_db"
    }

    shell Db.db {
        > health
        <? ok
    }

    shell client {
        > curl http://localhost:${Db.PORT}/status
        <? running
    }
}
```

### Composed effect re-exposing variables

```relux
effect StartAuth {
    let AUTH_PORT = available_port()
    start StartDb as Db {
        DB_NAME = "auth"
    }
    expose auth
    expose AUTH_PORT
    expose Db.PORT as DB_PORT

    shell auth {
        > start-auth --port ${AUTH_PORT} --db-port ${Db.PORT}
        <~10s? listening on ${AUTH_PORT}
    }
}

test "auth uses correct db" {
    start StartAuth as Auth

    shell client {
        > curl http://localhost:${Auth.AUTH_PORT}/config
        <? db_port: ${Auth.DB_PORT}
    }
}
```

### Function parameters in SCREAMING_SNAKE_CASE

```relux
fn http_request(EXPECTED_CODE, URL, METHOD) {
    > curl -s -o /tmp/response.json -w "%{http_code}" -X ${METHOD} ${URL}
    <? ^${EXPECTED_CODE}$
}

pure fn url(PATH) {
    "http://localhost:9000${PATH}"
}
```

### Effect alias as CamelCase

```relux
effect Cluster {
    expect PORT_PRIMARY, PORT_SECONDARY

    start Node as Primary {
        NODE_PORT = PORT_PRIMARY
        NODE_NAME = "primary"
    }
    start Node as Secondary {
        NODE_PORT = PORT_SECONDARY
        NODE_NAME = "secondary"
    }

    expose Primary.node as primary
    expose Secondary.node as secondary
    expose Primary.NODE_ID as PRIMARY_ID
    expose Secondary.NODE_ID as SECONDARY_ID
}

test "cluster status" {
    let P1 = available_port()
    let P2 = available_port()

    start Cluster as C {
        PORT_PRIMARY = P1
        PORT_SECONDARY = P2
    }

    shell C.primary {
        > cluster-info
        <? primary: ${C.PRIMARY_ID}
        <? secondary: ${C.SECONDARY_ID}
    }
}
```

## Migration

### Breaking changes

1. **All variables must be SCREAMING_SNAKE_CASE.** Existing `let port`, `expect db_port`, `fn curl(url, method)` must become `let PORT`, `expect DB_PORT`, `fn curl(URL, METHOD)`.

2. **Effect aliases must be CamelCase.** Existing `start Db as db` must become `start Db as Db` (or a more descriptive alias like `start Db as MyDb`).

3. **Variable identifiers** (`is_var_ident` in the parser) must be restricted to SCREAMING_SNAKE_CASE. The current permissive rule (any alphanumeric + underscore) is replaced.
