# Shared Dependencies

[Previous: Effects and Dependencies](03-effects-and-dependencies.md)

## The task service

The task service is the most complex piece of the stack. It depends on both db and auth:

- It stores tasks in the db service (in a `tasks` database)
- It authenticates users through the auth service
- It has its own `/login` endpoint that forwards credentials to auth and issues a Bearer token

To start it, we need a running db_service, a running auth_service, and the `tasks` database created. But `Auth` already starts `Db` internally. If `Tasks` also starts `Db`, does the database start twice?

## The Tasks effect

Create `relux/lib/service/tasks.relux`:

```relux
import api/http
import service/db { url as db_url, Db }
import service/auth { Auth }

effect Tasks {
    start Db
    start Auth
    expose service

    shell setup {
        log("create the tasks database")
        http_request(200, db_url("/db/tasks"), "POST")
    }

    shell service {
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/task_service.py
        <? ^listening on 9020$
    }
}

pure fn url(path) {
    "http://localhost:9020${path}"
}
```

`Tasks` declares two dependencies: `start Db` and `start Auth`. But `Auth` itself also starts `Db`. This creates a diamond in the dependency graph:

```
  Tasks
  |   |
  |   Auth
  |   |
  +-- Db
```

If relux started a new db_service for each `start Db`, we would end up with two database instances on the same port -- and the second one would fail to bind. This is where effect deduplication comes in.

Relux identifies each effect instance by its name and overlay values (we will cover overlays in the next chapter). Two `start Db` statements with no overlay both refer to the same identity: `(Db, {})`. Relux resolves the full dependency graph as a DAG and runs each unique instance exactly once.

In our case, the execution order is:

1. **Db** -- starts the database (once)
2. **Auth** -- creates the `auth` database, starts auth_service
3. **Tasks** setup -- creates the `tasks` database, starts task_service

Step 2 and the setup part of step 3 both use the single running database from step 1. No port conflicts, no duplicated work.

## Authenticated requests

The task service requires a Bearer token on every request except `/login`. We need to extend the HTTP library to support authentication headers.

Add a 4-arity `curl` and `http_request_authorized` functions to `api/http.relux`:

```relux
# skip unless which("curl")
fn curl(url, method, req_body, extra_headers) {
    let outdir = "${__RELUX_TEST_ARTIFACTS}/http"
    > mkdir -p ${outdir}
    match_ok()

    let file_rand = rand(10)
    let filename = "${outdir}/${file_rand}.http_response.txt"

    > curl -v -X ${method} ${extra_headers} -d '${req_body}' -o ${filename} ${url}
    <? ^> $

    filename
}
```

The existing 3-arity `curl` now delegates to the 4-arity version with an empty `extra_headers`. The new `http_request_authorized` functions pass the Bearer header:

```relux
fn http_request_authorized(expected_code, url, token) {
    http_request_authorized(expected_code, url, "GET", token, "")
}

fn http_request_authorized(expected_code, url, method, token) {
    http_request_authorized(expected_code, url, method, token, "")
}

fn http_request_authorized(expected_code, url, method, token, req_body) {
    let response_filename = curl(url, method, req_body, "-H 'Authorization: Bearer ${token}'")
    http_match_code(expected_code)
    match_ok()
    response_filename
}
```

We also need a way to extract values (not just match them) from JSON responses. The login endpoint returns a token that we need to capture and use in subsequent requests. Add `jq_extract` to `jq.relux`:

```relux
# skip unless which("jq")
fn jq_extract(filename, query) {
    > jq -r '${query}' ${filename}
    <? ^jq (.*)$
    <? ^(.+)$
    let value = $0
    match_ok()
    value
}
```

The first `<? ^jq (.*)$` skips past the echoed command in the output buffer. The second `<? ^(.+)$` matches the actual jq output, and `let value = $0` captures the full match into `value`. The function returns it so the caller can store the result.

## Writing task tests

Create `relux/tests/tasks/smoke.relux`:

```bash
relux new --test tasks/smoke
```

```relux
import api/http
import jq
import service/auth { SeededAuth }
import service/tasks { url as tasks_url, Tasks }

test "task CRUD" {
    """
    Log in, create a task, read it back, update it, and delete it.
    """
    start Tasks
    start SeededAuth

    shell client {
        log("login as alice")
        let response_filename = http_request(200, tasks_url("/login"), "POST", "{\"login\": \"alice\", \"password\": \"alice_secret\"}")
        let token = jq_extract(response_filename, ".token")

        log("create a task")
        let response_filename = http_request_authorized(200, tasks_url("/tasks"), "POST", token, "{\"title\": \"buy milk\", \"status\": \"todo\"}")
        let task_id = jq_extract(response_filename, ".id")
        jq_match_query(response_filename, ".title", "^buy milk$")

        log("read it back")
        let response_filename = http_request_authorized(200, tasks_url("/tasks/${task_id}"), token)
        jq_match_query(response_filename, ".title", "^buy milk$")
        jq_match_query(response_filename, ".status", "^todo$")

        log("update the status")
        let response_filename = http_request_authorized(200, tasks_url("/tasks/${task_id}"), "PUT", token, "{\"status\": \"done\"}")
        jq_match_query(response_filename, ".status", "^done$")

        log("delete it")
        let response_filename = http_request_authorized(200, tasks_url("/tasks/${task_id}"), "DELETE", token)
        jq_match_query(response_filename, ".deleted", "^${task_id}$")
    }
}
```

The test starts both `Tasks` and `SeededAuth`. Behind the scenes, this pulls in the entire stack: db starts once, auth starts with the `auth` database and seed users, tasks starts with the `tasks` database. The test itself is just login and CRUD.

Notice how `jq_extract` captures the token and task ID into variables that are used in subsequent requests. The `${task_id}` in `tasks_url("/tasks/${task_id}")` is interpolated at the call site -- relux variables are just strings.

## The dependency graph

With all three services, the full effect graph looks like this:

```
  test "task CRUD"
  |              |
  Tasks          SeededAuth
  |   |          |
  |   +--------> Auth
  |              |
  +------------> Db
```

Five `start` statements across effects and the test, but only four unique effect instances. `Auth` is started by both `Tasks` and `SeededAuth` -- it runs once. `Db` is started by both `Tasks` and `Auth` -- it runs once. You declare what you need; relux figures out the rest.

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
    │   ├── auth/
    │   │   ├── smoke.relux
    │   │   └── errors.relux
    │   └── tasks/
    │       └── smoke.relux
    └── lib/
        ├── jq.relux
        ├── api/
        │   └── http.relux
        └── service/
            ├── db.relux
            ├── auth.relux
            └── tasks.relux
```

The suite now tests all three services. Each service has its own effect module. Dependencies are declared, not managed -- relux resolves the graph and deduplicates automatically.

But all the effects use hardcoded ports: 9000, 9010, 9020. Running tests sequentially works because each test tears down before the next starts. Run with `-j 4` and multiple tests will try to bind the same ports simultaneously. The next chapter solves this with dynamic ports and overlays.

---

Next: [Parallel Execution](05-parallel-execution.md) -- replace hardcoded ports with dynamic allocation
