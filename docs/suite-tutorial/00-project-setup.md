# Project Setup

## What we are testing

This tutorial walks through building an integration test suite for a small but realistic system: three HTTP services that depend on each other.

```text
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ task_service │────▶│ auth_service │     │              │
│   :9020      │     │   :9010      │     │  db_service  │
│              │──┐  └──────┬───────┘     │   :9000      │
└──────────────┘  │         │             │              │
                  │         └────────────▶│              │
                  └──────────────────────▶│              │
                                          └──────────────┘
```

**db_service** is a key-value database. It stores data in flat files, exposes a JSON REST API, and logs every operation to stdout. You create named databases, then read and write keys inside them.

**auth_service** handles authentication. It stores login/password pairs in the db service and exposes register and login endpoints. It depends on a running db_service with a pre-created database.

**task_service** is the system under test. It provides a task management API with full CRUD, authenticates users through the auth service, and stores tasks in the db service. It depends on both other services.

All three are small Python scripts — no frameworks, no dependencies beyond the standard library. You can read their full specification in `SPEC.md`.

## Prerequisites

This tutorial assumes you have completed the [DSL tutorial](../dsl-tutorial/index.html) and are comfortable with all Relux language features: shells, operators, variables, functions, effects, imports, and condition markers.

You will also need:

- **Relux** installed and on your `PATH`
- **Python 3** (any recent version — the services use only the standard library)
- Some other common tools, but we will get to it later

## Scaffolding the project

Copy the services scripts into new directory: this will be out "monorepo". Now, from the project root:

```text
relux init
```

This creates `Relux.toml` and the `relux/` directory structure:

```text
project/
├── Relux.toml
├── SPEC.md
├── db_service.py
├── auth_service.py
├── task_service.py
└── relux/
    ├── .gitignore
    ├── tests/
    └── lib/
```

The generated `Relux.toml` is fully commented out — feel free to play with it. Although, I do not recommend to change the jobs number yet:

```toml
name = "suite-tutorial"

# [shell]
# command = "/bin/sh"
# prompt = "relux> "

[timeout]
match = "2s"
test = "30s"
suite = "5m"

# [run]
# jobs = 1
```

## Creating the first test file

Let's scaffold a test file for the database service:

```text
relux new --test db/smoke
```

```text
Created relux/tests/db/smoke.relux
```

The command creates `relux/tests/db/smoke.relux` with a starter test:

```relux
test "hello-relux" {
    shell myshell {
        > echo hello-relux
        <? ^hello-relux$
        match_ok()
    }
}
```

This is a placeholder — it just sends `echo hello-relux` and matches the output. Not useful yet, but it proves the toolchain works.

## Checking and running

First, validate the file without executing it:

```text
relux check
```

`check passed` shows that the relux code is fine. Now run it:

```text
relux run
```

```text
...
test result: ok. 1 passed; 0 failed; finished in 8.7ms
...
```

The template test passes. We have a working project and a confirmed toolchain. The placeholder test will be replaced with real database tests in the next chapter.

## Starting services manually

You will not need to start the services by hand once the test suite is written — effects will handle that. But it helps to understand how they work before automating them.

Each service is a standalone Python script with command-line arguments:

```text
# Start the database (port 9000, data in ./data)
python3 db_service.py --port 9000 --data-dir /tmp/db-data

# In another terminal: create a database and write a key
curl -X POST http://localhost:9000/db/mydb
curl -X PUT http://localhost:9000/db/mydb/greeting -d '{"value": "hello"}'
curl http://localhost:9000/db/mydb/greeting
```

Every service prints `listening on PORT` when it is ready to accept requests, and logs each operation as a plain-text line to stdout. This is important — Relux tests will match these log lines to verify service behavior from the inside, not just through HTTP responses.

---

Next: [Testing the Database Service](01-testing-the-database.md) — write the first real tests
