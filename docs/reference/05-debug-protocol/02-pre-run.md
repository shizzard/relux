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
  "env": [
    { "name": "__RELUX_RUN_ID", "value": "abc123" },
    { "name": "__RELUX_SHELL_PROMPT", "value": "relux> " },
    { "name": "HOME", "value": "/home/user" }
  ],
  "config": {
    "shell": "/bin/sh",
    "prompt": "relux> ",
    "timeouts": {
      "match": 5.0,
      "test": 300.0,
      "suite": 600.0
    },
    "timeoutMultiplier": 10.0
  },
  "breakpoints": {
    "tests/deploy/smoke.relux": [5, 12],
    "lib/helpers.relux": [3]
  },
  "frozen": false
}
```

`source` — the resolved source graph for this test. Each value is a `{ filename, content, definitions }` object (or array of them), where each definition has `name`, `startLine`, `endLine`.
- `test` — single object for the file containing the selected test.
- `functions` — array of files containing function definitions reachable from this test. Includes both pure and impure functions.
- `effects` — array of files containing effect definitions reachable from this test.

`env` — environment variables that will be visible to the test (relux internal vars + inherited process env).
`config` — same as `test-select` plus the effective `timeoutMultiplier` for debug mode.
`breakpoints` — currently set breakpoints, keyed by file. Empty object `{}` if none set.
`frozen` — whether freeze mode is active.

## Commands

TODO.

## Events

TODO.
