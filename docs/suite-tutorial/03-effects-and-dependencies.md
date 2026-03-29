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

Effects solve this. An effect is a reusable setup block that starts infrastructure and exports a shell for tests to use.

## The StartDb effect

Open `service/db.relux` -- we already have the `pure fn url` there. Add the effect to the same file:

```relux
import api/http

effect StartDb -> db {
    shell db {
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

The syntax `effect StartDb -> db` means: this effect is called `StartDb`, and it exports a shell named `db`. After the effect runs, tests can interact with that shell. In test you can decide not to use that shell at all, if you only need the effect for side effects (in this case, just for running database).

Co-locating the effect with the `url` function keeps everything about the database service in one module. Tests import both from the same place:

```relux
import service/db { url as db_url, StartDb }
```

Now update `smoke.relux`:

```relux
import service/db { url as db_url, StartDb }
import api/http
import jq

test "key-value CRUD" {
    """
    Create a database, write a key, read it back, and delete it.
    """
    need StartDb

    shell client {
        log("create the database")
        let response_filename = http_request(200, db_url("/db/mydb"), "POST")
        jq_match_query(response_filename, ".created", "^mydb$")
        ...
    }
}
```

`need StartDb` tells relux: before this test runs, execute the `StartDb` effect. The setup block is gone -- replaced by a single line. If you would need to match log lines on the running database shell, you would use `need StartDb as db`. After that, `shell db` blocks would be executed on the shell, that was exported by the `StartDb` effect.

Do the same for `errors.relux`. The manual `shell db { ... }` block is replaced by `need StartDb` in both files.

## The StartAuth effect

The auth service depends on db. It needs:
1. A running db_service
2. The `auth` database created in it
3. The auth_service itself started and ready

This is our first dependency chain -- an effect that needs another effect. Create `relux/lib/service/auth.relux`:

```relux
import api/http
import service/db { url as db_url, StartDb }

effect StartAuth -> auth {
    need StartDb

    shell db_client {
        log("create the auth database")
        http_request(200, db_url("/db/auth"), "POST")
    }

    shell auth {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/auth_service.py
        <? ^listening on 9010$
    }
}

pure fn url(path) {
    "http://localhost:9010${path}"
}
```

`need StartDb` inside the effect declares a dependency. When a test says `need StartAuth`, relux resolves the chain: it starts `StartDb` first, then runs `StartAuth`.

The `shell db_client { ... }` block opens a fresh shell to send the HTTP request that creates the `auth` database in the already-running db_service. The effect then starts auth_service in its own exported shell, sets a fail pattern, and waits for readiness.

Just like `service/db.relux`, the module co-locates the effect with a `pure fn url` for the auth service's base URL. Tests import both:

```relux
import service/auth { url as auth_url, StartAuth }
```

## Seeding test data

Before writing auth tests, think about what most of them need: pre-existing users. A registration test can create its own user, but every login test would repeat the same registration calls as setup noise.

We can solve this with another effect that builds on `StartAuth`. Add `SeededAuth` to `service/auth.relux`:

```relux
effect SeededAuth -> auth {
    need StartAuth as auth

    shell db_client {
        log("create seed database users")
        http_request(200, url("/register"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        http_request(200, url("/register"), "POST", "{\"login\": \"bob\", \"password\": \"bob_secret\"}")
        http_request(200, url("/register"), "POST", "{\"login\": \"eva\", \"password\": \"eva_secret\"}")
    }
}
```

`SeededAuth` needs `StartAuth`, which needs `StartDb`. The chain grows:

1. **StartDb** -- raw database service running
2. **StartAuth** -- auth service running, `auth` database created
3. **SeededAuth** -- auth service with pre-registered test users

Each layer adds exactly one concern. Tests choose the layer they need: a registration test needs `StartAuth`, a login test needs `SeededAuth`.

## Writing auth tests

Create `relux/tests/auth/smoke.relux`:

```bash
relux new --test auth/smoke
```

```relux
import api/http
import service/auth { url as auth_url, StartAuth }

test "register and login" {
    """
    Register a new user, log in with correct and incorrect passwords.
    """
    need StartAuth

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

One line of setup: `need StartAuth`. The test registers its own user, then exercises both success and failure login paths. This is a smoke test -- it only needs the auth service running, not pre-seeded data.

Now add error-path tests in `relux/tests/auth/errors.relux`. These use `SeededAuth` where pre-existing users are needed:

```relux
import api/http
import service/auth { url as auth_url, StartAuth, SeededAuth }

# skip if SMOKE
test "register duplicate user" {
    """
    Verify registering a user that already exists returns 409.
    """
    need SeededAuth

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
    need StartAuth

    shell client {
        http_request(404, auth_url("/login"), "POST", "{\"login\": \"nobody\", \"password\": \"whatever\"}")
    }
}
```

Notice the different effect choices: the duplicate registration test needs `SeededAuth` because alice must already exist. The unknown user test only needs `StartAuth` -- no seeded users, just a running auth service.

## What we have so far

```
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

Each service module in `lib/service/` owns its effects and its `url` function. Tests import both from the same place. Effects compose into layers -- `StartDb` -> `StartAuth` -> `SeededAuth` -- and tests pick the layer they need.

The pattern scales: when we add the task service in the next chapter, it will need both db and auth. Since `StartAuth` already needs `StartDb`, relux will resolve the full dependency graph automatically -- and run each effect exactly once.

---

Next: [Shared Dependencies](04-shared-dependencies.md) -- add the task service and see effect deduplication in action
