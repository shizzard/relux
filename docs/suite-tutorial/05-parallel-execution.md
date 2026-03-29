# Parallel Execution

[Previous: Shared Dependencies](04-shared-dependencies.md)

## The problem: port collisions

Every effect in the suite uses a hardcoded port: db on 9000, auth on 9010, tasks on 9020. This works when tests run one at a time -- each test tears down its effects before the next starts. But run the suite with `-j 4` and four tests spin up simultaneously. Four copies of `StartDb` all try to bind port 9000 -- the first succeeds, the other three crash.

The fix is to stop hardcoding ports. Each effect instance should get its own port, and downstream effects should know which port their dependency is listening on.

## Dynamic ports with `available_port()`

Relux provides a built-in pure function `available_port()` that binds to an ephemeral TCP port, records the port number, and releases the socket. Call it as close to startup as possible to minimize the window for another process to claim the same port.

Environment overlay variables let us pass a port into an effect at the `need` site. Combine the two and each effect instance gets a unique port.

Update `service/db.relux`:

```relux
import api/http

effect StartDb -> db {
    shell db {
        let db_root = "${__RELUX_TEST_ARTIFACTS}/database"

        > mkdir ${db_root}
        match_ok()

        !? ^error:

        > ${__RELUX_SUITE_ROOT}/db_service.py --port ${DB_PORT} --data-dir ${db_root}
        <~10s? ^listening on ${DB_PORT}$
    }
}

pure fn url(port, path) {
    "http://localhost:${port}${path}"
}
```

The effect now reads `${DB_PORT}` from its environment instead of hardcoding `9000`. The `<~10s?` is an inline tolerance timeout -- it means "wait up to 10 seconds for this pattern". When multiple tests run in parallel, services compete for CPU and may take longer to start than the default match timeout.

The `url` function takes the port as an argument -- functions cannot read overlay variables, so the caller must pass it explicitly.

## Propagating ports through the chain

`StartAuth` depends on `StartDb`. It needs to know the database port so it can pass `--db-port` to auth_service. Overlay variables solve this: the `need` site passes `DB_PORT` into `StartDb`, and `StartAuth` uses the same value for its own `--db-port` flag.

Update `service/auth.relux`:

```relux
import api/http
import service/db { url as db_url, StartDb }

effect StartAuth -> auth {
    need StartDb {
        DB_PORT = DB_PORT
    }

    shell db_client {
        log("create the auth database")
        http_request(200, db_url(DB_PORT, "/db/auth"), "POST")
    }

    shell auth {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/auth_service.py --port ${AUTH_PORT} --db-port ${DB_PORT}
        <~10s? ^listening on ${AUTH_PORT}$
    }
}

effect SeededAuth -> auth {
    need StartAuth as auth {
        DB_PORT = DB_PORT
        AUTH_PORT = AUTH_PORT
    }

    shell auth_client {
        log("create seed database users")
        http_request(200, url(AUTH_PORT, "/register"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        http_request(200, url(AUTH_PORT, "/register"), "POST", "{\"login\": \"bob\", \"password\": \"bob_secret\"}")
        http_request(200, url(AUTH_PORT, "/register"), "POST", "{\"login\": \"eva\", \"password\": \"eva_secret\"}")
    }
}

pure fn url(port, path) {
    "http://localhost:${port}${path}"
}
```

`StartAuth` receives `DB_PORT` and `AUTH_PORT` from its caller. It forwards `DB_PORT` to `StartDb` and uses both ports when starting the auth service.

`SeededAuth` receives `DB_PORT` and `AUTH_PORT` from its caller and passes them explicitly to `StartAuth`. Overlays are never inherited -- every `need` site must pass the values it wants the effect to see.

Update `service/tasks.relux`:

```relux
import api/http
import jq
import service/db { url as db_url, StartDb }
import service/auth { SeededAuth }

effect StartTasks -> tasks {
    need StartDb {
        DB_PORT = DB_PORT
    }
    need SeededAuth {
        DB_PORT = DB_PORT
        AUTH_PORT = AUTH_PORT
    }

    shell db_client {
        log("create the tasks database")
        http_request(200, db_url(DB_PORT, "/db/tasks"), "POST")
    }

    shell tasks {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/task_service.py --port ${TASKS_PORT} --db-port ${DB_PORT} --auth-port ${AUTH_PORT}
        <~10s? ^listening on ${TASKS_PORT}$
    }
}

effect SeededTasks -> tasks {
    need StartTasks as tasks {
        DB_PORT = DB_PORT
        AUTH_PORT = AUTH_PORT
        TASKS_PORT = TASKS_PORT
    }

    shell tasks_client {
        log("login as alice")
        let response_filename = http_request(200, url(TASKS_PORT, "/login"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        let token = jq_extract(response_filename, ".token")
        log("auth token: ${token}")

        log("create a task")
        let response_filename = http_request_authorized(200, url(TASKS_PORT, "/tasks"), "POST", token, "{\"title\": \"buy milk\", \"status\": \"todo\"}")
        let task_id = jq_extract(response_filename, ".id")
        jq_match_query(response_filename, ".title", "^buy milk$")

        log("login as bob")
        let response_filename = http_request(200, url(TASKS_PORT, "/login"), "POST", "{\"login\": \"bob\", \"password\": \"bob_secret\"}")
        let token = jq_extract(response_filename, ".token")
        log("auth token: ${token}")

        log("create a task")
        let response_filename = http_request_authorized(200, url(TASKS_PORT, "/tasks"), "POST", token, "{\"title\": \"buy milk\", \"status\": \"todo\"}")
        let task_id = jq_extract(response_filename, ".id")
        jq_match_query(response_filename, ".title", "^buy milk$")
    }
}

pure fn url(port, path) {
    "http://localhost:${port}${path}"
}
```

`SeededTasks` follows the same layering pattern we used for auth: it needs `StartTasks`, logs in as two users, and creates a task for each. Tests that need pre-existing tasks use `need SeededTasks` instead of setting up data themselves.

The full port chain looks like this:

```
  test
  |  DB_PORT = available_port()
  |  AUTH_PORT = available_port()
  |  TASKS_PORT = available_port()
  |
  StartTasks
  |    |         |
  |    |      SeededAuth
  |    |         |
  |    |      StartAuth
  |    |         |
  +----+---> StartDb
```

Each port is allocated once at the test level and flows down through overlays. Effect deduplication still works -- two `need StartDb { DB_PORT = DB_PORT }` with the same value share one instance.

## Updating the tests

Each test now passes ports when needing effects. Here is the updated `tasks/smoke.relux`:

```relux
import api/http
import jq
import service/tasks { url as tasks_url, StartTasks }

test "task CRUD" {
    """
    Log in, create a task, read it back, update it, and delete it.
    """
    let db_port = available_port()
    let auth_port = available_port()
    let tasks_port = available_port()

    need StartTasks {
        DB_PORT = db_port
        AUTH_PORT = auth_port
        TASKS_PORT = tasks_port
    }

    shell client {
        log("login as alice")
        let response_filename = http_request(200, tasks_url(tasks_port, "/login"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        let token = jq_extract(response_filename, ".token")
        log("auth token: ${token}")

        log("create a task")
        let response_filename = http_request_authorized(200, tasks_url(tasks_port, "/tasks"), "POST", token, "{\"title\": \"buy milk\", \"status\": \"todo\"}")
        let task_id = jq_extract(response_filename, ".id")
        jq_match_query(response_filename, ".title", "^buy milk$")

        log("read it back")
        let response_filename = http_request_authorized(200, tasks_url(tasks_port, "/tasks/${task_id}"), token)
        jq_match_query(response_filename, ".title", "^buy milk$")
        jq_match_query(response_filename, ".status", "^todo$")

        log("update the status")
        let response_filename = http_request_authorized(200, tasks_url(tasks_port, "/tasks/${task_id}"), "PUT", token, "{\"status\": \"done\"}")
        jq_match_query(response_filename, ".status", "^done$")

        log("delete it")
        let response_filename = http_request_authorized(200, tasks_url(tasks_port, "/tasks/${task_id}"), "DELETE", token)
        jq_match_query(response_filename, ".deleted", "^${task_id}$")
    }
}
```

The three `available_port()` calls at the top allocate unique ports. The overlay blocks on `need` pass them down. The test body passes `tasks_port` to `tasks_url()` so the curl commands target the right port.

The same pattern applies to the db and auth test files. Each test allocates the ports it needs and passes them through overlays.

## Error-path tests

The `SeededTasks` effect pays off in error-path tests. Create `tasks/errors.relux`:

```relux
import api/http
import jq
import service/tasks { url as tasks_url, StartTasks, SeededTasks }

# skip if SMOKE
test "unauthorized without token" {
    """
    Verify requests without a Bearer token return 401.
    """
    let db_port = available_port()
    let auth_port = available_port()
    let tasks_port = available_port()

    need StartTasks {
        DB_PORT = db_port
        AUTH_PORT = auth_port
        TASKS_PORT = tasks_port
    }

    shell client {
        http_request(401, tasks_url(tasks_port, "/tasks"), "POST", "{\"title\": \"nope\"}")
    }
}

# skip if SMOKE
test "get nonexistent task" {
    """
    Verify reading a task that does not exist returns 404.
    """
    let db_port = available_port()
    let auth_port = available_port()
    let tasks_port = available_port()

    need SeededTasks {
        DB_PORT = db_port
        AUTH_PORT = auth_port
        TASKS_PORT = tasks_port
    }

    shell client {
        log("login as alice")
        let response_filename = http_request(200, tasks_url(tasks_port, "/login"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        let token = jq_extract(response_filename, ".token")

        log("get a task that does not exist")
        let response_filename = http_request_authorized(404, tasks_url(tasks_port, "/tasks/999"), token)
        jq_match_query(response_filename, ".error", "^task 999 not found$")
    }
}
```

Notice the different effect choices: the unauthorized test only needs `StartTasks` -- no seeded data, just a running service to reject the request. The nonexistent task test needs `SeededTasks` because it logs in as alice, who must exist.

## Running in parallel

Enable parallel execution in `Relux.toml`:

```toml
[run]
jobs = 4
```

Or pass it on the command line:

```bash
relux run -j 4
```

Each test gets its own set of ports. Four tests running simultaneously means four separate service stacks, each on different ports, with no collisions. The effect graph is resolved per-test, and deduplication operates within a single test's dependency tree -- not across tests.

## CI readiness

A few flags make the suite CI-friendly:

**Timeout multiplier.** Tolerance timeouts (`~`) scale with the `-m` flag. CI machines are often slower, so double the timeouts:

```bash
relux run -m 2.0
```

Assertion timeouts (`@`) are never scaled -- they test hard time bounds that should hold on any machine.

**Fail-fast vs. all.** For local development, stop at the first failure:

```bash
relux run --strategy fail-fast
```

For CI, run everything to get the full picture:

```bash
relux run --strategy all
```

**Flaky markers.** If a test is inherently timing-sensitive, mark it so relux retries before reporting failure:

```relux
[flaky]
test "sometimes slow" {
    """
    Verify the service responds under load.
    """
    ...
}
```

**CI-only tests.** Some tests only make sense in CI:

```relux
[run if "${CI}"]
test "full integration" {
    """
    Run the full integration suite against the staging environment.
    """
    ...
}
```

## What we built

The suite started in chapter 0 as an empty project. Over five chapters it grew into a parallel integration test suite for three interconnected services:

- **Chapter 0** -- project setup and first empty test
- **Chapter 1** -- testing the database with inline curl commands
- **Chapter 2** -- extracting reusable HTTP and jq libraries
- **Chapter 3** -- effects for declarative infrastructure setup
- **Chapter 4** -- shared dependencies and effect deduplication
- **Chapter 5** -- dynamic ports, overlays, and parallel execution

The final architecture separates concerns cleanly:

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
    │   ├── auth/
    │   │   ├── smoke.relux
    │   │   └── errors.relux
    │   └── tasks/
    │       ├── smoke.relux
    │       └── errors.relux
    └── lib/
        ├── jq.relux
        ├── api/
        │   └── http.relux
        └── service/
            ├── db.relux
            ├── auth.relux
            └── tasks.relux
```

Test files say what to test. Library files say how to talk to services. Effect modules say how to start them. Overlays make everything parallel-safe.

---

The complete working example project is available at [`project/`](project/).
