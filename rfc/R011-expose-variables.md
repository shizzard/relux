# R011: Expose Variables and Naming Conventions

- **Status**: implemented
- **Created**: 2026-04-18
- **Depends on**: R008

## Abstract

Extend effect `expose` declarations to support variables in addition to shells. Introduce an explicit `shell`/`var` keyword in `expose` to disambiguate the target type. Enforce CamelCase for effect aliases (the `as` target in `start` declarations), aligning them with effect naming conventions.

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
    
    let bucket_name

    shell Bucket.service {
        # have to extract the bucket name from the shell that created it
        > echo ${bucket_name}
        <= echo
        <? ^(.*)$
        bucket_name = $1
    }

    shell client {
        > upload --bucket ${bucket_name} ...
    }
}
```

This is fragile: the effect must export the value into the shell's environment, and the test must switch into that shell, echo it out, capture it with a regex, and assign it to a local variable — all before the value can be used. And the value is only available after that extraction sequence, not from the moment the effect is started. What we want is for the effect to declare certain computed values as part of its public interface, just as it declares shells.

### Ambiguous expose targets

The `expose` declaration currently accepts a bare identifier. When we extend it to support variables, `expose something` becomes ambiguous: is `something` a shell or a variable? Rather than restricting variable naming conventions (which would be a large, ergonomically costly breaking change), we solve this by requiring an explicit keyword.

### Effect alias casing

Effect aliases (the name after `as` in `start Effect as Alias`) currently use the permissive variable identifier rules. Since an alias names an effect instance — not a shell, function, or variable — it should follow the same convention as effects: CamelCase.

## Proposal

### Naming conventions

The DSL enforces the following naming conventions:

| Convention | Applies to | Examples |
|---|---|---|
| CamelCase | Effects, effect aliases | `Db`, `StartAuth`, `start Node as Primary` |
| snake_case | Functions, shells | `curl()`, `http_request()`, `shell service` |
| Permissive | Variables (`let`, `expect`, function parameters) | `let port`, `let PORT`, `let db_host` |

Variable identifiers remain permissive (any alphanumeric + underscore, starting with a letter or underscore), matching current behavior. The only naming change is that effect aliases must now be CamelCase.

### Keyword-disambiguated expose

The `expose` declaration requires an explicit `shell` or `var` keyword to specify the target type:

```relux
effect CreateBucket {
    expect REGION
    let bucket_id = uuid()
    expose shell service
    expose var bucket_id

    shell service {
        > create-bucket --id ${bucket_id} --region ${REGION}
        <? bucket ready
    }
}
```

`expose shell service` exposes a shell. `expose var bucket_id` exposes a variable. The keyword removes all ambiguity without imposing naming restrictions on variables.

### Re-exposing from dependencies

Both shells and variables can be re-exposed from dependencies using the existing dot-access and `as` syntax, with the `shell`/`var` keyword:

```relux
effect FullStack {
    start CreateBucket as Storage
    start StartApi as Api

    expose shell Api.service as api
    expose shell Storage.service as storage
    expose var Storage.bucket_id as BUCKET_ID
}
```

The `as` alias must follow the convention of its type — shell aliases are snake_case, variable aliases are permissive.

### Accessing exposed variables from tests

Tests access exposed variables through the effect alias using dot-notation in interpolation:

```relux
test "upload to bucket" {
    start FullStack as Stack

    shell Stack.api {
        > upload --bucket ${Stack.bucket_id} file.txt
        <= uploaded
    }
}
```

`${Stack.bucket_id}` reads the variable exposed by the `FullStack` effect instance aliased as `Stack`. The same dot-access syntax works in any interpolation context — send, match, and shell-level `let` bindings:

```relux
    shell Stack.api {
        let my_bucket = Stack.bucket_id
        > upload --bucket ${my_bucket} file.txt
    }
```

Test-level and effect-level `let` bindings cannot reference exposed variables because they are evaluated at resolve time, before effects are started. Shell-level `let` works because it executes at runtime, when the effect instance and its exposed values are available.

### Immutability

Exposed variables are read-only from the caller's perspective. A test or parent effect can read `${Alias.var}` but cannot assign to it. The value was computed during effect setup and is fixed for the lifetime of the effect instance.

## Examples

### Leaf effect exposing a variable

```relux
effect StartDb {
    expect DB_NAME
    let port = available_port()
    expose shell db
    expose var port

    shell db {
        > start-db --port ${port} --name ${DB_NAME}
        <~10s? listening on ${port}
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
        > curl http://localhost:${Db.port}/status
        <? running
    }
}
```

### Composed effect re-exposing variables

```relux
effect StartAuth {
    let auth_port = available_port()
    start StartDb as Db {
        DB_NAME = "auth"
    }
    expose shell auth
    expose var auth_port
    expose var Db.port as db_port

    shell auth {
        > start-auth --port ${auth_port} --db-port ${Db.port}
        <~10s? listening on ${auth_port}
    }
}

test "auth uses correct db" {
    start StartAuth as Auth

    shell client {
        > curl http://localhost:${Auth.auth_port}/config
        <? db_port: ${Auth.db_port}
    }
}
```

### Function parameters

```relux
fn http_request(expected_code, url, method) {
    > curl -s -o /tmp/response.json -w "%{http_code}" -X ${method} ${url}
    <? ^${expected_code}$
}

pure fn url(path) {
    "http://localhost:9000${path}"
}
```

### Effect alias as CamelCase

```relux
effect Cluster {
    expect port_primary, port_secondary

    start Node as Primary {
        node_port = port_primary
        node_name = "primary"
    }
    start Node as Secondary {
        node_port = port_secondary
        node_name = "secondary"
    }

    expose shell Primary.node as primary
    expose shell Secondary.node as secondary
    expose var Primary.node_id as primary_id
    expose var Secondary.node_id as secondary_id
}

test "cluster status" {
    let p1 = available_port()
    let p2 = available_port()

    start Cluster as C {
        port_primary = p1
        port_secondary = p2
    }

    shell C.primary {
        > cluster-info
        <? primary: ${C.primary_id}
        <? secondary: ${C.secondary_id}
    }
}
```

## Migration

### Breaking changes

1. **`expose` requires `shell`/`var` keyword.** Existing `expose service` must become `expose shell service`.

2. **Effect aliases must be CamelCase.** Existing `start Db as db` must become `start Db as Db` (or a more descriptive alias like `start Db as MyDb`).
