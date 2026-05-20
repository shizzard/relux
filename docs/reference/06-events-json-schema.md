# `events.json` Schema

Every test run writes a per-test `events.json` next to its `event.html`
under `relux/out/run-<timestamp>-<id>/logs/<test>/`. The file is the
canonical structured artifact: the viewer that ships in `event.html` is
one consumer, and downstream tooling (dashboards, CI integrations,
custom reporters) can read the same file.

This article describes the on-disk shape. The TypeScript declarations
shipped under `viewer/src/types/` are generated from the Rust source
via [`ts-rs`](https://docs.rs/ts-rs) and stay in sync; they are the
machine-readable equivalent of what this page describes in prose.

## Conventions

- **Tagged enums** carry their discriminator either as `"kind"`
  (`Span`, `Event`, `BufferEvent`, `TestOutcome`, `SpanKind`,
  `EventKind`, `BufferEventKind`) or as `"type"`
  (`CancelReasonRecord`, `TimeoutValue`, `FailureRecord`). The
  discriminator is always a kebab-case string. The remaining
  variant-specific fields are flattened alongside it.
- **Timestamps** (`ts`, `start_ts`, `end_ts`, `spawn_ts`,
  `terminate_ts`) are fractional milliseconds since test start,
  encoded as JSON numbers.
- **Durations exposed to JSON** are JSON numbers in milliseconds
  (`elapsed`, `duration`) unless they live inside a `TimeoutValue`,
  where they are pre-formatted humantime strings.
- **IDs** (`SpanId`, `EventSeq`) are 64-bit unsigned integers. JSON
  object keys (e.g. `spans`) are stringified per JSON rules; the
  generated TS types reflect this.
- **String values** are arbitrary UTF-8. Shell output is captured
  through a UTF-8 stream sanitizer.

## Top-level shape

```jsonc
{
  "schema_version": 1,
  "info":   { ... TestInfo ... },
  "outcome": { "kind": "pass" | "fail" | "cancelled" | "skip", ... },
  "env":    { "bootstrap": [["KEY", "value"], ...] },
  "shells": { "<shell-marker>": { ... ShellRecord ... }, ... },
  "spans":  { "<span-id>":     { ... Span ... },         ... },
  "events":        [ { ... Event ... },        ... ],
  "buffer_events": [ { ... BufferEvent ... },  ... ],
  "sources":   { "<relative-path>": "<file contents>", ... },
  "artifacts": [ { "path": "...", "size": 123, "mime": "..." }, ... ]
}
```

Field notes:

- `schema_version` (`u32`) — current version is `1`. Bumped on any
  backwards-incompatible change. Consumers should verify this matches
  the version they expect and fail loudly otherwise. The viewer
  rejects mismatched artifacts with a banner.
- `info` — `{ name, path, duration_ms }`. `path` is the source-relative
  path of the test file; `duration_ms` is the total wall-clock from
  test start to outcome.
- `env.bootstrap` — list of `(name, value)` env entries captured at
  test startup (the seed for the test's environment chain).
- `shells` — every PTY spawned during the test, keyed by stable
  identity marker (see [`ShellRecord`](#shells)).
- `spans` — every span opened during the test, keyed by `SpanId` (see
  [`Span`](#spans)). Forms a tree via `parent`.
- `events` — execution events in `seq` order (see [`Event`](#events)).
- `buffer_events` — parallel timeline of PTY-buffer transitions in
  `seq` order (see [`BufferEvent`](#buffer-events)).
- `sources` — `.relux` file contents referenced by any `Span.location`
  or `Event.source`. Keyed by relative path; only files actually
  referenced are embedded.
- `artifacts` — files written under the test's artifacts directory
  (see [`ArtifactEntry`](#artifacts)).

## Outcome

`outcome` is a tagged enum on `kind`:

| `kind`        | Extra fields                                    |
| ------------- | ----------------------------------------------- |
| `"pass"`      | none                                            |
| `"fail"`      | one of the four [`FailureRecord`](#failure) variants, flattened |
| `"cancelled"` | [`CancellationRecord`](#cancellation), flattened |
| `"skip"`      | [`SkipRecord`](#skip), flattened                |

The variant tag carried by `FailureRecord` lives on `type` (not
`kind`) to avoid colliding with the outer outcome tag — so a
`fail`-outcome payload looks like
`{ "kind": "fail", "type": "match-timeout", ... }`.

### Failure

`FailureRecord` is a tagged enum on `type`. All variants carry a
pre-computed `call_stack` (the active span stack at the failure site)
and `vars_in_scope`. Most variants also carry a `buffer_tail` (the
last bytes of the PTY buffer when the failure landed).

| `type`                  | Source of failure                                                        |
| ----------------------- | ------------------------------------------------------------------------ |
| `"match-timeout"`       | `match` exceeded its effective timeout                                   |
| `"fail-pattern-matched"`| an installed `fail` pattern matched a recv line                          |
| `"shell-exited"`        | the PTY shell died unexpectedly (carries `exit_code: i32 \| null`)       |
| `"runtime"`             | any other runtime error (carries `message`; `span`/`event_seq` optional) |

Each variant also carries the `span` and `event_seq` that pinpoint the
event-stream location of the failure.

### Cancellation

`CancellationRecord`:

```jsonc
{
  "reason": { ... CancelReasonRecord ... },
  "span": <span-id> | null,
  "event_seq": <seq> | null,
  "shell": "<name>" | null,
  "call_stack": [ { ... StackFrame ... }, ... ]
}
```

`CancelReasonRecord` is a tagged enum on `type`:

| `type`             | Extra fields                  |
| ------------------ | ----------------------------- |
| `"test-timeout"`   | `duration_ms`                 |
| `"suite-timeout"`  | `duration_ms`                 |
| `"fail-fast"`      | `trigger_test`                |
| `"sigint"`         | none                          |

### Skip

`SkipRecord` is a pointer into the in-stream marker evaluations:

```jsonc
{
  "span": <marker-eval span-id>,
  "event_seq": <bool-check event seq>,
  "marker_kind": "skip" | "run" | "flaky",
  "evaluation": { ... MarkerEvalDetail ... }
}
```

The viewer focuses these at open time and expands ancestors so the
markers tree is unfolded.

## Shells

Keyed by stable identity marker. Each entry:

```jsonc
{
  "marker": "<same as the map key>",
  "name": "<spawn-time bare name>",
  "spawn_ts": <ms>,
  "terminate_ts": <ms> | null,
  "command": "<the spawning shell command>"
}
```

The display layer renders qualified forms like `Db.inner` from events
(`ShellSwitch`, `EffectExposeShell`); the record itself holds the
bare name observed at spawn time.

## Spans

A span represents one bracketed region of execution. Spans nest via
`parent`. Each span:

```jsonc
{
  "id": <span-id>,
  "parent": <span-id> | null,
  "start_ts": <ms>,
  "end_ts": <ms> | null,
  "location": { ... SourceLocation ... } | null,
  "kind": "<one of the kinds below>",
  ...   // kind-specific fields, flattened
}
```

`SpanKind` is tagged on `kind`:

| `kind`             | Purpose                                                              |
| ------------------ | -------------------------------------------------------------------- |
| `"test"`           | Root span for the test body. `name`.                                 |
| `"effect-setup"`   | An effect being acquired. `effect`, `overlay`, `alias`, `marker`, `is_reuse`. The bootstrap span has `is_reuse: false`; dedup'd reuse spans have `is_reuse: true` and zero duration. |
| `"effect-cleanup"` | An effect being released. `effect`, `alias`, `setup_span`, `marker`, `is_deferred`. Parented under the test, not the long-closed setup; `setup_span` back-references its pair. |
| `"shell-block"`    | A `shell <name>` block. `shell`.                                     |
| `"cleanup-block"`  | A `cleanup` block. No payload.                                       |
| `"fn-call"`        | A function call (user or BIF). `name`, `args`, `result`, `callee_kind` (`"user" \| "bif"`), `is_pure`. |
| `"markers"`        | Synthetic root grouping per-test marker evaluations.                 |
| `"marker-eval"`    | One marker evaluation under `markers`. `marker_kind`, `modifier` (`"if" \| "unless"`), `decision` (`"pass" \| "mark"`). |

## Events

An event is a point-in-time observation made by the VM. Events are
emitted in monotonic `seq` order. Each event carries the span it
landed on, the shell it acted on (when applicable), and a source
location resolving against `sources`. The common envelope:

```jsonc
{
  "seq": <u64>,
  "ts": <ms>,
  "span": <span-id>,
  "shell": "<display name>" | null,
  "shell_marker": "<shell map key>" | null,
  "source": { ... SourceLocation ... } | null,
  "kind": "<one of the kinds below>",
  ...   // kind-specific fields, flattened
}
```

`shell` and `shell_marker` are present iff a shell was in scope at
the emit site; `shell_marker` is the stable identity, `shell` is the
display name at that moment.

`EventKind` is tagged on `kind`. The variants, grouped by concern:

**Shell lifecycle**

| `kind`                  | Extra fields              |
| ----------------------- | ------------------------- |
| `"shell-spawn"`         | `name`, `command`         |
| `"shell-ready"`         | `name`                    |
| `"shell-switch"`        | `name`                    |
| `"shell-terminate"`     | `name`                    |
| `"effect-expose-shell"` | `name`, `target`, `qualifier` |
| `"effect-expose-var"`   | `name`, `target`, `qualifier`, `value` |

**I/O**

| `kind`     | Extra fields |
| ---------- | ------------ |
| `"send"`   | `data`       |
| `"recv"`   | `data`       |

**Matching** (`buffer_seq` cross-references a `buffer_events` entry)

| `kind`          | Extra fields                                                                                 |
| --------------- | -------------------------------------------------------------------------------------------- |
| `"match-start"` | `pattern`, `is_regex`, `effective` (a [`TimeoutValue`](#timeoutvalue))                       |
| `"match-done"`  | `matched`, `elapsed` (ms), `captures: { [name]: string } \| null`, `buffer_seq`              |
| `"timeout"`     | `pattern`, `buffer_seq: u64 \| null`, `effective`. `buffer_seq` is null when no buffer event corresponds (the failure record's `buffer_tail` is canonical in that case). |

**Fail patterns**

| `kind`                     | Extra fields                                       |
| -------------------------- | -------------------------------------------------- |
| `"fail-pattern-set"`       | `pattern`, `is_regex`                              |
| `"fail-pattern-cleared"`   | none                                               |
| `"fail-pattern-triggered"` | `pattern`, `is_regex`, `matched_line`, `buffer_seq: u64 \| null` (null because fail-pattern hits observe without advancing the cursor) |

**Control flow**

| `kind`          | Extra fields                                  |
| --------------- | --------------------------------------------- |
| `"sleep-start"` | `duration` (ms)                               |
| `"sleep-done"`  | none                                          |
| `"timeout-set"` | `timeout`, `previous` (both `TimeoutValue`)   |

**Values**

| `kind`            | Extra fields                                                                 |
| ----------------- | ---------------------------------------------------------------------------- |
| `"var-let"`       | `name`, `value`                                                              |
| `"var-assign"`    | `name`, `value`, `previous`                                                  |
| `"var-read"`      | `name`, `value` (`""` when undefined)                                        |
| `"string-eval"`   | `result`                                                                     |
| `"interpolation"` | `template`, `result`, `bindings: Array<[name, value]>`                       |
| `"pure-match"`    | `match_kind` (`"regex" \| "literal"`), `value`, `pattern`, `result` (matched substring or `""`), `captures: { [name]: string }` |
| `"bool-check"`    | `evaluation: MarkerEvalDetail`. Emitted as the last event inside a `marker-eval` span. |

`MarkerEvalDetail` is tagged on `shape`: `"unconditional"`,
`"bare" + { value, met }`, `"eq" + { lhs, rhs, met }`, or
`"regex" + { value, pattern, met }`.

**Diagnostics**

| `kind`        | Extra fields |
| ------------- | ------------ |
| `"annotate"`  | `text`       |
| `"log"`       | `message`    |
| `"warning"`   | `message`    |
| `"error"`     | `message`    |

**Cancellation**

| `kind`        | Extra fields                                                |
| ------------- | ----------------------------------------------------------- |
| `"cancelled"` | `reason: CancelReasonRecord`. Emitted on the span the VM was inside when it observed cancellation. |

## Buffer events

A parallel timeline tracking transitions of each shell's PTY output
buffer. Buffer events always carry a shell. The common envelope:

```jsonc
{
  "seq": <u64>,
  "ts": <ms>,
  "shell": "<display name>",
  "shell_marker": "<shell map key>",
  "kind": "<one of the kinds below>",
  ...   // kind-specific fields, flattened
}
```

`BufferEventKind` is tagged on `kind`:

| `kind`      | Extra fields                       | Meaning                                                                                  |
| ----------- | ---------------------------------- | ---------------------------------------------------------------------------------------- |
| `"grew"`    | `data`                             | New bytes appended to the buffer.                                                        |
| `"matched"` | `before`, `matched`, `after`       | A `match` consumed the cursor up through `matched`; `before` is what preceded, `after` is what now remains. |
| `"reset"`   | `consumed`                         | The buffer was reset (e.g. cleared between shell blocks); `consumed` is what got dropped. |

## Stack frames

`StackFrame` (used in `FailureRecord.call_stack` and
`CancellationRecord.call_stack`):

```jsonc
{
  "span": <span-id>,
  "kind": "<span kind discriminator>",
  "name": "<fn or effect name>" | null,
  "args": [["name", "value"], ...],
  "alias": "<user-supplied alias>" | null,
  "location": { ... SourceLocation } | null
}
```

`kind` mirrors the span's `SpanKind` discriminator (e.g.
`"fn-call"`, `"shell-block"`). `alias` is the user-supplied
`start FX as Alias` binding when present; only effect-setup /
effect-cleanup frames carry one today.

## `TimeoutValue`

```jsonc
// Either:
{ "type": "tolerance",
  "duration": "5s",            // humantime-formatted
  "multiplier": "1.0",
  "total_duration": "5s",
  "source": { ... SourceLocation } | null }
// or:
{ "type": "assertion",
  "duration": "30s",
  "source": { ... SourceLocation } | null }
```

`tolerance` is the soft kind that scales with `--timeout-multiplier`;
`assertion` is the hard kind that does not. All three duration fields
are humantime strings — consumers should display them verbatim
rather than re-parsing.

## `SourceLocation`

```jsonc
{
  "file": "<relative path; matches a key in `sources`>",
  "line": <1-based line number>,
  "start": <byte offset into the source>,
  "end":   <byte offset into the source>
}
```

`start`/`end` resolve against `sources[file]`.

## Artifacts

```jsonc
{
  "path": "<forward-slash relative path>",
  "size": <bytes>,
  "mime": "<mime/type>" | null
}
```

`path` never starts with `/` and never contains `.` / `..` segments.
The list is sorted with files preceding subdirectory contents at each
level (`cmp_artifact_paths`). `mime` is derived from the extension
via `mime_guess`; the browser does its own sniffing on click.

## Versioning

`schema_version` is currently `1`. A future change that adds new
optional fields or new tagged-enum variants is *not* a breaking
change and does not bump the version; consumers should ignore unknown
variants gracefully. Any change that removes or renames fields, or
narrows the meaning of an existing field, bumps the version.

To regenerate the TypeScript bindings after editing the Rust types,
run `just viewer-build` — it runs the `ts-rs` export tests and then
rebuilds the viewer bundle.
