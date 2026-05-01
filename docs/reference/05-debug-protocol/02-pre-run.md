# Stage: pre-run

A test has been selected. The server has resolved the full module graph. The client renders the source viewer with breakpoint support.

## State

Delivered via `session/init` response when the client connects during this stage.

```json
{
  "source": {
    "test": {
      "filename": "tests/deploy/smoke.relux",
      "content": "test \"deploy smoke test\" {\n  shell s {\n    ...",
      "definitions": [
        { "kind": "test", "name": "deploy smoke test", "startLine": 1, "endLine": 42 }
      ]
    },
    "functions": [
      {
        "filename": "lib/helpers.relux",
        "content": "fn check_status() {\n  ...\n}\n\nfn build_url(host, port) {\n  ...\n}",
        "definitions": [
          { "kind": "function", "name": "check_status", "startLine": 1, "endLine": 8 },
          { "kind": "pure_function", "name": "build_url", "startLine": 10, "endLine": 15 }
        ]
      }
    ],
    "effects": [
      {
        "filename": "lib/effects/database.relux",
        "content": "effect Database -> shell db {\n  ...\n}",
        "definitions": [
          { "kind": "effect", "name": "Database", "startLine": 1, "endLine": 30 }
        ]
      }
    ]
  },
  "env": {
    "__RELUX_SHELL_PROMPT": "relux> ",
    "__RELUX_SUITE_ROOT": "/home/user/projects/my-suite",
    "__RELUX": "/usr/local/bin/relux",
    "HOME": "/home/user",
    "PATH": "/usr/bin:/bin"
  },
  "config": {
    "shell": "/bin/sh",
    "prompt": "relux> ",
    "timeouts": {
      "match": "5s",
      "test": "5m",
      "suite": "10m"
    },
    "timeoutMultiplier": 10.0
  },
  "breakpoints": {
    "tests/deploy/smoke.relux": [{ "line": 5 }, { "line": 12 }],
    "lib/helpers.relux": [{ "line": 3 }]
  },
  "frozen": false
}
```

`source` — the resolved source graph for this test. Each value is a `{ filename, content, definitions }` object (or array of them), where each definition has `name`, `startLine`, `endLine`.
- `test` — single object for the file containing the selected test. Definitions list includes only the selected test (other tests in the same file are not included).
- `functions` — array of files containing function definitions reachable from this test. Each file's `definitions` list contains only the actually-reachable functions (pure or impure). Files holding no reachable functions are omitted.
- `effects` — array of files containing effect definitions reachable from this test. Each file's `definitions` list contains only the actually-reachable effects. Files holding no reachable effects are omitted.

A file may appear in both `functions` and `effects` if it contains reachable definitions of both kinds.

`env` — JSON object mapping name → value, holding env vars visible to the test. Includes inherited process env plus the run-stable relux internals (`__RELUX_SHELL_PROMPT`, `__RELUX_SUITE_ROOT`, `__RELUX`). Per-run / per-test internals (`__RELUX_RUN_ID`, `__RELUX_RUN_ARTIFACTS`, `__RELUX_TEST_ROOT`, `__RELUX_TEST_ARTIFACTS`) materialize at the execution stage and are not present here.
`config` — manifest-derived runtime configuration, plus the effective `timeoutMultiplier` for debug mode. Timeout values are humantime-format strings (e.g. `"5s"`, `"1m 30s"`). See [Config](00-common.md#config).
`breakpoints` — currently set breakpoints, keyed by suite-relative filename. Each value is an array of [Breakpoint](00-common.md#breakpoint) objects, sorted by line. Empty object `{}` if none set. See [Breakpointable lines](00-common.md#breakpointable-lines) for which lines are valid placements.
`frozen` — whether freeze mode is active. _Not yet emitted by the server — deferred._

## Commands

### `breakpoint/set`

Set a breakpoint at `(filename, line)`. Idempotent — setting an already-set breakpoint succeeds.

**Request params:**

```json
{ "filename": "tests/deploy/smoke.relux", "line": 5 }
```

`filename` — suite-relative path, must appear in `pre_run.source` (test, functions, or effects bucket).
`line` — 1-based line number; must be a [breakpointable line](00-common.md#breakpointable-lines).

**Response:**

```json
{ "breakpoint": { "line": 5 } }
```

**Errors:**

- `BREAKPOINT_INVALID` (-8) — `(filename, line)` is not a breakpointable position.
- `Invalid Request` (-32600) — no test selected (no prior `test/select`).

### `breakpoint/unset`

Unset a breakpoint at `(filename, line)`. Idempotent — unsetting a non-existent breakpoint succeeds.

**Request params:**

```json
{ "filename": "tests/deploy/smoke.relux", "line": 5 }
```

**Response:**

```json
{}
```

**Errors:**

- `Invalid Request` (-32600) — no test selected.

### `breakpoint/reset`

Clear all breakpoints in the current pre-run. Idempotent — resetting an empty set succeeds.

**Request params:** none (`{}`).

**Response:**

```json
{}
```

**Errors:**

- `Invalid Request` (-32600) — no test selected.

### `breakpoint/list`

Return all currently set breakpoints, in the same shape as `PreRunState.breakpoints`.

**Request params:** none (`{}`).

**Response:**

```json
{
  "breakpoints": {
    "tests/deploy/smoke.relux": [{ "line": 5 }, { "line": 12 }],
    "lib/helpers.relux": [{ "line": 3 }]
  }
}
```

**Errors:**

- `Invalid Request` (-32600) — no test selected.

## Events

`stage-change` — emitted when the active stage changes. The payload's `state.breakpoints` reflects whatever was preserved or reset by the transition (re-selecting the same test preserves; selecting a different test clears).
