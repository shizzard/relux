# Relux Debug Protocol (RDP)

Companion to R012. Defines the wire protocol between the relux debug server (embedded in the binary) and the browser-based debug frontend.

## Wire Format

JSON-RPC 2.0 over WebSocket. WebSocket handles message framing (no Content-Length header needed). All messages are UTF-8 JSON objects.

Three message kinds:

| Kind | Direction | Has `id`? | Expects reply? |
|------|-----------|-----------|----------------|
| Command | client → server | yes | yes |
| Response | server → client | yes (echoes command `id`) | no |
| Event | server → client | no | no |

## Method Naming

Method names follow the format `subject/action`, in snake_case:

- **subject** — the entity being acted on (e.g. `execution`, `breakpoint`, `shell`)
- **action** — the operation (e.g. `start`, `set`, `get`, `list`)

Compound subjects use `_` within the subject part: `shell_buffer/get`.

Commands and events share the same namespace. They are distinguished by message framing (commands carry an `id`, events do not), not by name. The same method name may appear as both a command and an event (e.g. `execution/continue`).

## Session Stages

The debug session progresses through four stages. A client may connect (or reconnect) at any stage — the `session/init` response always delivers the full current state.

```
  test-select ──► pre-run ──► run ──► post-run
       │              │          │        │
       └──────────────┴──────────┴────────┴──── client may connect at any point
```

| Stage | Purpose | Enters when | Spec |
|-------|---------|-------------|------|
| `test-select` | Browse files, pick a test | Server starts | [01-test-select.md](01-test-select.md) |
| `pre-run` | Set breakpoints, browse source, inspect resolved graph | Test selected | [02-pre-run.md](02-pre-run.md) |
| `run` | Live debugging: step, inspect, stream buffers | `run` command sent | [03-run.md](03-run.md) |
| `post-run` | Review test outcome | Test execution completes or fails | [04-post-run.md](04-post-run.md) |

Common data structures shared across stages are defined in [00-common.md](00-common.md).

## Handshake

After connecting, the client sends a `session/init` command. The server validates the client version and responds with the current session state. The `state` object carries its own `stage` discriminant, so the client reads `state.stage` to determine which UI to render and uses the remaining fields to reconstruct the full view.

### `session/init`

Initialize the debug session. Valid in any stage.

**Params:**

```json
{
  "client": "rdp-client",
  "version": "0.5.0"
}
```

`client` — client name (free-form string).
`version` — client version. Must match the server version exactly.

**Result:**

```json
{
  "server": "relux",
  "version": "0.5.0",
  "state": {
    "stage": "<test-select | pre-run | run | post-run>",
    ...
  }
}
```

`server` — always `"relux"`.
`version` — server version (semver).
`state` — stage-specific state snapshot. Contains a `stage` field indicating the current session stage, plus stage-specific fields (see stage docs).

Returns error code `-6` if the client version does not match the server version.

## Cross-Stage Commands

### `session/disconnect`

Terminate the debug session. Valid in any stage. The server kills all shells and exits.

**Params:** none

**Result:** `{}`

## Cross-Stage Events

### Event: `stage/change`

The server has transitioned to a new stage. Sent after a stage-transitioning command (e.g. `test/select`, `execution/start`) has been acknowledged. The client should switch its UI to the new stage using the provided state.

```json
{
  "state": {
    "stage": "<test-select | pre-run | run | post-run>",
    ...
  }
}
```

`state` — full state snapshot for the new stage (same shape as in `session/init`). The `stage` field is embedded in the state object.

## Error Codes

| Code | Meaning |
|------|---------|
| -1   | Generic error |
| -2   | File not found |
| -3   | Not stopped (inspection command sent while running) |
| -4   | Not running (execution command sent while stopped, except during pre-run) |
| -5   | Already in execution phase (second `execution/start` command) |
| -6   | Version mismatch (client version does not match server version) |

## Design Notes

**Why full-replace for breakpoints?** DAP's `breakpoint/set` replaces all breakpoints per file in one call. This avoids add/remove races and makes the client the single source of truth. The client tracks its own breakpoint set and always sends the full list.

**Why `shell_buffer/get` instead of reconstructing from `buffer/data` events?** The client may connect late, miss early output, or want to switch shell views. `shell_buffer/get` provides the full current state on demand. `buffer/data` events supplement it with live streaming.

**Why `match/state` as a periodic event?** The match operation is atomic from the debugger's perspective (can't step inside a regex wait). The periodic event gives the user visibility into long-running matches without breaking the step model.

**Why `eval/entry` is separate from `execution/stop`?** Eval entries accumulate even during `execution/continue` (they form the scrollable log). Bundling them into `execution/stop` would lose entries for lines that didn't hit breakpoints. Sending them as separate events means the frontend can build the complete log regardless of stop frequency.
