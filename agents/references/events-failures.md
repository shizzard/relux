# Failure, Cancellation, and Skip Records

The shape of `outcome` for non-passing tests.

## Outcome dispatch

```jsonc
{ "kind": "pass" }
{ "kind": "fail",      ...FailureRecord }
{ "kind": "cancelled", ...CancellationRecord }
{ "kind": "skip",      ...SkipRecord }
```

A test exits non-zero in CI for any of `fail`, `cancelled` -- not for `skip`.

## FailureRecord

Tagged by `type`. Common fields (when present): `span`, `event_seq`, `shell`, `call_stack`, `buffer_tail`, `vars_in_scope`.

| `type` | Fields specific to this variant |
|---|---|
| `match-timeout` | `pattern`, `effective` (TimeoutValue). |
| `fail-pattern-matched` | `pattern`, `matched_line`. |
| `shell-exited` | `exit_code` (or `null`). |
| `multi-match` | `patterns` (all), `matched` (indices that matched before timeout), `effective`. |
| `runtime` | `message` only; `span`, `event_seq`, `shell` are nullable. Used for pre-VM resolver errors. |

- `buffer_tail` is the tail of the shell's buffer at failure time -- usually the most informative single field.
- `call_stack` is leaf-to-root `StackFrame[]`. Each frame:
  - `span` -- `SpanId` (same key used by events' `span` field and the top-level `spans` map, where it appears as `id`).
  - `kind` -- the span kind string (`"fn-call"`, `"shell-block"`, `"test"`, `"effect-setup"`, etc.).
  - `name` -- kind-dependent. `test` -> test name; `effect-setup` / `effect-cleanup` -> effect name; `fn-call` -> function name; `shell-block` / `multi-match` -> shell display name; `null` otherwise.
  - `args` -- `[name, value][]`; non-empty only for `fn-call` (call arguments) and `effect-setup` (the evaluated overlay).
  - `alias` -- optional, only effect-setup frames carry one (`start FX as Alias`).
  - `location` -- optional source location.
- `vars_in_scope` is `[name, value][]` for every user variable visible from the failure point. The runtime applies the same projection rule as [events-recipes#2](events-recipes.md): when the failure is inside a `fn-call`, only that frame's vars appear; otherwise the shell-local lets plus the ambient (test or effect-setup) scope. Env vars are excluded -- they live in top-level `env.bootstrap`.
- `effective` (TimeoutValue) is tagged `tolerance` | `assertion`; carries `duration`, optional `total_duration` and `multiplier` for tolerance.

## CancellationRecord

```jsonc
{
  "kind": "cancelled",
  "reason": { "type": "test-timeout" | "suite-timeout" | "fail-fast" | "sigint", ... },
  "span": SpanId | null,
  "event_seq": EventSeq | null,
  "shell": string | null,
  "call_stack": StackFrame[]
}
```

Reasons carry context:

- `test-timeout`: `duration_ms`.
- `suite-timeout`: `duration_ms`.
- `fail-fast`: `trigger_test` (path of the test that triggered the cut).
- `sigint`: no payload.

## SkipRecord

```jsonc
{
  "kind": "skip",
  "span": SpanId,                       // marker-eval span that decided to skip
  "event_seq": EventSeq,                // bool-check event under it
  "marker_kind": "skip" | "run" | "flaky",
  "evaluation": MarkerEvalDetail,
  "location": SourceLocation | null     // marker source { file, line, start, end }
}
```

`MarkerEvalDetail` is tagged by `shape`:

- `unconditional`
- `bare` `{ value, met }`
- `pure-match` `{ value, pattern, is_regex, met }` -- covers both `=` (`is_regex: false`, containment) and `?` (`is_regex: true`, regex); there is no separate `eq`/`regex` shape.

**Truthiness** (used by `bare` `met`, and by `pure-match` which applies `met` after the comparison): only the empty string is falsy; any non-empty string is truthy. See [markers](markers.md) > *Expression shapes* for the full rule including how `=` and `?` produce their result strings.

### Reading the marker-eval span

The `outcome.span` points at the `marker-eval` span that drove the skip. Open that span for the marker's `decision` and source `location`:

- `decision: "pass"` -- the marker's action did **not** apply.
- `decision: "mark"` -- the marker's action **did** apply.

What "the action" means depends on `marker_kind`:

| `marker_kind` | `decision: "pass"` | `decision: "mark"` |
|---|---|---|
| `skip` | skip did not apply -> test ran (no skip recorded) | skip applied -> test skipped |
| `run` | run did not apply -> test skipped | run applied -> test ran |
| `flaky` | flaky did not apply -> ordinary test | flaky applied -> test treated as flaky |

The asymmetry is why a SkipRecord can carry `marker_kind: "run"` with `decision: "pass"`: a `# run if X` whose `X` is falsy means *run* did not get to apply, so the test was skipped. The `met: false` on `evaluation` tells you the condition went falsy; the `marker_kind` x `decision` table above tells you the action that the falsy condition caused.

Don't confuse `marker-eval.decision: "pass"` with `outcome.kind: "pass"`. The first is a verdict on a marker (action did not apply); the second is a verdict on a test (the test passed). They are unrelated and can disagree -- a skipped test routinely has both `outcome.kind: "skip"` *and* `marker-eval.decision: "pass"` on its marker-eval span.

### Reading the marker source

The SkipRecord names the marker only by `marker_kind`; the actual condition (`# run if SMOKE`, etc.) lives on the record's `location` (`file`, `line`, `start`, `end`). Slice `sources[file][start:end]` to get the verbatim marker line:

```bash
jq -r '.outcome.location as $loc | .sources[$loc.file][$loc.start:$loc.end]' events.json
```

(`location` is `Option<SourceLocation>` -- expect non-null for any marker with a real source span; synthetic markers carry `null`.)

The marker that fired the skip lives under the synthetic `markers` root span.

## Pitfalls and best practices

### Read `buffer_tail` before guessing

`buffer_tail` holds the bytes the matcher saw at failure time. It distinguishes "wrong pattern" from "service not ready" in one read.

Don't: assume the pattern has a typo and start editing it.

Do: read `outcome.buffer_tail` first; if it shows readiness output that hasn't arrived yet, add a wait before the failing match.

### Distinguish failure from cancellation when triaging

`cancelled` means an external stop (deadline, fail-fast, signal). `fail` means the test itself misbehaved. Different fix paths -- don't edit the test body on a `cancelled` outcome until you've checked the reason.

Don't:

```bash
# fix the failing match in the test
```

Do:

```bash
jq '.outcome | {kind, reason}' events.json
# if kind == "cancelled" and reason.type == "test-timeout":
#   increase --test-timeout or use --timeout-multiplier in CI
```

### `runtime` failures are not VM failures

A `runtime` FailureRecord typically means the resolver or pre-VM init refused the test. The fix is in the test's structure, not in any specific shell interaction. `span`, `event_seq`, `shell` may all be null; `message` is the only reliable field.

Don't: scan the buffer / call stack on a `runtime` failure -- they're often empty.

Do: read `outcome.message`; cross-reference with `relux check` output for the same file.

## See also

- [events-schema](events-schema.md) -- read if you need the top-level shape and event/span kinds
- [events-recipes](events-recipes.md) -- read if you need failure-triage recipes
- [markers](markers.md) -- read if you need to know why a skip fired
