# Common Data Structures

Reusable structures referenced across multiple stages and messages.

## SourceFile

A file with its source text and a list of named definitions.

```json
{
  "file": "lib/helpers.relux",
  "content": "fn check_status() {\n  ...\n}\n\nfn build_url(host, port) {\n  ...\n}",
  "definitions": [
    { "kind": "function", "name": "check_status", "startLine": 1, "endLine": 8 },
    { "kind": "pure_function", "name": "build_url", "startLine": 10, "endLine": 15 }
  ]
}
```

`file` — path relative to the `relux/` directory.
`content` — full source text of the file, or `null` if not yet loaded. The client can fetch it on demand via `getSource`.
`definitions` — list of [Definition](#definition) objects in the file.

## Definition

A typed, named span within a source file.

```json
{ "kind": "function", "name": "check_status", "startLine": 1, "endLine": 8 }
```

`kind` — one of: `test`, `function`, `pure_function`, `effect`.
`name` — definition name.
`startLine` — first line of the definition (1-based, inclusive).
`endLine` — line after the last line of the definition (1-based, exclusive).

## Variable

A name-value pair.

```json
{ "name": "Db.port", "value": "5432" }
```

## CallFrame

A single frame in the call stack.

```json
{ "name": "check_status", "file": "lib/helpers.relux", "line": 3, "kind": "function" }
```

`name` — display name of the frame.
`file` — source file path (relative to `relux/` directory).
`line` — current line within the frame.
`kind` — one of: `test`, `shell`, `function`, `effect`, `cleanup`.

## Shell

A shell instance with its buffer and match/fail state.

```json
{
  "name": "s",
  "origin": "test",
  "active": true,
  "buffer": "$ echo hello\nhello\n$ ",
  "failPattern": null,
  "matchState": null
}
```

`name` — shell identifier.
`origin` — where the shell was created (e.g. `"test"`, `"effect:Database"`).
`active` — true for the shell currently executing DSL instructions.
`alias` — (optional) effect alias, present for effect-originated shells.
`buffer` — full PTY output buffer content.
`failPattern` — active fail pattern string, or `null`.
`matchState` — active [MatchState](#matchstate) object, or `null`.

## MatchState

State of an in-progress pattern match.

```json
{
  "pattern": "^Deploy complete",
  "isRegex": true,
  "elapsed": 2.3,
  "timeout": 10.0
}
```

`pattern` — the pattern being waited on.
`isRegex` — `true` for regex match, `false` for literal.
`elapsed` — seconds elapsed since match started.
`timeout` — total timeout in seconds.

When freeze mode is active, `elapsed` and `timeout` are omitted.

## Effect

An effect instance with lifecycle status.

```json
{
  "name": "Database",
  "alias": "Db",
  "status": "started",
  "shells": ["Db.__shell"],
  "vars": { "Db.port": "5432", "Db.host": "localhost" }
}
```

`name` — effect definition name.
`alias` — alias assigned in the `start` declaration.
`status` — one of: `starting`, `started`, `stopping`, `stopped`.
`shells` — shell names owned by this effect.
`vars` — exported variables as key-value pairs.

## EvalEntry

Structured evaluation trace for a single executed statement.

```json
{
  "index": 7,
  "file": "tests/basic.relux",
  "line": 12,
  "source": "> \"${cmd} --flag ${val}\"",
  "tree": [
    { "expr": "${cmd}", "value": "deploy" },
    { "expr": "${val}", "value": "prod" },
    { "resolved": "deploy --flag prod" }
  ]
}
```

`index` — monotonically increasing counter across the test run.
`file` — source file containing the statement.
`line` — line number of the statement.
`source` — original source text of the statement.
`tree` — evaluation steps. Each node is one of:
- `{ "expr": "<template>", "value": "<resolved>" }` — variable/capture resolution
- `{ "call": "<name>", "args": [...], "returned": "<value>" }` — function call
- `{ "resolved": "<final>" }` — final resolved value

## Breakpoint

A verified or unverified breakpoint.

```json
{ "line": 5, "verified": true }
```

`line` — line number.
`verified` — `true` if the line is actionable, `false` otherwise.
`message` — (optional) explanation when `verified` is `false`.

## Config

Manifest-derived configuration.

```json
{
  "shell": "/bin/sh",
  "prompt": "relux> ",
  "timeouts": {
    "match": 5.0,
    "test": 300.0,
    "suite": 600.0
  }
}
```

`shell` — shell command.
`prompt` — shell prompt string.
`timeouts` — timeout values in seconds: `match`, `test`, `suite`.
`timeoutMultiplier` — (optional, present in `pre-run` and `run`) effective timeout multiplier for debug mode.
