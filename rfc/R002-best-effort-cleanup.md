# R002: Best-Effort Cleanup

- **Status**: accepted
- **Created**: 2026-03-10

## Motivation

Cleanup blocks currently restrict operations to `>`, `=>`, `let`, and variable assignment — no match operators, no function calls. This was a deliberate choice to avoid the "panic during unwind" problem: if a match fails during cleanup, what do you do?

The consequence is that cleanup shells have no way to synchronize against command completion. A `> rm -f /tmp/foo` is sent to the shell instantly, but the actual I/O takes orders of magnitude longer. Without a match operation to act as a barrier, the runtime may terminate the cleanup shell before the command finishes. This affects even trivial commands, and becomes critical for slower operations like `docker rm` or API calls.

## Design

Cleanup blocks become regular shell blocks with best-effort semantics. All operations are allowed; the difference is in the error boundary.

### Allowed operations (all of them)

| Operation                                  | Currently | Proposed |
|--------------------------------------------|-----------|----------|
| Send (`>`, `=>`)                           | Allowed   | Allowed  |
| `let` / assignment                         | Allowed   | Allowed  |
| Match operators (`<?`, `<=`)               | Forbidden | Allowed  |
| Fail patterns (`!?`, `!=`)                 | Forbidden | Allowed  |
| Timeout (`~`)                              | Forbidden | Allowed  |
| Function calls (impure)                    | Forbidden | Allowed  |
| Function calls (pure)                      | Forbidden | Allowed  |
| Buffer reset (`@reset`)                    | Forbidden | Allowed  |

### Failure semantics

On any failure in a cleanup block — match timeout, match failure, function error — the runtime:

1. Stops executing the cleanup block immediately
2. Terminates the cleanup shell
3. Emits a single warning

The test result is never affected by cleanup outcome. This is already the documented behavior for send failures; the change extends it uniformly to all operations.

### What stays the same

- Cleanup runs in a fresh implicit shell, not in any existing shell
- Existing shells are terminated automatically by the runtime
- Cleanup always executes, regardless of whether the test/effect passed or failed

## Example

```
effect PostgresTestDb(db_name) -> db {
    shell db {
        > createdb ${db_name}
        <? ^$
        match_prompt()
    }

    cleanup {
        > dropdb --if-exists ${db_name}
        match_prompt()
    }
}

test "migrations run cleanly" {
    need PostgresTestDb("test_migrations")

    shell db {
        > psql ${db_name} -f migrations.sql
        <? ^CREATE TABLE
    }

    cleanup {
        > rm -rf /tmp/test-artifacts
        match_prompt()
    }
}
```
