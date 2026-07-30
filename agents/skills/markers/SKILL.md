---
name: relux:markers
description: Add, change, remove, move, or consolidate `# skip` / `# flaky` / `# run if` markers on tests, effects, fns, and pure fns. Use when the user asks to gate a test on an environment variable, mark something flaky, restrict to one OS or CI provider, change or delete an existing marker, promote a marker upstream (test -> effect or test -> fn), or extract a repeated marker onto a guarded `pure fn`. Fires on phrasings like "skip this when X", "only run on Y", "mark as flaky", "remove this skip", "move this marker to the effect", "these N tests share a marker -- extract it", and on any direct edit to a `# skip` / `# run` / `# flaky` line. For diagnosing why a past run was reported as Skipped from an existing `events.json` / `event.html`, the dedicated run-time-forensics skill is the right entry instead (Wave 2 leaf, not yet drafted).
---

# Manage condition markers

Add, edit, or remove `# skip` / `# run` / `# flaky` markers on a `test`,
`effect`, `fn`, or `pure fn` declaration -- with discipline about layer
choice, form, propagation, and de-duplication. The same five-step core
(layer -> form -> condition -> apply -> verify -> audit) handles every
operation; step 1 classifies the operation and the rest of the workflow
adapts.

## When to use

User phrasings:

- "Skip this test when `${VAR}` is set." / "Mark this as flaky."
- "Only run on Linux." / "Run this only in CI."
- "Change the marker on this test from `# skip` to `# skip unless ...`."
- "Drop the skip from this test."
- "Move this skip up to the effect."
- "These five tests have the same marker -- extract it onto a guarded
  `pure fn`."
- "Audit the markers in this file."

Agent-task signals:

- About to write, edit, or delete a `# skip` / `# run` / `# flaky` line.
- About to copy the same marker line onto N adjacent tests.
- A test is documented as "CI only" / "linux only" / "skipped pending X"
  in a comment but has no marker.
- Reviewing a file and a marker line looks duplicated by an upstream
  effect or function, contradicted by another marker on the same target,
  or written with a form/polarity that obscures intent.

**Out of scope:** if the user is starting from a Skipped outcome in a past
run's `events.json` / `event.html` and wants to know *which* marker
fired, that is the diagnose-skipped skill's territory (Wave 2 leaf; not yet
drafted). Once the responsible marker is identified, this skill takes over
to change it.

**Direct invocation (`/relux:markers`).** Ask the user which operation
(add / modify / remove / move / consolidate) and the target (file path
plus `test` / `effect` / `fn` / `pure fn` name) before starting. Step 1
of the workflow classifies on these; without them the pre-flight checks
have nothing to walk.

## Pre-flight checks

- [ ] **Required:** read `../../references/markers.md`. The workflow below
      delegates to it for forms, expression shapes, evaluation timing,
      propagation rules, and worked-out pitfalls; the skill assumes that
      material is loaded.
- [ ] **Classify the operation:** Add, Modify, Remove, Move, or
      Consolidate. Each maps to a distinct path in the workflow below.
      If the user signalled "change this marker" but the *layer* is what
      should change, classify as Move (not Modify).
- [ ] Identify the target(s) -- which `test` / `effect` / `fn` / `pure fn`
      declaration each operation touches.
- [ ] Identify the trigger -- the environment variable, command, or value
      the condition keys on. Markers only see env vars and pure
      BIFs/pure fns; `let` bindings and shell output are not in scope.
- [ ] Walk the upstream chain. For a `test`, check every `effect` it
      `start`s and every `fn` / `pure fn` it calls (transitively) for an
      existing marker. The skip set propagates through both axes.
- [ ] For Modify, Remove, Move, and Consolidate, read the existing
      marker line(s) verbatim -- each path's first step depends on the
      current form, polarity, and condition.

## Workflow

Pick the operation path below based on the classification done in
pre-flight. **Verify** and **Audit** are shared phases run at the end of
every path.

### The layer rubric (used by Add, Move, Consolidate)

For paths that place a marker, pick the declaration whose marker most
narrowly captures the real condition.

- The condition is "this **test** needs `${VAR}`" -- mark the test.
- The condition is "this **service** (an effect) needs `${VAR}`" -- mark
  the effect. Every test that `start`s it inherits the skip (or flaky).
- The condition is "this **helper** (a `fn` / `pure fn`) needs `${VAR}`" --
  mark the function. Every test that calls it (transitively) inherits the
  skip / flaky.
- The condition is "these **N tests** share a gate" -- write a guarded
  `pure fn` and have each test opt in with `let _group := mark_<group>()`
  (the **Consolidate** path).

If the right answer is "the effect" or "the function" and the user
pointed at a test, surface the choice before writing. A `# skip` on a
test whose effect already skips is duplication; the real fix lives
upstream.

### Path: Add

Use when no marker exists on the target yet.

1. **Pick the layer** using the rubric above.
2. **Pick form and polarity.** Sense (`skip` / `run` / `flaky`) and
   polarity (`if` / `unless`) -- `../../references/markers.md` > *Forms* is the
   source of truth; *Pick the marker that reads like intent* gives the
   rubric. For multiple AND conditions, stack markers on separate lines.
3. **Compose the condition** using `../../references/markers.md` >
   *Expression shapes*. Prefer the bare form for single-variable
   truthiness and equality checks.
4. **Apply.** Write each marker on its own line directly above the
   declaration. Comments and blank lines between markers and the
   declaration are allowed; nothing else.

Then run **Verify**, then **Audit**.

### Path: Modify

Use when a marker exists on the target and form, polarity, or condition
needs to change. If the *layer* is what should change, switch to the
**Move** path instead.

1. **Locate** the existing marker line(s) on the target. Read them
   verbatim.
2. **Diagnose what is changing:**
   - *Form / polarity* (e.g. `# skip` -> `# skip unless ...`,
     `# run if X` -> `# skip unless X`): refer to
     `../../references/markers.md` > *Forms* and *Pick the marker that reads
     like intent*.
   - *Condition* (rewriting the expression): refer to
     `../../references/markers.md` > *Expression shapes*.
   - *Layer*: this is a Move, not a Modify. Switch paths.
3. **Apply.** Edit the existing line(s) in place. Do not stack a new
   marker on top of the old one when the intent was to change the
   existing one.

Then run **Verify**, then **Audit**.

### Path: Remove

Use when a marker should be deleted entirely. The discipline here is
about confirming what removal exposes, not about composing anything.

1. **Locate** the marker line(s). Read them verbatim.
2. **Justify the removal.** Walk the upstream chain and answer one of:
   - *The precondition is now permanently satisfied* (e.g., the tool the
     marker guarded on is now bundled with the project) -- safe to
     remove.
   - *An upstream effect or function already enforces the same gate* via
     marker propagation -- safe to remove (this marker was dead-code
     duplication, see *Skip propagates transitively* in
     `../../references/markers.md`).
   - *No upstream gate exists and the precondition still matters* --
     stop. Either keep the marker, follow the **Move** path to push it
     upstream, or follow the **Consolidate** path to extract it onto a
     shared `pure fn`.
3. **Apply.** Delete the line(s). If multiple stacked markers exist on
   the target, delete only the one(s) that should go.

Then run **Verify**, then **Audit**.

### Path: Move

Use when the marker stays in spirit but should live at a different
layer (test -> effect, test -> fn, effect -> fn, or any direction).
Mechanically: Remove at source, Add at destination.

1. **Locate** the existing marker at the source.
2. **Pick the destination layer** using the rubric.
3. **Compose for the destination.** The condition expression may need to
   change -- a test-local gate may not map directly onto an effect's
   vocabulary. Refer to `../../references/markers.md` > *Expression shapes*.
4. **Apply.** Delete the marker line(s) at the source declaration; write
   the new marker line(s) directly above the destination declaration.
   Verify the source declaration has no other reason to carry a marker
   before deleting.

Then run **Verify**, then **Audit**.

### Path: Consolidate

Use when N tests or effects share an identical (or near-identical) marker
and the duplication should be hoisted onto a shared guarded `pure fn`.
Do not apply this path to function groups; for those, place the marker
on each `fn` directly.

1. **Identify the group.** List every target that participates and
   confirm they all key on the same condition.
2. **Name the group.** Pick a `mark_<group>` / `requires_<tool>` name
   that reads as the reason the group exists (e.g. `mark_smoke`,
   `requires_docker`).
3. **Author the guard.** Write a new `pure fn` whose body returns a
   short label string, with the shared marker placed above it. Follow
   the **Add** path against this new `pure fn` declaration.
4. **Apply at each participant.** Insert
   `let _<name> := mark_<group>()` as the first statement inside each
   target's body, then delete each per-target marker line that the
   guard now subsumes.

Then run **Verify**, then **Audit**.

### Shared: Verify

Exercise both branches of every condition this operation introduced or
changed:

```bash
# Truthy branch:
CI=1 relux run -f path/to/test.relux

# Falsy branch (test should be reported as Skipped):
unset CI
relux run -f path/to/test.relux
```

In each run, check the affected tests' reported outcomes:

- Targeted tests report `Pass` or `Skipped` as expected.
- A target reported `Invalid` (not Skipped) means the body failed to
  lower -- a `# skip` does not shield a broken test (see
  `../../references/markers.md` > *`# skip` does not shield a broken test*).
  Fix the body first, then re-verify the gate.
- For **Move** and **Consolidate**: every test that was previously
  gated by the old placement is still gated by the new one (no test
  silently lost or gained its marker).
- For **Remove**: the test now runs (or skips for a different reason).
  If it still skips, an upstream marker is responsible -- the **Audit**
  phase catches this.
- No unrelated tests changed outcome.

For a faster sanity check without spawning shells, `relux check` reports
resolve-time errors in marker expressions (typos, undefined pure fns,
malformed regex). It does not evaluate the condition.

### Shared: Audit the chain

Re-walk the upstream chain once.

- If an effect or function the test depends on now duplicates a similar
  marker, remove whichever placement is wrong by the layer rubric.
- If a sibling test repeats this exact marker line, the
  group-via-guarded-`pure fn` pattern applies -- which promotes this
  edit retroactively from **Add** into **Consolidate**.
- For **Remove**: confirm no upstream marker is still gating the target.
  If one is, decide whether the removal was correct (upstream is the
  real gate; keep the upstream marker, the removal was right) or
  premature (the target needed its own gate; restore it via **Add**).

## Done when

- For **Add**, **Modify**, **Move**: each affected marker is placed on
  the correct declaration per the layer rubric, and form/polarity read
  like a sentence.
- For **Remove**: the marker line is gone and no test depends on its
  absence creating an unintended run.
- For **Consolidate**: the guarded `pure fn` exists, every participating
  test opts in via `let _group := mark_<group>()`, and no per-test
  marker remains from the old pattern.
- Both truthy and falsy branches of any new/changed condition have been
  exercised; outcomes match the intent.
- The upstream chain contains no duplicate or contradictory markers.

## Cross-skill handoffs

None in this wave. Future revisions will link to `diagnose-skipped` (for
run-time forensics on which marker fired and why) and `diagnose-flaky`
(for deciding when `# flaky` is the right gate vs. fixing the underlying
race).

## References

- `../../references/markers.md` -- forms, expression shapes, evaluation timing,
  propagation, and worked-out pitfalls.
- `../../references/functions.md` -- function-level propagation rules; how a
  marker on `fn` / `pure fn` reaches every caller.
- `../../references/effects-identity.md` -- effect-level propagation rules; how
  a marker on `effect` reaches every test that `start`s it.
- `../../references/events-failures.md` -- `SkipRecord` shape in the structured
  log; use when inspecting `events.json` to confirm which marker fired.

## Pitfalls

`../../references/markers.md` > *Pitfalls and best practices* enumerates the
recurring marker mistakes (bare vs interpolated truthiness, run-if vs
skip-unless phrasing, transitive skip propagation, group-via-guarded-
`pure fn`). The pre-flight read of that file is the discipline; this
section is intentionally empty to avoid drift.
