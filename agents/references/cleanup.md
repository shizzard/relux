# Cleanup

The `cleanup` block runs in a freshly spawned implicit shell after the test (or effect) finishes. Both test blocks and effect blocks may declare one.

## Where it lives

- **Test cleanup** -- inside a `test "name" { ... }` body, after all `shell` blocks. At most one per test.
- **Effect cleanup** -- inside an `effect Name { ... }` body, as the last section after `shell` blocks. At most one per effect.

## When it runs

1. All test/effect shells are terminated.
2. Test cleanup (if present).
3. Effect cleanups, in reverse topological order (dependents before dependencies).

Always runs, regardless of pass/fail/cancel. Effect cleanup fires at last-guard release of the effect instance -- once per unique identity *within a single test's dependency graph*, no matter how many `start` sites shared it. The effect registry is per-test (see [effects-identity](effects-identity.md) > Scope), so two tests that both `start` the same effect run setup and cleanup independently -- nothing crosses the test boundary. Test cleanup fires once per test.

Cleanup runs under uncancellable tokens: still executes when the test was cancelled (`TestTimeout`, `SuiteTimeout`, `FailFast`, `Sigint`). Cleanup itself has its own timeout budget but cannot be cancelled by the parent.

## Allowed operations

Any statement valid in a shell block is valid here: `>`, `=>`, `<?`, `<=`, `!?`, `!=`, multimatch, `let`, reassignment, timeouts, function calls, BIFs.

## Shell visibility

Cleanup runs in a brand-new implicit shell -- not the test's shell, not the effect's exposed shells (they're terminated by step 1).

Visible state:

| In test cleanup | In effect cleanup |
|---|---|
| Test-level `let` bindings | Effect-level `let` bindings |
| Inherited environment | Inherited environment |
|  | Overlay variables (from the `start` site that materialised the instance) |

Not visible in either: shell-scoped `let` bindings, regex captures, prior buffer state from any other shell.

## Failure semantics

Cleanup failures are logged as warnings. They do not change the test's pass/fail outcome.

## Pitfalls and best practices

### Don't depend on test-body state inside cleanup

Cleanup's shell starts empty. The test's buffer, captures, prior sends, and shell-scoped `let`s are not visible. Promote any value cleanup needs to a top-level `let` (effect or test scope) or an overlay variable.

Don't:

```relux
test "..." {
    shell s {
        > start-pg
        <? pid=(\d+)
        let pg_pid = $1
    }
    cleanup {
        // pg_pid not in scope -- shell-scoped lets don't reach cleanup
        > pg-stop --pid ${pg_pid}
    }
}
```

Do:

```relux
test "..." {
    // test-level let, visible in cleanup
    let pg_pid
    shell s {
        > start-pg
        <? pid=(\d+)
        pg_pid = $1
    }
    cleanup {
        > pg-stop --pid ${pg_pid}
    }
}
```

### Don't set fail patterns in cleanup

Cleanup is best-effort: a `!?` / `!=` hit only emits a warning, never changes the test outcome. The slot starts empty in cleanup's fresh shell -- leave it that way. If a teardown command's exit status matters, check it via the command itself.

Don't:

```relux
cleanup {
    !? ERROR
    > pg-stop
    <? stopped
}
```

Do:

```relux
cleanup {
    > pg-stop -f /tmp/run-${__RELUX_RUN_ID}/pid || true
}
```

### Make cleanup idempotent

Cleanup runs even when setup failed partway. Assume the resource may already be gone, never created, or in a half-state. Use `-f`, `|| true`, or explicit existence checks.

Don't:

```relux
cleanup {
    > rm /tmp/run-${__RELUX_RUN_ID}/socket
    > rmdir /tmp/run-${__RELUX_RUN_ID}
}
```

Do:

```relux
cleanup {
    > rm -f /tmp/run-${__RELUX_RUN_ID}/socket
    > rm -rf /tmp/run-${__RELUX_RUN_ID}
}
```

### Don't delete files under `${__RELUX_RUN_ARTIFACTS}`

Files dropped under `${__RELUX_RUN_ARTIFACTS}` are scanned by the runtime and surfaced as the test's artifact list (see [project-layout](project-layout.md) > Built-in environment variables). They are the post-mortem surface for the viewer -- logs, rendered configs, captured outputs. Cleanup that removes them erases that surface for every test that shared the effect instance.

Cleanup is for state *outside* the run dir (real databases, cloud resources, shared queues). Leave the run dir alone.

Don't:

```relux
cleanup {
    > rm -rf ${__RELUX_RUN_ARTIFACTS}/db/
}
```

Do:

```relux
cleanup {
    > psql -h prod-db -c "DROP DATABASE test_${run_id}"
}
```

### Don't use cleanup to stop services

Shell termination already kills any process the shell launched (children of the PTY). Cleanup runs in a separate shell with no link to those processes -- if Relux is killed, cleanup never runs and orphans anything you tried to stop here. Reserve cleanup for filesystem side effects (temp dirs, log collection) that survive shell termination.

Don't:

```relux
shell svc {
    > my-service --foreground
    <? ready
}
cleanup {
    > pkill my-service
}
```

Do:

```relux
shell svc {
    > my-service --foreground
    <? ready
}
cleanup {
    > rm -rf /tmp/my-service-state
}
```

## See also

- [effects-identity](effects-identity.md) -- read if you need effect lifecycle and last-release semantics
- [fail-patterns](fail-patterns.md) -- read if you need fail-pattern slot rules
- [statements](statements.md) -- read if you need `let` placement for cleanup-visible values
- [block-structure](block-structure.md) -- read if you need where the cleanup section sits in test/effect bodies
- [project-layout](project-layout.md) -- read if you need the built-in env vars (`__RELUX_RUN_ID`, etc.) used in the cleanup examples above
