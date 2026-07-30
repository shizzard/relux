# events.json Recipes

Operational queries against `events.json`. Verify field names against [events-schema](events-schema.md) before scripting -- the schema evolves.

The non-trivial queries (variable scope reconstruction, span subtree walks, multi-pass merges) are implemented once in `tools/events.py`. Skills point at the subcommand; the script owns the algorithm. Trivial one-line `jq` filters remain inline where they are clearer than a shelled subcommand.

## Cross-cutting tips

- Events are append-only; `seq` is the canonical order.
- `parent_span_id` (`span.parent`) is the only structural link between spans.
- Buffer events are top-level; filter by `shell_marker` (stable) or `shell` (display name).
- `outcome` is a single tagged enum at top level -- there is no `report` field.
- Pre-formatted durations live on TimeoutValue (`duration`, `total_duration`) -- no arithmetic needed.

## Using the CLI

```
python3 <plugin>/tools/events.py [--events PATH] <subcommand> [...]
```

`--events` defaults to `events.json` in the current directory; `cd` into the per-test log directory or pass `--events` explicitly.

### Locating the tool

`<plugin>` is the directory containing this `references/` folder and the sibling `tools/` folder; the tool's absolute path is `<plugin>/tools/events.py`. Resolve `<plugin>` from the absolute path of any plugin file you have on hand (this reference, a SKILL.md) by stripping back to that root, and use the absolute form in every command -- bare `python3 tools/events.py` only works when the cwd is the plugin root.

Use `python3`, not `python` -- stock macOS has no `python` shim.

Subcommands:

| Command | Answers |
|---|---|
| `vars [--at-seq N] [--span ID] [--shell MARKER] [--with-env]` | Which user variables are visible at the failure point (default), or at any other vantage. |
| `stack [--at-seq N \| --span ID]` | Call stack leaf-to-root from a span (or the failure event's span by default). |
| `buffer SHELL [--at-seq N]` | Cumulative buffer for a shell up to a seq (default: end of run). `SHELL` accepts either the `shell_marker` (stable across renames) or the display `shell` name. |
| `timeline SHELL` | Per-shell chronological merge of events and buffer events. Same `SHELL` resolution as `buffer`. |
| `dedup` | Effect-setup spans grouped by identity hash (`marker`); `is_reuse: false` is the bootstrap acquire. |
| `pure-trace SPAN_ID` | Value events under a span subtree (interpolation / var-let / var-read / var-assign / pure-match / string-eval). |
| `diff OTHER_EVENTS_JSON` | First event index where two runs disagree on `(kind, shell_marker, source)`. Ignores `seq` and `buffer_events` (PTY chunking noise). |

Output is JSON on stdout for structured subcommands; raw text for `buffer`. Compose with `jq` when narrowing further.

## Reconstructing context around an event

### 0. Enumerate the shells in this log

`buffer` and `timeline` both take a `SHELL` argument; this recipe is how you find one to pass.

```bash
jq '.shells | to_entries | map({marker: .key, name: .value.name, spawn_ts: .value.spawn_ts, terminate_ts: .value.terminate_ts})' events.json
```

There is no "main shell" in Relux -- a test can declare any number of `shell <name>` blocks, plus an implicit `__cleanup` shell whenever it has a `cleanup` block, plus shells inherited via `expose shell` from effects. Pick by `name`:

- `name: "__cleanup"` -- the implicit shell for cleanup blocks. Skip this unless you are specifically diagnosing cleanup.
- `name: "<word>"` (e.g. `"smoke"`, `"db"`) -- a `shell <name>` block declared in the test or in an effect's body.
- `name: "Alias.shell"` -- a shell re-exposed by an effect under an alias, visible only inside the effect's setup span.

If the test has only one user-declared shell, that's almost certainly the one you want.

Pass either the marker (stable; recommended) or the display name to `buffer` / `timeline`:

```bash
python3 <plugin>/tools/events.py buffer crumbly-drone-6747   # marker
python3 <plugin>/tools/events.py buffer smoke                # display name
```

### 1. Buffer state for shell `S` at event seq `N`

```bash
python3 <plugin>/tools/events.py buffer "$S" --at-seq "$N"
```

### 2. Variable scope at the failure point (or another vantage)

For the failure vantage (the most common question -- "what was visible when this broke?"), the runtime already captured the projection. Read it directly:

```bash
jq '.outcome | select(.kind == "fail") | .vars_in_scope' events.json
```

`outcome.vars_in_scope` is `[name, value][]` and is computed by the runtime with the same rule the tool would replay, so the answer is identical -- with zero hops.

**For any other vantage** (an earlier event, a different span, a passing test, a cleanup span), use the CLI to replay scope from a chosen `(seq, span, shell)`:

```bash
python3 <plugin>/tools/events.py vars
```

Defaults to the failure vantage when `outcome.kind` is `fail` or `cancelled`; override `--at-seq`, `--span`, `--shell` to project from a different point. Pass `--with-env` to layer `env.bootstrap` underneath the user vars (the viewer surfaces env in a separate modal, but at runtime env is always visible at the bottom of the chain). The tool reports names and values only; the provenance of each value lives in `env.bootstrap[].source` (see [environment](environment.md)).

The algorithm mirrors the runtime's three-map model: ambient scope (`test` / `effect-setup`) at the bottom, shell-local lets on top of that, fn-call frames as a hard barrier. Cleanup spans inherit the effect's scope via `setup_span`. Transparent BIFs (pure BIFs and `annotate` / `log` / `sleep`) are not barriers. See `tools/events.py` for the implementation.

### 3. Call stack at event seq `N`

```bash
python3 <plugin>/tools/events.py stack --at-seq "$N"
```

Returns `[{id, kind, name?, effect?, alias?, shell?}, ...]` leaf-to-root.

### 4. Per-shell chronological timeline

```bash
python3 <plugin>/tools/events.py timeline "$S"
```

Returns `[{seq, kind, src: "event" | "buffer"}, ...]` sorted by `seq`.

## Failure triage

### 5. Locate the failing event

```bash
jq '.outcome | select(.kind == "fail") | {type, span, event_seq, shell, buffer_tail}' events.json
```

### 6. Cleanup spans for a failed test

```bash
jq '[.spans | to_entries[] | select(.value.kind == "effect-cleanup" or .value.kind == "cleanup-block") | .value]' events.json
```

### 7. Why was this test skipped

`outcome.kind == "skip"` carries the `marker-eval` span and the `bool-check` event under it.

```bash
jq '.outcome | select(.kind == "skip") | {marker_kind, evaluation}' events.json
```

## Effect and dedup investigation

### 8. Was this effect fresh or reused / which identity tuple did `start` resolve to

```bash
python3 <plugin>/tools/events.py dedup
```

Returns one group per identity hash (`marker`) with all acquires under it. The acquire with `is_reuse: false` is the bootstrap setup; the rest are dedup'd zero-duration acquires. The `overlay` field on each shows the evaluated overlay tuple.

## Pure-eval tracing

### 9. What did `let x := trim(y)` compute / trace pure evaluation under a span

```bash
python3 <plugin>/tools/events.py pure-trace "$SPAN_ID"
```

Collects every value event (`interpolation`, `var-let`, `var-read`, `var-assign`, `pure-match`, `string-eval`) under the span subtree. Use the `stack` output to pick the span you care about.

### 10. Capture values from a match

```bash
jq '[.events[] | select(.kind == "match-done" and .captures != null) | {seq, shell, captures}]' events.json
```

## Cross-run analysis

### 11. Artifacts a test produced

```bash
jq '.artifacts' events.json
```

### 12. Diff two runs for a regression

```bash
python3 <plugin>/tools/events.py --events run1/events.json diff run2/events.json
```

Walks the two runs' `events` arrays in **lockstep by array index** and returns the first index where the two sides disagree on `(kind, shell_marker, source)`. `buffer_events` are *not* part of the comparison -- they are the PTY chunking layer, non-deterministic across runs; the runtime is deterministic at the `(kind, shell_marker, source)` level on `events`, so a real divergence at index N means the test code took a different path or a match resolved differently at that line. `seq` is also dropped from the key because it shifts whenever buffer-event counts differ between runs, but it is reported in the output for triage.

Output shape:

```jsonc
{
  "first_divergence_index": 256,                // index into `events`, not into a merged stream
  "left":  { "seq": 3640, "kind": "match-done", "shell_marker": "...",
             "source": { "file": "relux/lib/http.relux", "line": 32 } },
  "right": { "seq": 3490, "kind": "timeout",    "shell_marker": "...",
             "source": { "file": "relux/lib/http.relux", "line": 32 } }
}
```

`{"identical": true, "length": N}` if every index agrees; if one side is longer, the extra-events form fires with `first_divergence_index` set to the overlap end and `left_length`/`right_length` reported.

`diff` is the right tool whether the second run passed or failed -- the comparison key is behavioural, not outcome-typed. Pass-vs-fail and pass-vs-pass drift go through the same command. See also Recipe 5 if all you need is the failing run's own anchor (`outcome.event_seq`, `buffer_tail`).

**Companion read for pass-vs-fail:** the failing run's `outcome` independently names the regression anchor (`event_seq`, `buffer_tail`, `vars_in_scope`). Reading it alongside `diff` is cheap and they should agree on the same `.relux` line:

```bash
jq '.outcome | {kind, type, event_seq, pattern, buffer_tail}' fail/events.json
```

## Pitfalls and best practices

### Verify a field exists before scripting against it

The schema evolves. Probe the actual artifact first with `jq 'keys'` and `jq '.outcome | keys'`, then build queries against what you confirm is there.

Don't:

```bash
jq '.report.failure.failed_at' events.json
```

Do:

```bash
jq 'keys' events.json
jq '.outcome | keys' events.json
```

### For tree traversal, use the CLI rather than nested jq

Walking `parent` chains, correlating buffer state with timing, or merging across spans needs multiple passes. The recipes above route those questions through `tools/events.py`. If a question is not covered by an existing subcommand, parse `events.json` in a small Python/Node script -- a single-pass `jq` expression is almost always the wrong hammer for tree work.

Don't: a 60-line nested jq expression for variable-scope reconstruction.

Do: `python3 <plugin>/tools/events.py vars [...]`, or for a new shape, add a subcommand to `tools/events.py` rather than reinventing the algorithm at every call site.

## See also

- [events-schema](events-schema.md) -- read if you need the field reference
- [events-failures](events-failures.md) -- read if you need the outcome shapes
- [viewer](viewer.md) -- read if you need to hand the user a visual link or ask for a clue only the visual surface reveals
