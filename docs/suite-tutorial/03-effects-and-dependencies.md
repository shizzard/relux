# Effects and Dependencies

[Previous: Extracting a Library](02-extracting-a-library.md)

## The problem

Every test that needs the database service repeats the same setup:

```relux
    shell db {
        let db_root = "${__RELUX_TEST_ARTIFACTS}/database"

        > mkdir ${db_root}
        match_ok()

        !? ^error:

        > ${__RELUX_SUITE_ROOT}/db_service.py --data-dir ${db_root}
        <? ^listening on 9000$
    }
```

Many lines, copy-pasted into every test in every file. But the real pain starts now: the auth service needs a running database with a pre-created `auth` database. Every auth test would need to start db, create the database, then start auth -- that is 15+ lines of setup before a single assertion. And if you get any of it wrong, you get a cryptic match timeout instead of a clear error.

Effects solve this. An effect is a reusable setup block that starts infrastructure and exposes shells for tests to use.

## The Db effect

Open `service/db.relux` -- we already have the `pure fn url` there. Add the effect to the same file:

```relux
import api/http

effect Db {
    expose service

    shell service {
        let db_root = "${__RELUX_TEST_ARTIFACTS}/database"

        > mkdir ${db_root}
        match_ok()

        !? ^error:

        > ${__RELUX_SUITE_ROOT}/db_service.py --data-dir ${db_root}
        <? ^listening on 9000$
    }
}

pure fn url(path) {
    "http://localhost:9000${path}"
}
```

The `expose service` declaration means: the `service` shell is part of this effect's public interface. After the effect runs, tests that started it with an alias can interact with that shell via dot-access (e.g. `shell db.service { ... }`). If a test only needs the effect for its side effects (in this case, just for running the database), it can omit the alias and skip shell access entirely.

Co-locating the effect with the `url` function keeps everything about the database service in one module. Tests import both from the same place:

```relux
import service/db { url as db_url, Db }
```

Now update `smoke.relux`:

```relux
import service/db { url as db_url, Db }
import api/http
import jq

test "key-value CRUD" {
    """
    Create a database, write a key, read it back, and delete it.
    """
    start Db

    shell client {
        log("create the database")
        let response_filename = http_request(200, db_url("/db/mydb"), "POST")
        jq_match_query(response_filename, ".created", "^mydb$")
        ...
    }
}
```

`start Db` tells relux: before this test runs, execute the `Db` effect. The setup block is gone -- replaced by a single line. If you would need to match log lines on the running database shell, you would use `start Db as db`. After that, `shell db.service { ... }` blocks would be executed on the shell that was exposed by the `Db` effect.

Do the same for `errors.relux`. The manual `shell db { ... }` block is replaced by `start Db` in both files.

## The Auth effect

The auth service depends on db. It needs:
1. A running db_service
2. The `auth` database created in it
3. The auth_service itself started and ready

This is our first dependency chain -- an effect that depends on another effect. Create `relux/lib/service/auth.relux`:

```relux
import api/http
import service/db { url as db_url, Db }

effect Auth {
    start Db
    expose service

    shell setup {
        log("create the auth database")
        http_request(200, db_url("/db/auth"), "POST")
    }

    shell service {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/auth_service.py
        <? ^listening on 9010$
    }
}

pure fn url(path) {
    "http://localhost:9010${path}"
}
```

`start Db` inside the effect declares a dependency. When a test says `start Auth`, relux resolves the chain: it starts `Db` first, then runs `Auth`.

The `shell setup { ... }` block opens a fresh shell to send the HTTP request that creates the `auth` database in the already-running db_service. The effect then starts auth_service in its own exposed `service` shell, sets a fail pattern, and waits for readiness.

Just like `service/db.relux`, the module co-locates the effect with a `pure fn url` for the auth service's base URL. Tests import both:

```relux
import service/auth { url as auth_url, Auth }
```

## Seeding test data

Before writing auth tests, think about what most of them need: pre-existing users. A registration test can create its own user, but every login test would repeat the same registration calls as setup noise.

We can solve this with another effect that builds on `Auth`. Add `SeededAuth` to `service/auth.relux`:

```relux
effect SeededAuth {
    start Auth as auth
    expose auth.service as service

    shell seeder {
        log("create seed database users")
        http_request(200, url("/register"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        http_request(200, url("/register"), "POST", "{\"login\": \"bob\", \"password\": \"bob_secret\"}")
        http_request(200, url("/register"), "POST", "{\"login\": \"eva\", \"password\": \"eva_secret\"}")
    }
}
```

`SeededAuth` doesn't define its own service shell — the auth service is already running inside `Auth`. The `expose auth.service as service` declaration re-exposes the `service` shell from the `Auth` dependency (aliased `auth`) so that tests starting `SeededAuth` can access it the same way they would with `Auth`.

The chain grows:

1. **Db** -- raw database service running
2. **Auth** -- auth service running, `auth` database created
3. **SeededAuth** -- auth service with pre-registered test users

Each layer adds exactly one concern. Tests choose the layer they need: a registration test starts `Auth`, a login test starts `SeededAuth`.

## Writing auth tests

Create `relux/tests/auth/smoke.relux`:

```text
relux new --test auth/smoke
```

```relux
import api/http
import service/auth { url as auth_url, Auth }

test "register and login" {
    """
    Register a new user, log in with correct and incorrect passwords.
    """
    start Auth

    shell client {
        log("register a new user")
        http_request(200, auth_url("/register"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")

        log("login with correct password")
        http_request(200, auth_url("/login"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")

        log("login with wrong password")
        http_request(401, auth_url("/login"), "POST", "{\"login\": \"alice\", \"password\": \"wrong\"}")
    }
}
```

One line of setup: `start Auth`. The test registers its own user, then exercises both success and failure login paths. This is a smoke test -- it only needs the auth service running, not pre-seeded data.

Now add error-path tests in `relux/tests/auth/errors.relux`. These use `SeededAuth` where pre-existing users are needed:

```relux
import api/http
import service/auth { url as auth_url, Auth, SeededAuth }

# skip if SMOKE
test "register duplicate user" {
    """
    Verify registering a user that already exists returns 409.
    """
    start SeededAuth

    shell client {
        log("register alice again")
        http_request(409, auth_url("/register"), "POST", "{\"login\": \"alice\", \"password\": \"other\"}")
    }
}

# skip if SMOKE
test "login unknown user" {
    """
    Verify logging in with a nonexistent user returns 404.
    """
    start Auth

    shell client {
        http_request(404, auth_url("/login"), "POST", "{\"login\": \"nobody\", \"password\": \"whatever\"}")
    }
}
```

Notice the different effect choices: the duplicate registration test starts `SeededAuth` because alice must already exist. The unknown user test only starts `Auth` -- no seeded users, just a running auth service.

## What we have so far

```text
project/
├── Relux.toml
├── db_service.py
├── auth_service.py
├── task_service.py
└── relux/
    ├── tests/
    │   ├── db/
    │   │   ├── smoke.relux
    │   │   └── errors.relux
    │   └── auth/
    │       ├── smoke.relux
    │       └── errors.relux
    └── lib/
        ├── jq.relux
        ├── api/
        │   └── http.relux
        └── service/
            ├── db.relux
            └── auth.relux
```

Each service module in `lib/service/` owns its effects and its `url` function. Tests import both from the same place. Effects compose into layers -- `Db` -> `Auth` -> `SeededAuth` -- and tests pick the layer they need.

The pattern scales: when we add the task service in the next chapter, it will start both db and auth. Since `Auth` already starts `Db`, relux will resolve the full dependency graph automatically -- and run each effect exactly once.

---

Next: [Shared Dependencies](04-shared-dependencies.md) -- add the task service and see effect deduplication in action
