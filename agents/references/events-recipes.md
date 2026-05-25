# events.json Recipes

Operational queries against `events.json`. Verify field names against [events-schema](events-schema.md) before scripting -- the schema evolves.

## Cross-cutting tips

- Events are append-only; `seq` is the canonical order.
- `parent_span_id` (`span.parent`) is the only structural link between spans.
- Buffer events are top-level; filter by `shell_marker` (stable) or `shell` (display name).
- `outcome` is a single tagged enum at top level -- there is no `report` field.
- Pre-formatted durations live on TimeoutValue (`duration`, `total_duration`) -- no arithmetic needed.

## Reconstructing context around an event

### 1. Buffer state for shell S at event seq N

Concatenate `grew` payloads from `buffer_events` for shell `S` up to seq `N`.

```bash
jq --arg s "$S" --argjson n "$N" '
  [.buffer_events[]
   | select(.shell == $s and .seq <= $n and .kind == "grew")
   | .data] | join("")' events.json
```

Output: the cumulative bytes in shell S's buffer at that point.

### 2. Variable scope at event seq N

Mirror the runtime's three-map model: one map per ambient scope (`test` / `effect-setup`), one per shell, one per fn-call frame. `fn-call` is a hard barrier (only that frame's vars are visible); `effect-cleanup` hops to its `setup_span` to inherit the effect's lets and overlay.

```python
# Pure BIFs and these three impure BIFs don't create a scope barrier --
# the agent sees through them to the caller's scope.
TRANSPARENT_IMPURE_BIFS = {"annotate", "log", "sleep"}


def is_transparent_bif(span):
    if span["kind"] != "fn-call" or span.get("callee_kind") != "bif":
        return False
    return span.get("is_pure") or span.get("name") in TRANSPARENT_IMPURE_BIFS


def vars_visible(data, event_seq, viewer_span_id, viewer_shell_marker):
    """
    Reconstruct variables visible at the given (event_seq, span, shell_marker)
    vantage. Returns dict name -> value.

    `viewer_shell_marker` is the stable shell identity (`shell_marker` on
    events), not the display `shell` -- the display name can be re-bound
    when an effect's shell is exported into a caller's scope.
    """
    spans = data["spans"]  # dict keyed by span id as string

    def span_by_id(sid):
        return spans.get(str(sid))

    def scope_context(span_id):
        """Walk ancestors. Returns (ambient_scope_id, innermost_fn_id_or_None)."""
        innermost_fn = None
        cur = span_by_id(span_id)
        while cur:
            kind = cur["kind"]
            if kind in ("test", "effect-setup"):
                return cur["id"], innermost_fn
            if kind == "effect-cleanup":
                # Cleanup runs with the effect's scope, even though parented under the test.
                return cur["setup_span"], innermost_fn
            if kind == "fn-call" and not is_transparent_bif(cur) and innermost_fn is None:
                innermost_fn = cur["id"]
            parent = cur.get("parent")
            if parent is None:
                break
            cur = span_by_id(parent)
        return None, innermost_fn

    scope_vars, shell_vars, frame_vars = {}, {}, {}

    # Pre-seed each opaque fn-call frame with its declared args.
    for span in spans.values():
        if span and span["kind"] == "fn-call" and not is_transparent_bif(span):
            frame_vars[span["id"]] = dict(span.get("args", []))

    for ev in data["events"]:
        if ev["seq"] > event_seq:
            break
        ambient, inner_fn = scope_context(ev["span"])
        ev_shell = ev.get("shell_marker")  # stable across rename

        if ev["kind"] == "var-let":
            name, value = ev["name"], ev["value"]
            if inner_fn is not None:
                frame_vars.setdefault(inner_fn, {})[name] = value
            elif ev_shell is not None:
                shell_vars.setdefault(ev_shell, {})[name] = value
            elif ambient is not None:
                scope_vars.setdefault(ambient, {})[name] = value

        elif ev["kind"] == "var-assign":
            # Mutate the existing binding wherever it lives (frame -> shell -> ambient).
            name, value = ev["name"], ev["value"]
            for m in (
                frame_vars.get(inner_fn) if inner_fn is not None else None,
                shell_vars.get(ev_shell) if ev_shell else None,
                scope_vars.get(ambient) if ambient is not None else None,
            ):
                if m and name in m:
                    m[name] = value
                    break

        elif ev["kind"] == "effect-expose-var":
            # Injected into the *parent's* ambient scope as `Alias.var`.
            emitter = span_by_id(ev["span"])
            if not emitter or emitter["kind"] != "effect-setup":
                continue
            alias = emitter.get("alias")
            parent = emitter.get("parent")
            if alias is None or parent is None:
                continue
            parent_ambient, _ = scope_context(parent)
            if parent_ambient is None:
                continue
            scope_vars.setdefault(parent_ambient, {})[f"{alias}.{ev['name']}"] = ev["value"]

    # Project through the viewer's vantage.
    view_ambient, view_fn = scope_context(viewer_span_id)
    if view_fn is not None:
        # Hard barrier: only the frame's vars are visible.
        return dict(frame_vars.get(view_fn, {}))

    # Shell context: ambient scope + shell-local lets (shell shadows ambient).
    out = dict(scope_vars.get(view_ambient, {})) if view_ambient is not None else {}
    if viewer_shell_marker is not None:
        out.update(shell_vars.get(viewer_shell_marker, {}))
    return out
```

For "what does `${name}` actually resolve to" rather than "what was let-bound", layer `data["env"]["bootstrap"]` underneath the returned dict -- the viewer surfaces env in a separate modal, but at runtime env is always visible at the bottom of the chain.

**Shortcut:** if the vantage you want is exactly the failure event, `outcome.vars_in_scope` already carries the projection the runtime captured with this same rule -- no replay needed.

```bash
jq '.outcome | select(.kind == "fail") | .vars_in_scope' events.json
```

Use the replay above only for vantages other than the failure point (an earlier event, a different span, a passing test).

### 3. Call stack at event seq N

Follow `spans[E.span].parent` to root; emit `(kind, name)` per span (leaf to root).

```bash
jq --argjson n $N '
  . as $log
  | ($log.events[] | select(.seq == $n) | .span) as $sid
  | def walk(id):
      if id == null then []
      else ($log.spans[id|tostring]) as $sp
           | [$sp.kind, ($sp.name // null)] + walk($sp.parent)
      end;
    walk($sid)' events.json
```

### 4. Per-shell chronological timeline

Merge `events` and `buffer_events` for shell S, sort by `seq`.

```bash
jq --arg s "$S" '
  [.events[]        | select(.shell == $s) | {seq, kind, src: "event"}]
+ [.buffer_events[] | select(.shell == $s) | {seq, kind, src: "buffer"}]
| sort_by(.seq)' events.json
```

## Failure triage

### 5. Locate the failing event

```bash
jq '.outcome | select(.kind == "fail") | {type, span, event_seq, shell, buffer_tail}' events.json
```

### 6. Cleanup spans for a failed test

Find spans where `kind == "effect-cleanup"` or `kind == "cleanup-block"`; their parent chain leads back to the test span.

```bash
jq '[.spans | to_entries[] | select(.value.kind == "effect-cleanup" or .value.kind == "cleanup-block") | .value]' events.json
```

### 7. Why was this test skipped

`outcome.kind == "skip"` carries the `marker-eval` span and the `bool-check` event under it.

```bash
jq '.outcome | select(.kind == "skip") | {marker_kind, evaluation}' events.json
```

## Effect and dedup investigation

### 8. Was this effect fresh or reused

Effect setup spans have `is_reuse`. The bootstrap setup has `false`; dedup'd acquires have `true`.

```bash
jq '[.spans | to_entries[]
     | select(.value.kind == "effect-setup")
     | {effect: .value.effect, alias: .value.alias, marker: .value.marker, is_reuse: .value.is_reuse}]' events.json
```

### 9. Which identity tuple did this start resolve to

Each `effect-setup` span carries `effect`, `overlay`, and `marker` (the identity hash). Same `marker` across spans means same instance.

```bash
jq '[.spans | to_entries[]
     | select(.value.kind == "effect-setup")
     | {effect: .value.effect, overlay: .value.overlay, marker: .value.marker}]' events.json
```

## Pure-eval tracing

### 10. What did `let x = trim(y)` compute

Under the `var-let` event's span, look for `interpolation` and `fn-call` spans (the `pure-match` / `var-read` events live inside).

```bash
jq '[.events[] | select(.kind == "interpolation" or .kind == "var-let" or .kind == "var-read")]' events.json
```

For a deep trace, walk by `span` (Python script -- jq is awkward for tree walks).

### 11. Capture values from a match

`match-done` events carry `captures: { "0": ..., "1": ..., ... }` (HashMap, optional).

```bash
jq '[.events[] | select(.kind == "match-done" and .captures != null) | {seq, shell, captures}]' events.json
```

## Cross-run analysis

### 12. Artifacts a test produced

```bash
jq '.artifacts' events.json
```

### 13. Diff two runs for a regression

Align by `(info.path, event_seq, event_kind)`; first divergent seq pair is the regression point.

```bash
# Hand the two artifacts to a script:
diff <(jq -c '[.events[] | {seq, kind, shell}]' run1/events.json) \
     <(jq -c '[.events[] | {seq, kind, shell}]' run2/events.json) | head
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

### For tree traversal, write a script -- jq is the wrong hammer

Walking `parent` chains, correlating buffer state with timing, or merging across spans needs multiple passes. A 20-line Python/Node script is clearer than nested jq.

Don't: a 60-line nested jq expression for variable-scope reconstruction.

Do: parse `events.json` in Python/Node; index spans and events by id/seq; compute the answer in plain code.

## See also

- [events-schema](events-schema.md) -- read if you need the field reference
- [events-failures](events-failures.md) -- read if you need the outcome shapes
- [viewer](viewer.md) -- read if you need to hand the user a visual link or ask for a clue only the visual surface reveals
