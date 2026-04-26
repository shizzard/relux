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
      "filename": "lib/helpers.relux",
      "content": null,
      "definitions": [
        { "kind": "function", "name": "check_status", "startLine": 1, "endLine": 8 },
        { "kind": "pure_function", "name": "build_url", "startLine": 10, "endLine": 15 }
      ]
    },
    {
      "filename": "lib/effects/database.relux",
      "content": null,
      "definitions": [
        { "kind": "effect", "name": "Database", "startLine": 1, "endLine": 30 }
      ]
    }
  ]
}
```

`project` — suite name from `Relux.toml` (or directory name).
`files` — every loaded source file (test files plus reachable lib/effect files), each a [SourceFile](00-common.md#sourcefile) with `filename`, `content` (always `null` here), and `definitions`. The client uses this for search, filtering, source preview, and rendering the test picker.

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

After the response, the server sends a [`stage/change`](index.md#event-stagechange) event with the new stage and its full state.

## Events

None specific to this stage.
