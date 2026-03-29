# Extracting a Library

[Previous: Testing the Database Service](01-testing-the-database.md)

## A second test file

The smoke tests work, but we only covered the happy path. The database has several error conditions worth testing: creating a database that already exists, reading a key that doesn't exist, operating on a nonexistent database. These deserve their own file.

Create `relux/tests/db/errors.relux`:

```bash
relux new --test db/errors
```

Replace the placeholder with an error-path test. You'll want to use the same helper functions from `smoke.relux` — `http_request`, `jq_match_query` — but of course they're not available here. Try writing the test anyway:

```relux
# skip if SMOKE
test "create duplicate database" {
    """
    Verify creating a database that already exists returns 409.
    """
    shell db {
        let db_root = "${__RELUX_TEST_ARTIFACTS}/database"

        > mkdir ${db_root}
        match_ok()
        
        !? ^error:

        > ${__RELUX_SUITE_ROOT}/db_service.py --data-dir ${db_root}
        <? ^listening on 9000$
    }

    shell client {
        log("create the database")
        let response_filename = http_request(200, "http://localhost:9000/db/testdb", "POST")
        jq_match_query(response_filename, ".created", "^testdb$")

        log("create it again — should fail")
        let response_filename = http_request(409, "http://localhost:9000/db/testdb", "POST")
        jq_match_query(response_filename, ".error", "^.*already exists.*$")
    }
}
```

Notice the `# skip if SMOKE` marker. As the suite grows, you will want a way to run just the smoke tests — the fast, happy-path checks that confirm services are alive. Error-path tests are important but slower and less critical for a quick sanity check. The marker reads the `SMOKE` environment variable: if it is set to any non-empty value, the test is skipped. Run `SMOKE=true relux run` and only the smoke tests execute. Skipped tests appear in the report as "skipped", not "failed" — because they were never run. We will add this marker to all non-smoke tests from here on.

Run `relux check` and you'll get errors: `http_request` and `jq_match_query` are not defined. They live in `smoke.relux`, and there's no way to reach them from here.

## Moving functions to lib/

The helper functions fall into two groups: HTTP plumbing (`curl`, `http_request`) and JSON inspection (`jq_match_query`). Let's split them accordingly.

Create `relux/lib/api/http.relux` with the HTTP helpers:

```relux
// curl functions

fn curl(url) {
    curl(url, "GET")
}

fn curl(url, method) {
    curl(url, method, "")
}

# skip unless which("curl")
fn curl(url, method, req_body) {
    let outdir = "${__RELUX_TEST_ARTIFACTS}/http"
    > mkdir -p ${outdir}
    match_ok()

    let file_rand = rand(10)
    let filename = "${outdir}/${file_rand}.http_response.txt"

    > curl -v -X ${method} -d '${req_body}' -o ${filename} ${url}
    <? ^> $

    filename
}

// http functions

fn http_match_code(code) {
    <? ^< HTTP/(\d\.\d) ${code} (.*)$
}

fn http_request(expected_code, url) {
    http_request(expected_code, url, "GET")
}

fn http_request(expected_code, url, method) {
    http_request(expected_code, url, method, "")
}

fn http_request(expected_code, url, method, req_body) {
    let response_filename = curl(url, method, req_body)
    http_match_code(expected_code)
    match_ok()
    response_filename
}
```

Create `relux/lib/jq.relux` with the JSON helper:

```relux
# skip unless which("jq")
fn jq_match_query(filename, query, pattern) {
    > jq -r '${query}' ${filename}
    <? ${pattern}
    match_ok()
}
```

Neither file has tests — they are pure library code. The `api/` subdirectory groups API-related helpers together; as the library grows, you might add `api/grpc.relux` or `api/mqtt.relux` next to it.

## Imports

Update `smoke.relux`: remove the function definitions and add imports at the top:

```relux
import api/http
import jq

test "key-value CRUD" {
    ...
}
```

Do the same for `errors.relux`:

```relux
import api/http
import jq

test "create duplicate database" {
    ...
}
```

Import paths resolve from the `<project_root>/relux/lib/` directory (reminder: `project_root` is the directory where `Relux.toml` lives). `api/http` thus points to `<project_root>/relux/lib/api/http.relux`.

These are wildcard imports — they bring in everything from the module. For a small project with well-named functions, this is fine. If you want to be explicit about what you use, selective imports list the names in curly braces:

```relux
import api/http { http_request }
import jq { jq_match_query }
```

Selective imports make it obvious which functions are in use and prevent name collisions. For now, wildcards keep things simple.

Run both files:

```bash
relux run
```

```text
...
test result: ok. 3 passed; 0 failed; finished in 382.1 ms
...
```

## Pure functions

Look at the test files again. The URL `http://localhost:9000` is hardcoded everywhere. When we later need to support dynamic ports (for parallel test execution), this will be a problem. Let's prepare by extracting URL construction into a function.

URL construction is just string concatenation — no shell commands needed. This is a good case for `pure fn`. Create `relux/lib/service/db.relux`:

```relux
pure fn url(path) {
    "http://localhost:9000${path}"
}
```

A `pure fn` cannot contain shell operators (`>`, `<?`, `<=`, `!?`, etc.). It can only do variable assignments, call other pure functions, and return values. The compiler enforces this — if you accidentally add a send or match operator inside a `pure fn`, `relux check` will reject it.

Why bother? Pure functions can be called in more places than regular functions: inside condition markers, inside overlay blocks, and inside other pure functions. They are also easier to reason about since they have no side effects.

The function is named `url` — generic within its module. To avoid confusion at the call site, import it with an alias:

```relux
import service/db { url as db_url }
import api/http
import jq
```

The `as` keyword renames the imported function. Now `db_url("/db/mydb")` reads clearly, and later we can add `service/auth.relux` with its own `url` function aliased to `auth_url`.

With all three imports in place, the tests look like this:

```relux
import service/db { url as db_url }
import api/http
import jq

test "key-value CRUD" {
    ...
    shell client {
        log("create the database")
        let response_filename = http_request(200, db_url("/db/mydb"), "POST")
        jq_match_query(response_filename, ".created", "^mydb$")

        log("write a key")
        let response_filename = http_request(200, db_url("/db/mydb/greeting"), "PUT", "{\"value\": \"hello\"}")
        jq_match_query(response_filename, ".wrote", "^greeting$")
        ...
    }
}
```

The gain is small for now. But in chapter 6, when we replace the hardcoded port with a dynamic one, we'll only need to change `url` in `service/db.relux` — not every test.

## What we have so far

```
project/
├── Relux.toml
├── db_service.py
├── auth_service.py
├── task_service.py
└── relux/
    ├── tests/
    │   └── db/
    │       ├── smoke.relux
    │       └── errors.relux
    └── lib/
        ├── jq.relux
        ├── api/
        │   └── http.relux
        └── service/
            └── db.relux
```

The HTTP and JSON helpers are shared. Both test files are focused on testing, not on plumbing. Adding a third test file for the database — say, multi-key workflows or concurrency — means writing only the test logic and a one-line import.

But look at the `shell db { ... }` block. It's still duplicated across every test in both files: create the artifacts directory, start the service, match the readiness line. When we add auth tests in the next chapter, we'll need the same db startup there too — plus auth on top of it. This is the kind of boilerplate that effects were designed to eliminate.

---

Next: [Effects and Dependencies](03-effects-and-dependencies.md) — extract service startup into reusable effects
