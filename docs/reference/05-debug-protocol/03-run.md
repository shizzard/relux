# Stage: run

The test is executing. The client renders the full debugger UI.

## State

Delivered via `session/hello` state when the client connects during this stage.

```json
{
  "test": {
    "name": "deploy smoke test",
    "file": "tests/deploy/smoke.relux",
    "startLine": 1,
    "endLine": 42
  },
  "status": "stopped",
  "stopReason": "breakpoint",
  "position": {
    "file": "tests/deploy/smoke.relux",
    "line": 12
  },
  "callstack": [
    { "name": "check_status", "file": "lib/helpers.relux", "line": 3, "kind": "function" },
    { "name": "shell s", "file": "tests/deploy/smoke.relux", "line": 8, "kind": "shell" },
    { "name": "deploy smoke test", "file": "tests/deploy/smoke.relux", "line": 1, "kind": "test" }
  ],
  "shells": [
    {
      "name": "s",
      "origin": "test",
      "active": true,
      "buffer": "$ echo hello\nhello\n$ ",
      "failPattern": null,
      "matchState": null
    },
    {
      "name": "Db.__shell",
      "origin": "effect:Database",
      "active": false,
      "alias": "Db",
      "buffer": "$ pg_isready\n/tmp:5432 - accepting connections\n$ ",
      "failPattern": "FATAL",
      "matchState": null
    }
  ],
  "effects": [
    {
      "name": "Database",
      "alias": "Db",
      "status": "started",
      "vars": { "Db.port": "5432", "Db.host": "localhost" }
    }
  ],
  "variables": {
    "test": [
      { "name": "url", "value": "http://localhost:8080" },
      { "name": "Db.port", "value": "5432" }
    ],
    "shell": [
      { "name": "response", "value": "200 OK" }
    ],
    "captures": [
      { "name": "$1", "value": "200" }
    ]
  },
  "breakpoints": {
    "tests/deploy/smoke.relux": [5, 12],
    "lib/helpers.relux": [3]
  },
  "frozen": false,
  "evalLog": [
    {
      "index": 0,
      "file": "tests/deploy/smoke.relux",
      "line": 5,
      "source": "> echo hello",
      "tree": [{ "resolved": "echo hello" }]
    },
    {
      "index": 1,
      "file": "tests/deploy/smoke.relux",
      "line": 6,
      "source": "<? ^hello$",
      "tree": [{ "resolved": "^hello$" }]
    }
  ]
}
```

`status` — one of: `running`, `stopped`, `matching`.
`stopReason` — present when `status` is `stopped`. One of: `entry`, `breakpoint`, `step`, `pause`.
`position` — current execution position. Present when `status` is `stopped`.
`callstack` — call stack, top of stack at index 0. Present when `status` is `stopped`.
`shells` — all shells with their full buffer contents and current match/fail state. The `matchState` object is present when that shell is actively waiting on a pattern match:

```json
{
  "pattern": "^Deploy complete",
  "isRegex": true,
  "elapsed": 2.3,
  "timeout": 10.0
}
```

`effects` — all effects with lifecycle status and exported variables.
`variables` — variables organized by scope. Only the populated scopes are present.
`breakpoints` — all breakpoints across all files.
`frozen` — freeze mode state.
`evalLog` — complete evaluation log accumulated so far (ordered by `index`).

When `status` is `matching`, `position` points to the match statement, and the active shell's `matchState` is non-null.

When `status` is `running`, `position`, `callstack`, and scope-level `variables` are absent (the VM is between statements).

## Commands

TODO.

## Events

TODO.
