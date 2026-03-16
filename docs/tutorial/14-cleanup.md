# Cleanup

[Previous: Effects and Dependencies](13-effects-and-dependencies.md)

The [previous article](13-effects-and-dependencies.md) introduced effects as reusable infrastructure — start a database, launch a service, tail a log file. Relux handles the lifecycle of those services automatically: when a test ends, it terminates all effect shells, which kills any processes running in them. You do not need to stop services yourself.

But services are not the only thing effects and tests create. A database effect might generate a data directory. A build effect might produce temporary files. A test might create artifacts that should not survive past the run. These leftovers are not tied to any shell — killing the shell does not clean them up.

Cleanup blocks solve this. They let you attach teardown commands to an effect or a test — commands that run after the test completes, regardless of whether it passed or failed. Their job is to remove temporary files, collect logs into an artifacts directory, or undo any filesystem side effects that setup left behind.

Here is an effect that creates a temporary working directory during setup and removes it during cleanup, using [`${__RELUX_RUN_ID}`](06-variables.md#relux-environment-variables) to ensure the directory is unique per test run:

```relux
effect TempWorkspace -> work {
    shell work {
        > mkdir -p /tmp/relux-${__RELUX_RUN_ID}
        match_ok()
        > cd /tmp/relux-${__RELUX_RUN_ID}
        match_ok()
    }
    cleanup {
        > rm -rf /tmp/relux-${__RELUX_RUN_ID}
    }
}
```

And here is a test with its own cleanup block:

```relux
test "test-level cleanup removes artifacts" {
    shell s {
        > touch /tmp/test-artifact-${__RELUX_RUN_ID}
        match_ok()
        > test -f /tmp/test-artifact-${__RELUX_RUN_ID} && echo "exists"
        <? ^exists$
    }
    cleanup {
        > rm -f /tmp/test-artifact-${__RELUX_RUN_ID}
    }
}
```

When the test finishes, Relux terminates all test and effect shells first — stopping any running processes — then spawns fresh cleanup shells to run the teardown commands. The syntax and behavior are the same in both cases.

## The cleanup block

A cleanup block goes inside an effect or test definition, after the shell blocks. It starts with the `cleanup` keyword followed by a body in braces. Each effect or test can have at most one cleanup block.

```relux
effect WithCleanup -> svc {
    shell svc {
        > touch /tmp/cleanup-test-marker
        match_ok()
    }
    cleanup {
        > rm -f /tmp/cleanup-test-marker
    }
}
```

Here, the effect creates a marker file during setup and removes it during cleanup.

## A fresh shell

Cleanup does not run in the effect's shell, or the test shell. Relux spawns a **new, implicit shell** dedicated to cleanup. This is a deliberate design choice: by the time cleanup runs, the original shells have already been terminated. Even if they were still around, they might be in an unpredictable state — a command may have crashed, a prompt may be missing, the buffer may contain unexpected output. A fresh shell sidesteps all of that. Cleanup starts from a clean slate every time.

This means you cannot rely on working directory changes or any shell-level state from the original shells. However, cleanup **does** have access to variables declared at the effect or test level with `let`, [overlay variables](13-effects-and-dependencies.md#overlay-variables) (for effects), and environment variables. If cleanup needs to know a path or a port number, declare it as a top-level `let` variable so both the shell blocks and the cleanup block can reference it.

## Allowed operations

Cleanup blocks support a restricted set of operations:

- **Send** (`>`) — send a command to the cleanup shell
- **Raw send** (`=>`) — send input without a trailing newline
- **Let** (`let`) — declare a [variable](06-variables.md)
- **Assignment** — reassign an existing variable

That is the complete list. These operations are enough to run teardown commands and organize them with local variables.

## What you cannot do

Cleanup blocks do not support [match operators](03-send-match-and-logs.md) (`<=`, `<?`), [function](08-functions.md) calls, [timeouts](09-timeouts.md), [fail patterns](10-fail-patterns.md), or buffer resets. This applies to both effect and test cleanup blocks. Relux enforces these restrictions at parse time — `relux check` rejects any cleanup block that uses a disallowed operation.

The reason is pragmatic: cleanup exists to run teardown after something has already gone wrong — a test failed, a timeout fired, a match never arrived. If cleanup itself could fail on a match, Relux would need to handle a failure during failure recovery. That is the classic panic-on-unwind problem: the teardown path must not introduce new failures, or the system becomes unpredictable. Restricting cleanup to fire-and-forget operations keeps the teardown path simple and reliable.

## Best-effort execution

Cleanup always runs, whether the test passed, failed, or timed out. And cleanup errors never change the test result. If a cleanup command fails — the file does not exist, the directory is already gone, the shell cannot start — Relux logs the issue but reports the test result based on the test body alone.

This means you do not need to worry about cleanup failures masking real test results or causing false negatives. A flaky teardown command will not turn a passing test red.

## Execution order

The full lifecycle of a test run is:

1. Effect setup (topological order)
2. Test body
3. Shell termination — all test shells, then all effect shells
4. **Test cleanup** (if present)
5. **Effect cleanup** (reverse topological order)

All shells are terminated before any cleanup runs. This guarantees that every process started during setup or the test body is dead before cleanup begins — cleanup only deals with what those processes left behind on the filesystem.

Test cleanup runs before effect cleanup. This way, if the test's teardown depends on files that effects created, those files are still present. Effect cleanup then unwinds in reverse topological order — dependents first, dependencies last.

Consider a chain of effects where each layer creates files that later layers depend on:

```relux
effect BuildApp -> build {
    shell build {
        > mkdir -p /tmp/build && echo "compiled" > /tmp/build/app.bin
        match_ok()
    }
    cleanup {
        > rm -rf /tmp/build
    }
}

effect GenerateConfig -> build {
    need BuildApp
    shell config {
        > echo "db=localhost" > /tmp/build/config.ini
        match_ok()
    }
    cleanup {
        > rm -f /tmp/build/config.ini
    }
}

effect DeployLocal -> build {
    need GenerateConfig
    shell deploy {
        > cp /tmp/build/app.bin /tmp/deploy/ && cp /tmp/build/config.ini /tmp/deploy/
        match_ok()
    }
    cleanup {
        > rm -rf /tmp/deploy
    }
}
```

Setup runs in dependency order:

1. `BuildApp` — create the build directory and binary
2. `GenerateConfig` — write a config file into the build directory
3. `DeployLocal` — copy artifacts to the deploy directory

Cleanup runs in the opposite direction:

1. `DeployLocal` cleanup — remove the deploy directory
2. `GenerateConfig` cleanup — remove the config file from the build directory
3. `BuildApp` cleanup — remove the entire build directory

This ordering matters. If `BuildApp` cleaned up first, it would delete `/tmp/build` — including the config file that `GenerateConfig`'s cleanup is about to target. Reverse topological order guarantees that each cleanup step runs while its dependencies' files still exist.

## Overlay variables in cleanup

As described above, cleanup can see top-level `let` variables and environment variables. For effects, there is an additional mechanism: overlay variables from the `need` site are also available in cleanup. This is useful when the cleanup needs to act on configuration that varies per instance:

```relux
effect TempDir -> work {
    shell work {
        > mkdir -p ${DIR}
        match_ok()
        > cd ${DIR}
        match_ok()
    }
    cleanup {
        > rm -rf ${DIR}
    }
}

test "temporary directory is cleaned up" {
    need TempDir as work {
        DIR = "/tmp/relux-test-workspace"
    }
    shell work {
        > touch testfile.txt
        match_ok()
    }
}
```

The `TempDir` effect uses `${DIR}` in both setup and cleanup. The value comes from the overlay at the `need` site. During cleanup, Relux interpolates `${DIR}` to `/tmp/relux-test-workspace`, so the `rm -rf` targets the right directory.

Overlays are the mechanism for making a single effect definition work across different configurations — the same `TempDir` effect with different `DIR` values creates and cleans up different directories. Test-level cleanup does not have overlay variables, but it can use top-level `let` variables and environment variables instead.

## Best practices

### Do not use cleanup to stop services

It is natural to think of cleanup as the place to stop a database or kill a service you started during setup. But Relux already handles this: when a test ends, it terminates all effect and test shells, which kills any processes running in them. Services started in a shell block die automatically with the shell — they are children of the PTY, so when Relux terminates the shell, the process goes with it. Even if Relux itself is killed, the OS cleans up the PTY and its children.

Using cleanup to stop services is actually worse than relying on shell termination. Cleanup runs in a **separate** shell — it has no connection to the process running in the effect's shell. If Relux crashes or is killed, cleanup never runs, and any service you expected cleanup to stop is left orphaned.

For the same reason, avoid starting daemonized or background services (processes that detach from the shell) during setup. A daemonized process is no longer a child of the PTY — it survives shell termination. If Relux is killed or terminated abnormally, neither shell termination nor cleanup can reach it, and it stays running indefinitely. Always run services in the foreground so they remain tied to the shell's lifecycle.

Reserve cleanup for things that shell termination does not handle: removing files, cleaning up directories, collecting logs, or any other filesystem side effects that outlive the shell.

### Keep cleanup self-contained

Cleanup can see top-level `let` variables, overlay variables (for effects), and environment variables — but it cannot see variables declared inside shell blocks or call functions. Shell-level `let` bindings and [regex captures](07-regex-matching.md) from the test body are not available.

Plan your cleanup around top-level variables. If a path or identifier is needed in both setup and cleanup, declare it with `let` at the effect or test level rather than inside a shell block.

### Make cleanup idempotent

Cleanup runs regardless of whether setup completed successfully. If an effect's shell block fails halfway through — the database started but the migration crashed — cleanup still runs. This means cleanup commands may encounter a partially initialized state: a file that was never created, a process that was never started, a directory that is already empty.

Write cleanup commands defensively. Assume nothing about what actually happened during setup — cleanup should be safe to run in any state, including when setup did nothing at all.

## Try it yourself

Take the two-effect dependency chain from the [previous article's challenge](13-effects-and-dependencies.md#try-it-yourself) and add cleanup:

1. Add a cleanup block to `StartDb` that removes the data directory it created during setup
2. Add a cleanup block to `Migrate` that removes any migration log files
3. Add a test-level cleanup block that removes any test-specific temporary files
4. Think about the execution order: which cleanup runs first? Verify your understanding matches the reverse topological rule

---

Next: [Condition Markers](15-condition-markers.md) — conditionally skip or run tests based on environment
