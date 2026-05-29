# events.json Schema

The shape of `events.json` -- one file per test under `relux/out/<run>/logs/<test>/events.json`.

## Top-level fields

| Field | Type | Notes |
|---|---|---|
| `schema_version` | number | Bump on incompatible changes. Current: 1. |
| `info` | `TestInfo` | `{ name, path, duration_ms }`. |
| `outcome` | `TestOutcome` | Tagged: `{ kind: "pass" \| "fail" \| "cancelled" \| "skip", ... }`. |
| `env` | `EnvInfo` | `{ bootstrap: [[name, value], ...] }`. |
| `shells` | `Record<string, ShellRecord>` | Keyed by shell marker. |
| `spans` | `Record<string, Span>` | Keyed by `SpanId` as string. |
| `events` | `Event[]` | Flat list, ordered by `seq`. |
| `buffer_events` | `BufferEvent[]` | Flat top-level list, ordered by `seq`. Filter by `shell` / `shell_marker`. |
| `sources` | `Record<string, string>` | `.relux` file contents referenced by spans/events. |
| `artifacts` | `ArtifactEntry[]` | Files in the test's `artifacts/` dir. |

## Outcome shape

```jsonc
{ "kind": "pass" }
{ "kind": "fail", "type": "match-timeout", "span": ..., "event_seq": ..., "buffer_tail": "...", "call_stack": [...], "vars_in_scope": [...], "effective": {...}, ... }
{ "kind": "cancelled", "reason": { "type": "test-timeout", "duration_ms": ... }, "span": ..., "call_stack": [...] }
{ "kind": "skip", ... }   // SkipRecord
```

See [events-failures](events-failures.md) for failure/cancellation/skip details.

## Span

Common fields on every span:

| Field | Notes |
|---|---|
| `id` | `SpanId` (number). |
| `parent` | `SpanId \| null`. Root spans have `null`. |
| `kind` | Tagged discriminator (see table below). |
| `start_ts`, `end_ts` | Milliseconds since test start (numbers). `end_ts` is null on open spans. |
| `location` | Optional `{ file, line, start, end }`. |

Kind-specific fields:

| `kind` | Extra fields |
|---|---|
| `test` | `name`. |
| `effect-setup` | `effect`, `overlay` (`[name, value][]`), `alias` (nullable), `marker` (identity hash; same on every setup for the same instance), `is_reuse` (`false` on the bootstrap, `true` on dedup'd zero-duration acquires). |
| `effect-cleanup` | `effect`, `alias`, `setup_span` (back-ref to the paired setup so consumers can resolve the effect's scope), `marker`, `is_deferred` (`false` on the final-release that runs the body; `true` on non-last releases). |
| `shell-block` | `shell` (display name; matches the `shell` field on events emitted under this span). |
| `multi-match` | `shell` (same shape as `shell-block`; opens at `<{` and closes at `multi-match-done` / `multi-match-timeout`). |
| `cleanup-block` | -- |
| `fn-call` | `name`, `args` (`[name, value][]`), `result` (nullable; populated on return), `callee_kind` (`"user"` for user `fn` / `pure fn`, `"bif"` for built-ins), `is_pure` (bool). |
| `markers` | -- (synthetic per-test root grouping `marker-eval` children). |
| `marker-eval` | `marker_kind` (`skip` / `run` / `flaky`), `modifier` (`unconditional` / `if` / `unless`), `decision` (`pass` -- the marker's action did not apply / `mark` -- the marker's action applied; what the action means depends on `marker_kind`, see [events-failures](events-failures.md) > *SkipRecord*). |

## Event

| Field | Notes |
|---|---|
| `seq` | Monotonic order (number). Canonical sort key. |
| `ts` | Milliseconds since test start. |
| `span` | Owning `SpanId`. |
| `shell` | Display name (may be qualified `Alias.name`) or null. |
| `shell_marker` | Stable identity; matches `BufferEvent.shell_marker`. |
| `source` | Optional `SourceLocation`. |
| `kind` | Tagged discriminator. |

### Event kinds

- Shell lifecycle: `shell-spawn`, `shell-ready`, `shell-switch`, `shell-terminate`.
- Effect interface: `effect-expose-shell`, `effect-expose-var`.
- I/O: `send`, `recv`.
- Matching: `match-start`, `match-done`, `timeout`.
- Multimatch: `multi-match-start`, `multi-match-pattern-done`, `multi-match-done`, `multi-match-timeout`.
- Fail patterns: `fail-pattern-set`, `fail-pattern-cleared`, `fail-pattern-triggered`.
- Control: `sleep-start`, `sleep-done`, `timeout-set`.
- Values: `var-let`, `var-assign`, `var-read`, `interpolation`, `string-eval`, `pure-match`.
- Markers: `bool-check` (final truthy/falsy of a marker condition).
- Diagnostics: `annotate`, `log`, `warning`, `error`.
- Cancellation: `cancelled` (carries `reason`).

## Buffer events

Top-level `buffer_events` array. Each carries `seq`, `ts`, `shell`, `shell_marker`, plus a `kind`-tagged payload:

| `kind` | Payload |
|---|---|
| `grew` | `{ data: string }` -- new bytes from the PTY. |
| `matched` | `{ before, matched, after }` -- recorded when a `<?`/`<=` (or per-pattern in multimatch) succeeds. |
| `reset` | `{ consumed: string }` -- recorded on bare `<?`/`<=`. |

Buffer events share the same `seq` space as `events` -- both can be merged into one chronological stream by `seq`.

## Shells

`shells[marker]` carries per-shell metadata. The `shell_marker` field on events / buffer events is the canonical key (the display `shell` may change across the test if shells are renamed via export).

## Artifacts

`artifacts[]` is the list of files under the test's artifact directory: `{ path, size, ... }`. Symlinks are skipped. Sorted with files-before-subdirs within each directory.

## Pitfalls and best practices

### `seq` is the canonical order; don't sort by `ts`

`ts` is coarse and may tie. `seq` is monotonic and total.

Don't:

```bash
jq '.events | sort_by(.ts)' events.json
```

Do:

```bash
jq '.events | sort_by(.seq)' events.json
```

### Buffer events are top-level, not nested in shells

Filter by `shell_marker` (or `shell`) to get a shell's stream.

Don't:

```bash
jq '.shells["s"].buffer_events' events.json   # field does not exist
```

Do:

```bash
jq '[.buffer_events[] | select(.shell_marker == "s")]' events.json
```

### `outcome` is a single tagged enum; there is no separate `report` or `failure` field

The earlier shape had `report.outcome` + `report.failure`; current shape collapses both into `outcome` with `kind`-tagged variants.

Don't:

```bash
jq '.report.failure' events.json
```

Do:

```bash
jq '.outcome | select(.kind == "fail")' events.json
```

## See also

- [events-failures](events-failures.md) -- read if you need fail/cancel/skip outcome shapes
- [events-recipes](events-recipes.md) -- read if you need ready-made queries against `events.json`
- [viewer](viewer.md) -- read if you need to hand the user a link to the visual surface over the same data
