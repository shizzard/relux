# Stage: test-select

The server has resolved the project but no test has been selected yet. The client renders a file browser / test picker.

## State

Delivered via `session/init` response when the client connects during this stage.

```json
{
  "project": "my-suite",
  "tests": [
    {
      "file": "tests/basic.relux",
      "content": null,
      "definitions": [
        { "kind": "test", "name": "my first test", "startLine": 1, "endLine": 20 },
        { "kind": "test", "name": "my second test", "startLine": 22, "endLine": 45 }
      ]
    },
    {
      "file": "tests/deploy/smoke.relux",
      "content": null,
      "definitions": [
        { "kind": "test", "name": "deploy smoke test", "startLine": 1, "endLine": 42 }
      ]
    }
  ]
}
```

`project` — suite name from `Relux.toml` (or directory name).
`tests` — all test files with their source and test definitions. Each entry is a [SourceFile](00-common.md#sourcefile) with `file`, `content`, and `definitions`. The client can use this for search, filtering, source preview, and rendering the test picker.

## Commands

### `source/get`

Fetch source text for a file. Used when the client needs to display source for a file whose `content` was `null` in the state.

**Params:**

```json
{
  "file": "tests/basic.relux"
}
```

**Result:**

```json
{
  "file": "tests/basic.relux",
  "content": "test \"my first test\" {\n  ..."
}
```

### `test/select`

Select a test to debug. The server resolves the module graph and transitions to the `pre-run` stage.

**Params:**

```json
{
  "file": "tests/deploy/smoke.relux",
  "test": "deploy smoke test"
}
```

**Result:** `{}`

After the response, the server sends a [`stage/change`](index.md#event-stagechange) event with the new stage and its full state.

## Events

None specific to this stage.
