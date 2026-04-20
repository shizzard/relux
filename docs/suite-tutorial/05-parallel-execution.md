# Parallel Execution

[Previous: Shared Dependencies](04-shared-dependencies.md)

## The problem: port collisions

Every effect in the suite uses a hardcoded port: db on 9000, auth on 9010, tasks on 9020. This works when tests run one at a time -- each test tears down its effects before the next starts. But run the suite with `-j 4` and four tests spin up simultaneously. Four copies of `Db` all try to bind port 9000 -- the first succeeds, the other three crash.

The fix is to stop hardcoding ports. Each effect instance should get its own port, and downstream effects should know which port their dependency is listening on.

## Dynamic ports with `available_port()`

Relux provides a built-in pure function `available_port()` that binds to an ephemeral TCP port, records the port number, and releases the socket. Call it as close to startup as possible to minimize the window for another process to claim the same port.

Environment overlay variables let us pass a port into an effect at the `start` site. Combine the two and each effect instance gets a unique port.

Update `service/db.relux`:

```relux
import api/http

effect Db {
    expect DB_PORT
    expose shell service

    shell service {
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

The `expect DB_PORT` declaration says: this effect requires `DB_PORT` to be provided by whoever starts it. If a caller forgets to pass it, relux reports an error at check time. The `<~10s?` is an inline tolerance timeout -- it means "wait up to 10 seconds for this pattern". When multiple tests run in parallel, services compete for CPU and may take longer to start than the default match timeout.

The `url` function takes the port as an argument -- functions cannot read overlay variables, so the caller must pass it explicitly.

## Propagating ports through the chain

`Auth` depends on `Db`. It needs to know the database port so it can pass `--db-port` to auth_service. The `expect` declaration names the required variables, and the `start` site passes them through an overlay block.

Update `service/auth.relux`:

```relux
import api/http
import service/db { url as db_url, Db }

effect Auth {
    expect DB_PORT, AUTH_PORT
    start Db
    expose shell service

    shell setup {
        log("create the auth database")
        http_request(200, db_url(DB_PORT, "/db/auth"), "POST")
    }

    shell service {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/auth_service.py --port ${AUTH_PORT} --db-port ${DB_PORT}
        <~10s? ^listening on ${AUTH_PORT}$
    }
}

effect SeededAuth {
    expect DB_PORT, AUTH_PORT
    start Auth as Dep
    expose shell Dep.service as service

    shell seeder {
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

`Auth` declares `expect DB_PORT, AUTH_PORT` — both must be provided by whoever starts it. Note that it does not need to pass the expected environment variables explicitly: these are passed inside the inherited enviroment overlay. This works as long as the expected environment variables in different overlays have the same name.

`SeededAuth` declares the same expects and passes them through to `Auth`. Each effect in the chain declares what it requires and forwards what its dependencies need.

Update `service/tasks.relux`:

```relux
import api/http
import jq
import service/db { url as db_url, Db }
import service/auth { SeededAuth }

effect Tasks {
    expect DB_PORT, AUTH_PORT, TASKS_PORT
    start Db
    start SeededAuth
    expose shell service

    shell setup {
        log("create the tasks database")
        http_request(200, db_url(DB_PORT, "/db/tasks"), "POST")
    }

    shell service {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/task_service.py --port ${TASKS_PORT} --db-port ${DB_PORT} --auth-port ${AUTH_PORT}
        <~10s? ^listening on ${TASKS_PORT}$
    }
}

effect SeededTasks {
    expect DB_PORT, AUTH_PORT, TASKS_PORT
    start Tasks as Dep
    expose shell Dep.service as service

    shell seeder {
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

`SeededTasks` follows the same layering pattern we used for auth: it starts `Tasks`, logs in as two users, and creates a task for each. Tests that need pre-existing tasks use `start SeededTasks` instead of setting up data themselves.

Each port is allocated once at the test level and flows down through overlays. Effect deduplication still works -- two `start Db { DB_PORT }` with the same evaluated value share one instance.

## Updating the tests

Each test now passes ports when starting effects. Here is the updated `tasks/smoke.relux`:

```relux
import api/http
import jq
import service/tasks { url as tasks_url, Tasks }

test "task CRUD" {
    """
    Log in, create a task, read it back, update it, and delete it.
    """
    let db_port = available_port()
    let auth_port = available_port()
    let tasks_port = available_port()

    start Tasks {
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

The three `available_port()` calls at the top allocate unique ports. The overlay blocks on `start` pass them down. The test body passes `tasks_port` to `tasks_url()` so the curl commands target the right port.

The same pattern applies to the db and auth test files. Each test allocates the ports it needs and passes them through overlays.

## Error-path tests

The `SeededTasks` effect pays off in error-path tests. Create `tasks/errors.relux`:

```relux
import api/http
import jq
import service/tasks { url as tasks_url, Tasks, SeededTasks }

# skip if SMOKE
test "unauthorized without token" {
    """
    Verify requests without a Bearer token return 401.
    """
    let db_port = available_port()
    let auth_port = available_port()
    let tasks_port = available_port()

    start Tasks {
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

    start SeededTasks {
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

Notice the different effect choices: the unauthorized test only starts `Tasks` -- no seeded data, just a running service to reject the request. The nonexistent task test starts `SeededTasks` because it logs in as alice, who must exist.

## Running in parallel

Enable parallel execution in `Relux.toml`:

```toml
[run]
jobs = 4
```

Or pass it on the command line:

```text
relux run -j 4
```

Each test gets its own set of ports. Four tests running simultaneously means four separate service stacks, each on different ports, with no collisions. The effect graph is resolved per-test, and deduplication operates within a single test's dependency tree -- not across tests.

## CI readiness

A few flags make the suite CI-friendly:

**Timeout multiplier.** Tolerance timeouts (`~`) scale with the `-m` flag. CI machines are often slower, so double the timeouts:

```text
relux run -m 2.0
```

Assertion timeouts (`@`) are never scaled -- they test hard time bounds that should hold on any machine.

**Fail-fast vs. all.** For local development, stop at the first failure:

```text
relux run --strategy fail-fast
```

For CI, run everything to get the full picture:

```text
relux run --strategy all
```

**Flaky markers.** If a test is inherently timing-sensitive, mark it so relux retries before reporting failure:

```relux
# flaky
test "sometimes slow" {
    """
    Verify the service responds under load.
    """
    ...
}
```

**CI-only tests.** Some tests only make sense in CI:

```relux
# run if CI
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

Test files say what to test. Library files say how to talk to services. Effect modules say how to start them. Environment overlays make everything parallel-safe.

---

The complete working example project is available at [`project/`](project/).
