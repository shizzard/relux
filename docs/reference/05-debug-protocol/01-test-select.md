# Stage: test-select

The server has resolved the project but no test has been selected yet. The client renders a file browser / test picker.

## State

Delivered via `session/init` response when the client connects during this stage.

```json
{
  "stage": "test-select",
  "project": "my-suite",
  "files": [
    {
      "filename": "tests/basic.relux",
      "content": null,
      "definitions": [
        { "kind": "test", "name": "my first test", "startLine": 1, "endLine": 20 },
        { "kind": "test", "name": "my second test", "startLine": 22, "endLine": 45 }
      ]
    },
    {
      "filename": "tests/deploy/smoke.relux",
      "content": null,
      "definitions": [
        { "kind": "test", "name": "deploy smoke test", "startLine": 1, "endLine": 42 }
      ]
    }
  ]
}
```

`project` — suite name from `Relux.toml` (or directory name).
`files` — files containing tests, each a [SourceFile](00-common.md#sourcefile) with `filename`, `content` (always `null` here), and `definitions` of `kind: "test"`. The client uses this to render the test picker. Reachable lib/effect files are not included at this stage — they're delivered in [pre-run state](02-pre-run.md), scoped to what the selected test actually uses.

## Commands

### `source/get`

Fetch source text for a file. Used when the client needs to display source for a file whose `content` was `null` in the state.

**Params:**

```json
{
  "filename": "tests/basic.relux"
}
```

**Result:**

```json
{
  "filename": "tests/basic.relux",
  "content": "test \"my first test\" {\n  ..."
}
```

Returns error code `-2` if the file is not in the loaded source table.

### `test/select`

Select a test to debug. The server resolves the module graph and transitions to the `pre-run` stage.

**Params:**

```json
{
  "filename": "tests/deploy/smoke.relux",
  "test": "deploy smoke test"
}
```

**Result:** `{}`

After the response, the server sends a [`stage-change`](index.md#event-stage-change) event with the new stage and its full state.

Returns error code `-7` if the test name does not resolve to a runnable plan in the named file.

## Events

None specific to this stage.
