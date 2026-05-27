---
name: relux:test-edit
description: Modify an existing `test` declaration -- changing its body (sends, matches, captures, fail patterns, sleeps), markers, `start` deps, `let` bindings, docstring, name, cleanup -- or relocate it under `relux/tests/<sut>/<scope>/`, or split a fat test by hard-switching to `relux:test-write` for a new sibling and stripping the now-covered slice from the original. Use when the user asks to retune a match, anchor an unanchored pattern, tighten or loosen an assertion, add or drop a `start <Effect>`, change the test's isolation values, fix a captures-lost-to-clobber bug, add or remove a marker, rename the test, move it to a different SUT/scope, update its docstring, or split one fat test into two siblings. Fires on phrasings like "fix the match in `<test>`", "anchor this regex", "add a fail pattern", "the test name no longer matches the body", "move this test under `<sut>/<scope>/`", "this test does too much -- split it", "swap `start <OldEffect>` for `<NewEffect>`". Diagnosing a failing test is future `test-debug`; flake triage is future `test-flakiness`; authoring a brand-new test is `relux:test-write`; changing an effect's surface is `relux:effect-edit`; helper authoring is `relux:function`. Hard-switch protocol -- if the edit needs an effect that does not yet exist, switch to `relux:effect-write`; if the edit needs a broken effect fixed first, switch to `relux:effect-edit`; if the edit is a split-by-extraction, switch to `relux:test-write` for the new sibling and return here to strip.
---

# Modify an existing test

Edit a `test` declaration in place, relocate it under
`relux/tests/<sut>/<scope>/`, or coordinate a split-by-extraction
(hand off to `relux:test-write` for the new sibling, return to
strip the covered slice from the original).

Pick the workflow shape **first** -- in-place body edit, relocate,
or split-by-extraction. Each shape walks a distinct rhythm; the
classification in *Pre-flight* feeds straight into one of the
three subsections of *Workflow*. The Verify pass at the end --
`relux check && relux run` -- is the same for all three and is
the universal anchor.

## When to use

User phrasings:

- "Fix the match in `<test>`." / "This regex isn't anchored."
- "Add a `!?` fail pattern on this shell."
- "Tighten the assertion -- the regex is too broad."
- "Swap `<? ...` for `<= ...` here (the value has regex metachars)."
- "The captures are lost -- bind them with a `let`."
- "Add `start <Effect>` / Drop `start <Effect>` from this test."
- "Change the isolation -- the port is hardcoded; use
  `available_port()`."
- "Update the docstring -- it no longer matches the body."
- "Rename this test -- the name doesn't read as the assertion."
- "Add `# skip unless CI` to this test." / "Remove the
  `# flaky`."
- "Move this test under `<sut>/<scope>/`."
- "This test does too much -- split it into two siblings."

Agent-task signals:

- The test name and body have drifted apart -- the name claims
  one assertion, the body proves a different one (or several).
  Fix is "rename + tighten body" (in-place) or "split into
  named siblings" (split-by-extraction).
- A match that should be anchored isn't -- the body relies on
  unanchored `<? ...` or substring `<= ...` and is susceptible
  to the echo trap (`../../references/matching.md` > *Echo trap*).
- A `let` consumes `$1` / `$2` two or more matches later -- the
  capture was overwritten in between (`../../references/statements.md`
  > *Bind captures with `let` before the next match overwrites
  them*).
- The test hardcodes a shared mutable resource -- port `8080`,
  `/tmp/foo`, a fixed database name -- and the suite runs
  concurrently. Fix is `available_port()` / `uuid()` /
  effect-driven identity.
- The test file's scope or SUT subdirectory no longer matches
  the test's content -- it lives under `tests/api/smoke/` but
  exercises an `auth/` concern, or under a SUT subdirectory
  that does not match what the test actually drives.
- A test has grown several distinct behaviors under one name --
  classic split-by-extraction signal.

**Out of scope:**

- **Diagnosing a failing test** -- future `test-debug` (not yet
  drafted). The `relux run` -> read the viewer / structured log
  -> identify the failing match -> form a hypothesis loop is its
  own discipline; this skill applies a *known* change, it does
  not figure out what change is needed.
- **Flake triage** -- future `test-flakiness` (not yet drafted).
  Deciding whether a flaky test is a real race, an isolation
  bug, or a candidate for `# flaky` is that skill's territory;
  applying the `# flaky` once decided is this skill (in-place
  marker edit) but the *decision* is not.
- **Authoring a brand-new test** -- `relux:test-write`. When a
  split-by-extraction needs a new sibling, switch to that skill
  for the new file, then return here to strip from the
  original.
- **Changing an effect's surface** -- `relux:effect-edit`. If
  the edit on the test side reveals that the effect's surface
  is wrong, switch out before continuing. Do not patch around a
  broken effect by mangling the test.
- **Helper functions** -- `relux:function`. If an in-place edit
  grows a repeated send/match block to three occurrences (or
  the SUT is a non-POSIX shell needing a project-local anchor),
  hand off to author the helper, then return.
- **Marker decisions** that carry product intent (`# skip
  unless CI` to gate a heavy integration test, `# flaky`
  acceptance) -- `relux:markers` for the placement rubric. This
  skill applies a marker change the user has decided on.
- **Removing a test** -- mechanical (delete the file). No
  rubric; no skill.

**Direct invocation (`/relux:test-edit`).** Ask the user which
test (path + name), which workflow shape the change is (in-place
body edit, relocate, or split-by-extraction), and what the
intended change is. Without those three pieces, the pre-flight
classification has nothing to walk.

## Pre-flight checks

- [ ] **Required:** read `../../references/block-structure.md`. Source
      of truth for the test block skeleton, section ordering,
      docstring slot, naming rules.
- [ ] **Required:** read `../../references/matching.md`. Source of
      truth for `<?` / `<=` semantics, the buffer/cursor model,
      the command-match-anchor rhythm, captures, the echo trap.
      Most in-place body edits touch matching.
- [ ] **Required:** read `../../references/imports.md` -- specifically
      *Group tests by SUT then scope*. Source of truth for the
      `relux/tests/<sut>/<scope>/` layout, the universal `smoke/`
      bucket, the propose-then-pick discipline for non-smoke
      scopes, and the snake_case filename rule. Relocate and
      split-by-extraction both rely on it; in-place edits use it
      when the file's home has drifted.
- [ ] **Conditional:** read `../../references/statements.md` if the
      edit touches sends, captures-to-let, `sleep`, or
      reassignment.
- [ ] **Conditional:** read `../../references/interpolation.md` if the
      edit touches `${...}` payloads -- especially the
      `${...}-only-accepts-a-name` pitfall when the edit needs
      to splice a BIF call result.
- [ ] **Conditional:** read `../../references/fail-patterns.md` if the
      edit adds, removes, or retunes `!?` / `!=`.
- [ ] **Conditional:** read `../../references/multimatch.md` if the
      edit swaps a chain of `<?`/`<=` for `<{ ... }` (or vice
      versa).
- [ ] **Conditional:** read `../../references/timeouts.md` if the edit
      adds, removes, or retunes inline `~Ns?` / `@Ns=` overrides.
      Test-level `~Ns` / `@Ns` on the declaration is a no-go
      (the audit catches it).
- [ ] **Conditional:** read `../../references/markers.md` if the edit
      adds or removes `# skip` / `# run` / `# flaky`. The
      marker-flip flag rule below relies on it.
- [ ] **Conditional:** read `../../references/effects-identity.md` and
      `../../references/effects-expose.md` if the edit changes a
      `start <Effect>` line -- adds one, drops one, swaps for
      another, changes the overlay.
- [ ] **Conditional:** read `../../references/cleanup.md` if the edit
      adds, removes, or modifies a `cleanup { ... }` block.
- [ ] **Conditional:** read `../../references/functions.md` and
      `../../references/bifs.md` if the edit touches a helper / BIF
      call site.
- [ ] Locate the test (file path + line range). Read the full
      `test { ... }` block verbatim, including its file-level
      `import`s and markers. Tests are typically small enough
      that the whole block fits in one read.
- [ ] **Classify the workflow shape** -- the central pre-flight
      decision. The three shapes are documented in *Workflow*
      below; pick exactly one. The classification gates which
      subsection's discipline applies.

### Classify the workflow shape

| Signal | Shape |
|---|---|
| Body / marker / docstring / name / effect-set change; file path stays put. | **In-place body edit** |
| File moves to a different `<sut>/<scope>/`; body content stays the same (optional rename + docstring tweak only). | **Relocate** |
| One fat test asserts multiple behaviors; a slice belongs in a new sibling test. | **Split-by-extraction** |

If signals point at two shapes, **the body change always wins**:
a relocate that "also fixes the match" is two edits. Land the
in-place body edit first under its discipline, then run a
separate relocate. Bundling them muddies the diff and hides
which change broke verification.

## Workflow

### Shape 1: In-place body edit

The common case. The file stays where it is; one or more of the
following body elements changes:

- **Match operator pick** (`<?` <-> `<=`, `<? ...` becoming
  `<{ ... }`, etc.).
- **Pattern body** -- anchoring an unanchored regex, tightening
  a broad pattern, swapping captures.
- **Captures-to-let** -- inserting `let x = $1` immediately
  after a `<?` so the value survives the next match's clobber.
- **Sends** -- changing the command, adding a `=>` continuation,
  rewording an `>` payload.
- **Fail patterns** -- adding `!?` / `!=` on a service-running
  shell, retuning an existing one, removing a stale one.
- **Sleeps** -- replacing `sleep` with a readiness match.
- **Inline timeout overrides** -- adding `~5s?` on a match that
  legitimately needs a longer leash, or removing one that has
  rotted.
- **`start <Effect>` set** -- adding a `start`, dropping one,
  swapping for a different effect, changing the overlay.
- **Test-scope `let` bindings** -- adding test-isolation values
  (`let port = available_port()`), changing an existing one,
  dropping one that is no longer used.
- **Markers** -- adding / removing `# skip`, `# run`, `# flaky`
  on the test declaration or its file-level header.
- **Docstring** -- tightening the detail block when the body
  has drifted, replacing a docstring that restates the name
  with one that adds detail.
- **Test name** -- renaming the `test "..."` string when it no
  longer reads as the assertion the body proves.
- **Cleanup** -- adding, removing, or editing the `cleanup {
  ... }` block (most tests do not need one).

The rule -- the same one that drives `relux:test-write`'s
authoring rhythm -- still holds on the edit side: **one
behavior per test**. If the in-place edit needs to *add*
assertions that prove a different behavior, the right move is
**split-by-extraction**, not a fatter body.

**Match / send / capture / fail-pattern / sleep edits.** The
canonical rules live in `../../references/matching.md` (operators,
buffer/cursor, command-match-anchor rhythm, echo trap,
anchoring discipline), `../../references/statements.md` (send
operators, capture-binding pitfall, prefer-match-over-sleep),
`../../references/fail-patterns.md` (slot scope, inline-only on
service shells), and `../../references/interpolation.md`
(`${...}-only-accepts-a-name`, regex-interpolation-is-raw).
Read whichever ones the edit touches before applying. The
skill's job is to direct the agent at the right reference, not
to restate it.

**Inline timeout edits.** `../../references/timeouts.md` is the
canonical reference. The author's discipline -- *no* `~Ns` /
`@Ns` on the test declaration itself, suite default covers
that -- still holds on edits. If the user asks for a
test-level timeout, push back: the right fix is the suite
default in `Relux.toml`. Inline overrides on individual
matches are fine when the legitimate match-time exceeds the
suite default.

**Marker edits.** `../../references/markers.md` owns the forms,
expression shapes, and evaluation timing. Marker propagation
(`fn` -> every caller, `effect` -> every starter) is
load-bearing on edits too: adding `# skip unless CI` to a
`fn` will skip every test that calls it, not just this one.
**Marker-flip flag rule:** if the requested edit *incidentally*
flips a test's run state -- the user asked to retune a match
but the edit also adds / removes `# skip` / toggles `# flaky`
-- pause and surface the flip to the user before applying. If
the flip is the intended change, just apply. The Verify pass
catches accidental flips (the test starts or stops running),
but raising it before the run is cheaper than letting the
user notice after the fact.

**`start <Effect>` set edits.** Adding or dropping a `start`
shifts which services the test depends on. The effect's
identity and expose surface are documented at the effect
declaration; read it before editing the overlay or the body
references. Three patterns to watch for:

- **Adding a `start`.** The new effect's `expect` set must be
  satisfied by the overlay. Run `relux check` to confirm the
  overlay shape resolves. If the new effect carries markers
  (`# skip unless ...`), they propagate to this test --
  `../../references/markers.md` > *Propagation*. The added
  dependency may make the test conditionally skipped where it
  was unconditional before; flag this to the user if it was
  not intended.
- **Dropping a `start`.** Every `<Alias>.<shell>` /
  `<Alias>.<var>` reference in the body that depended on it
  must also go. `relux check` catches this. If this test was
  the only caller of the effect, the effect is now an orphan
  -- not an error, but flag the dead-code possibility (the
  effect itself may be removable).
- **Swapping `start <OldE>` for `start <NewE>`.** The new
  effect's surface may differ. Walk every
  `<OldAlias>.<x>` reference in the body and translate to
  `<NewAlias>.<y>` (or `relux check` fails). Overlay vars may
  differ between the two effects; adjust.

**Docstring / name drift.** When the body changes, walk the
two strings the test exposes -- the `test "..."` name and the
`"""..."""` docstring -- and confirm they still read as the
assertion the new body makes. If the name no longer reads as
the assertion, rename it; if the docstring restates the name
or has rotted around what the body actually does, rewrite the
detail block. `relux:test-write` > *Docstring restates the
name* (Pitfalls) is the canonical Don't/Do.

**Effect-set edit that needs an effect that does not exist.**
**Stop and hard-switch to `relux:effect-write`** (same
context-budget rationale as `relux:test-write`'s required-effect
handover). Do not draft "what the effect would look like" in
the meantime; the test edit will wait. When the new effect
lands, return here.

**Effect-set edit that needs a broken effect fixed first.**
Hard-switch to `relux:effect-edit`. Same reasoning.

**Helper extraction during a body edit.** If the in-place edit
grows a third occurrence of the same send/match block, hand off
to `relux:function` for the helper (and import it at this file's
header), then return here. Do not inline the same block twice
in the name of the local edit.

### Shape 2: Relocate

The file moves to a different `relux/tests/<sut>/<scope>/`. The
body content does **not** change as part of the relocate; if a
body change is also needed, run an in-place edit first (Shape
1) and the relocate after.

**Pre-condition.** The relocate is warranted when:

- The current `<sut>/` is wrong -- the test exercises a
  different service than the SUT subdirectory suggests.
- The current `<scope>/` is wrong -- the test belongs in a
  different categorical bucket (`smoke/` vs `crud/` vs `auth/`,
  etc.).
- The filename has drifted from the assertion -- the file is
  named `smoke_test.relux` but the assertion is "rejects
  expired tokens", and the new home suggests a sharper name.

`../../references/imports.md` > *Group tests by SUT then scope* owns
the layout rules. Walk that section before deciding the new
path; the propose-then-pick discipline for non-smoke scopes
applies on relocates as much as on initial authoring.

**Workflow.**

1. **Confirm the new path with the user** if the scope is
   anything other than `smoke/`. Surface 3-5 candidate scopes
   that fit the test's content; let the user pick (or accept
   the recommended one). For `smoke/`, no exchange is needed
   -- it is the universal bucket.
2. **Decide whether to rename** the file. The filename is the
   `snake_case` condensation of the test's assertion; if the
   current name reads wrong at the new home, rename it as
   part of the move. The test's `"name"` string is independent
   and stays unchanged by the move *unless* the docstring tweak
   (next step) also implies a name change.
3. **Decide whether to tweak the docstring.** Most relocates do
   not change the docstring -- the test's purpose is the same;
   only its filesystem home is different. The exception is when
   the docstring framed the test in terms of the *old* home
   ("part of the smoke battery" when the test is moving into
   `auth/`); tighten it then.
4. **Move the file.** `mv` works because tests do not export
   anything; no caller needs updating. Do not `cp` and leave
   the original behind -- that turns the relocate into a
   duplication.
5. **Walk Verify.** A relocate that breaks `relux check` (e.g.
   the new path collides with an existing test, or a marker
   the test inherits became unresolvable in the new location)
   surfaces there.

**Common pitfall to avoid.** Bundling body edits into the
relocate. The diff becomes ambiguous -- which change broke
the next failing run? Land the body edit first under Shape 1,
verify, then run the relocate as a clean structural change.

### Shape 3: Split-by-extraction

A fat test asserts several behaviors; the user (or the agent's
audit) decides one slice belongs in a new sibling. The
operation is **Write + Modify**, coordinated across two
skills.

**Recognition shape** (mirror of the one-behavior-per-test
rule in `relux:test-write` > *Before you start: decompose the
test*): the body proves more than one assertion; the name is
vague (`smoke`, `basic`, `it_works`) because no single
sentence covers what the test does; the docstring lists
several behaviors with "and" or bullets.

**Workflow.**

1. **Name the slice to extract.** Identify which assertion the
   new sibling will make. The new test's name is the
   one-liner; if you cannot write it as a clean sentence, the
   slice is not coherent yet -- propose a sharper boundary to
   the user before continuing.
2. **Hard-switch to `relux:test-write`** with a specific
   instruction:
   - The new test's intended name (one-liner assertion).
   - Which slice of the original to model the new sibling on
     (the relevant `shell` block / sequence of sends and
     matches).
   - Which SUT and scope the new sibling lives under (usually
     the same as the original's; if not, the
     `../../references/imports.md` scope discipline applies).
   - Which effects, helpers, and markers the new sibling needs
     (likely the same as the original's, but the new sibling
     is its own block and gets its own `start` / `import`
     lines).
   The hard-switch reasoning is the same as `relux:test-write`'s
   required-effect handover: authoring is heavy enough that
   bundling it into the in-place edit's session burns context
   and risks leaving the strip half-done.
3. **Wait for the new sibling to land and verify**
   (`relux check && relux run` on the new test). Until the
   new sibling is verified, the original is the only proof of
   the extracted assertion and stripping it is unsafe.
4. **Return here.** Re-enter `relux:test-edit` to strip the
   covered slice from the original. The strip is itself an
   **in-place body edit** (Shape 1) -- walk that subsection's
   discipline for it. Remove the assertions the new sibling now
   covers; keep the assertions that remain unique to the
   original.
5. **Update the original's docstring and (likely) name.** The
   original test's stated assertion is narrower now. Rewrite
   the name to read as the surviving assertion; rewrite the
   docstring to describe the narrower scope. Skipping this
   step leaves a test whose name promises more than the body
   proves.
6. **Walk Verify on the stripped original.** A green run on
   the new sibling does not vouch for the stripped original;
   run both.

**Surface preservation.** The union of the new sibling's
assertion and the stripped original's assertion must cover
what the original used to cover. If the strip accidentally
drops an assertion the new sibling does not pick up, you have
*lost coverage* -- silent and visible only by reading the
diff. Audit the diff against the original's body before
declaring the split done.

### Hard-switch table

When the in-place edit (Shape 1) reveals a dependency that
needs to be authored or fixed first, do not patch around it.
Hand off:

| Trigger | Hand off to |
|---|---|
| Edit needs a `start <NewEffect>` that does not exist. | `relux:effect-write` |
| Edit reveals a broken effect surface (caller expects something the effect does not expose; an `expect` set needs to change to support this test). | `relux:effect-edit` |
| Edit grows a repeated send/match block to three occurrences (or the SUT is a non-POSIX shell needing a project-local anchor). | `relux:function` |
| Edit reveals the test should be split rather than modified in place. | Switch to **Shape 3** (split-by-extraction) and hard-switch through `relux:test-write` for the new sibling. |
| Marker placement requires the layer rubric (which `fn` / `effect` / `test` carries the guard). | `relux:markers` |

The hard-switch is a stop, not a yield. Surface the dependency
to the user, name the target skill, and stop until the
upstream change lands.

## Shared: Verify

After every Modify -- in-place, relocate, or the strip half of
a split -- in order:

```bash
relux check
```

`relux check` catches:

- Parse errors (malformed test block, out-of-order sections,
  missing docstring quotes after a docstring tweak, naming-kind
  violations on a renamed effect alias).
- Resolver errors (unknown effect after a `start` change,
  missing import after a helper rename, function-vs-effect
  identifier kind mismatch, capture used without a prior `<?`).
- Regex compile errors in match patterns and fail patterns
  after a regex retune.
- Marker satisfiability errors (a marker that references an
  unknown env var or pure helper after a guard edit).
- Overlay shape errors at each `start <Effect>` site after an
  overlay change.

`relux check` does **not** spawn shells. It cannot tell whether
a match will actually succeed against the live SUT after a
retune, whether a fail-pattern misfires on real output, or
whether captures are correctly clobber-safe at runtime.

```bash
relux run -f path/to/this_test.relux
```

`relux run` is the DoD anchor. It catches:

- **Body regressions** -- a tightened regex that misses,
  unanchored matches still hitting the echo, fail patterns
  misfiring on legitimate output, sleeps too short for the new
  send sequence.
- **Marker flips** -- a test that was unconditionally running
  is now skipped (or vice versa). The viewer's run summary and
  the skip records in `events.json` surface this immediately.
  If the flip was unintended, this is the cheapest place to
  catch it.
- **Effect-set changes** -- a `start` added without the overlay
  the effect expects (catches at check, but the runtime is the
  arbiter of "the overlay value evaluates wrong at runtime").
- **Isolation fixes** -- a test that previously hardcoded a
  port to `8080` may have been running serially against another
  test that also used `8080`; running the suite (not just the
  edited test) under `-j N` is the only way to confirm the
  isolation fix actually buys parallel safety.
- **Cleanup edits** -- cleanup runs at end-of-test under
  uncancellable tokens; only a real run exercises it.

A green `relux run` on the edited test is the gate for an
in-place body edit. For a relocate, `relux check` is usually
sufficient (the body is unchanged) but run anyway -- it is
cheap and rules out the inherit-from-new-marker scenarios.
For a split, run **both** tests (new sibling and stripped
original) -- the new sibling's pass does not vouch for the
stripped original.

When the run fails, the diagnose loop belongs to future
`test-debug`. Surface the failure to the user; this skill ends
here.

## Shared: Audit

After the run passes, re-walk the edit once.

- **One behavior per test.** Read the test name out loud as
  the assertion. Does the body still prove that and only
  that? If not, the edit was a covert fatten -- consider a
  split-by-extraction follow-up.
- **Name and docstring match the body.** The
  `test "<one-line assertion>"` string reads as the assertion;
  the `"""..."""` docstring adds detail (not a restatement).
  If the body changed enough that either is now wrong, the
  edit is incomplete.
- **Anchored patterns.** Every `<? <regex>` is line-anchored
  (`^...$`) unless mid-line content is deliberately targeted.
  Recent retunes especially -- a tightening that loses the
  `^` / `$` re-opens the echo trap.
- **Captures bound before clobber.** Every `${1}` / `${2}` /
  ... / `$1` / `$2` / ... reference is either in the line
  immediately after the `<?` that produced it, or has been
  bound to a `let` first. Edits that insert a new `<?` between
  a capture and its consumer are the classic regression here.
- **Fail patterns inline if set.** `!?` / `!=` lives in the
  shell body that owns the slot, not inside a called `fn`.
- **Test isolation.** Every shared resource the test touches
  -- port, path, database name, run-id -- comes from `uuid()`,
  `available_port()`, or an effect's `expect`-driven identity.
  Edits that swap one hardcoded value for another (`/tmp/foo`
  for `/tmp/bar`) miss the point; the fix is a derived unique
  value.
- **No test-level timeout.** The test declaration does not
  carry a `~Ns` / `@Ns`. If a timeout edit added one, push
  back: the right fix is the suite default in `Relux.toml`.
- **Marker run-state matches intent.** The test's effective
  marker state (after propagation from any effect or `fn` it
  uses) matches what the user expected. If the edit
  incidentally flipped run state, confirm with the user.
- **File placement matches the scoping rule.** The file lives
  under `relux/tests/<sut>/<scope>/` with the SUT matching the
  service the test exercises and the scope matching the rule
  in `../../references/imports.md` > *Group tests by SUT then
  scope*. An in-place edit that pushed the test outside its
  natural scope is a hint that a relocate (or a split) is the
  next step.
- **For relocate edits:** the new path resolves (no collision
  with an existing test), the filename reads as the assertion
  in `snake_case`, and the body is unchanged from before the
  move.
- **For split edits:** the new sibling and the stripped
  original together cover what the original used to cover.
  Diff-review the original's pre-edit body against the union
  of (new sibling) + (stripped original); any assertion that
  fell through the cracks is lost coverage.

## Done when

- The intended workflow shape ran end-to-end; unintended
  changes did not slip in. (Diff review: scan for accidental
  edits to elements outside the user's intent.)
- For an in-place body edit: every body element the edit
  intended to touch is updated, and the audit pass surfaces
  no rotted name / docstring / captures / anchors / isolation.
- For a relocate: the file lives at the new
  `relux/tests/<sut>/<scope>/<name>.relux` path, the filename
  is `snake_case` and reads as the assertion, the body is
  byte-identical to its pre-move state.
- For a split: the new sibling exists, was authored via
  `relux:test-write`, and verified green; the original was
  stripped of the covered slice, its name and docstring
  updated to reflect the narrower assertion; the diff of (new
  sibling body) + (stripped original body) covers every
  assertion in the original's pre-split body.
- `relux check` passes.
- `relux run` passes on every test the edit affected (the
  edited test for in-place; the moved test for relocate; both
  tests for split).
- For a marker edit: the test's effective run state matches
  the user's intent. If the edit incidentally flipped run
  state, the flip was raised with the user before applying
  (or, if not raised before, was confirmed acceptable after
  the Verify pass surfaced it).

## Cross-skill handoffs

- `relux:test-write` -- the hard-switch target for the new-
  sibling half of any split-by-extraction. Always called
  *before* the strip-from-original step here; the strip
  depends on the new sibling existing and being verified.
- `relux:effect-write` -- the hard-switch target when an
  in-place edit needs a `start <NewEffect>` that does not
  exist yet. Same context-budget reasoning as
  `relux:test-write`'s required-effect handover: stop and
  switch; do not draft the test edit against a hypothetical
  effect.
- `relux:effect-edit` -- the hard-switch target when an
  in-place edit reveals the effect's surface is wrong (a var
  needs to be exposed, an `expect` set needs adjustment to
  support the test). Patching around a broken effect by
  mangling the test is the wrong direction.
- `relux:function` -- when the in-place edit grows a repeated
  send/match block to three occurrences (or the SUT is a
  non-POSIX shell that needs a project-local anchor function).
  Hand off, return with the helper in scope.
- `relux:markers` -- when the marker decision needs the layer
  rubric (which `fn` / `effect` / `test` carries the guard).
  This skill applies a marker change the user has decided on;
  the placement rubric is its own discipline.
- `relux:configure` -- when an edit reveals the suite-level
  default match / test / suite timeout is wrong for the suite
  as a whole. Per-test timeout overrides are an audit failure
  (`../../references/timeouts.md`); the right fix is the suite
  default.
- Future `test-debug` (not yet drafted) -- when `relux run`
  fails after the edit and the diagnose loop is needed. This
  skill stops at the first verify; the diagnose loop is its
  own discipline.
- Future `test-flakiness` (not yet drafted) -- when the
  decision to add / remove `# flaky` is not yet made. This
  skill applies the marker once decided; the triage is its
  own discipline.

## References

- `../../references/block-structure.md` -- test block skeleton,
  section ordering, docstring slot, naming rules.
- `../../references/matching.md` -- `<?` / `<=` semantics, buffer /
  cursor, command-match-anchor rhythm, captures, echo trap,
  anchoring discipline.
- `../../references/statements.md` -- send (`>` / `=>`), `let` /
  reassignment, `sleep`, capture-binding pitfall.
- `../../references/multimatch.md` -- `<{ ... }`, captures-don't-
  bind, when to reach for multimatch.
- `../../references/fail-patterns.md` -- `!?` / `!=` slot semantics,
  shell-scoped, inline-only on service shells.
- `../../references/interpolation.md` -- `${var}`, `${1}`,
  `${Alias.var}`, `${...}` only accepts a name, regex
  interpolation is raw.
- `../../references/timeouts.md` -- `~` vs `@`, inline overrides,
  the no-test-level-timeouts rule, suite defaults.
- `../../references/markers.md` -- marker forms, expression shapes,
  propagation, the layer rubric.
- `../../references/effects-identity.md` -- effect lifecycle, the
  `expect` contract, the overlay shape at `start` sites.
- `../../references/effects-expose.md` -- `Alias.shell` /
  `Alias.var` access at the call site.
- `../../references/cleanup.md` -- test cleanup placement, fresh
  shell, idempotency, artifact preservation.
- `../../references/imports.md` -- import syntax, resolution from
  project root, the SUT/scope test-tree layout used by
  relocate and split.
- `../../references/functions.md` -- `fn` vs `pure fn`, frame scope
  vs shell scope, the call-from-test-body rules.
- `../../references/bifs.md` -- `match_ok` / `match_not_ok`,
  `available_port`, `uuid`, `sleep`, control characters.
- `../../references/project-layout.md` -- `relux/tests/` location,
  built-in env vars (`__RELUX_RUN_ARTIFACTS`, `__RELUX_RUN_ID`).

## Pitfalls

The recurring DSL-level mistakes -- the echo trap, unanchored
regex, lost captures, sleep-instead-of-match, regex
interpolation as raw text, `${...}` holding a function call,
test-level timeouts on fixtures, service kills in cleanup,
depending on shell-scope state in cleanup -- live in
`../../references/matching.md`, `../../references/statements.md`,
`../../references/interpolation.md`, `../../references/timeouts.md`, and
`../../references/cleanup.md` with canonical Don't/Do examples. The
pre-flight reads load them; this section captures only the
edit-specific disciplines that have no reference home.

### Letting the name and docstring rot after a body edit

The two strings the test exposes -- the
`test "..."` name and the `"""..."""` docstring -- are the
test's interface to every future reader (in the source, in
`relux run` output, in the viewer's run summary, in `grep`
hits). A body edit that changes which assertion the test
makes invalidates that interface. Skip the rename / docstring
tweak and the test reads as one thing in the catalogue and
proves another in the run -- the canonical "this test passes
so the feature must work" trap is exactly that.

Don't:

```relux
// Before:
test "starts and serves health on the configured port" {
    """
    Spawns the server with PORT=available_port(), waits for the
    "ready on :<port>" line, hits /healthz from a second shell,
    asserts a 200 response.
    """
    let port = available_port()
    shell s {
        > my-server --port ${port}
        <? ^ready on :${port}$
        match_ok()
    }
    shell probe {
        > curl http://localhost:${port}/readyz
        <? ^ok$
        match_ok()
    }
}

// After (body retuned to hit /readyz; name and docstring left
// stale -- the test no longer asserts what either string
// claims):
test "starts and serves health on the configured port" {
    """
    Spawns the server with PORT=available_port(), waits for the
    "ready on :<port>" line, hits /healthz from a second shell,
    asserts a 200 response.
    """
    let port = available_port()
    ...
}
```

Do: rename to `"starts and serves readyz on the configured
port"`, rewrite the docstring's `hits /healthz` to
`hits /readyz`, run Verify.

### Bundling body edits into a relocate

A relocate is a structural move -- file path changes, body
stays. Folding a "while I'm here, also fix the match" change
into the same edit muddies the diff: when the next run fails,
the agent (or reviewer) cannot tell whether the body change
or the move caused it. Land the body edit first under Shape
1, verify green, then run the relocate as a clean structural
change.

Don't:

```text
# single edit
- mv tests/api/smoke/old_name.relux tests/api/auth/rejects_expired_token.relux
- (also retune the match in the body)
```

Do:

```text
# edit 1 (Shape 1): retune the match in place, verify green
# edit 2 (Shape 2): mv tests/api/smoke/old_name.relux
#                   tests/api/auth/rejects_expired_token.relux, verify green
```

### Stripping the original before the new sibling is verified

In a split-by-extraction, the original test is the only proof
of the extracted assertion until the new sibling lands *and*
runs green. Stripping the original before the new sibling has
been verified leaves a window where neither test covers the
extracted behavior. If the new sibling has a bug the strip
exposed (a typo in the rebuilt overlay, a different
`available_port()` than the original used to share resources
with), the regression is silent until someone re-reads the
diff.

Don't:

```text
1. Decide to split.
2. Strip the original.
3. Hand off to test-write for the new sibling.
4. Verify both.
```

Do:

```text
1. Decide to split.
2. Hand off to test-write for the new sibling.
3. Verify the new sibling (relux check && relux run).
4. Return to test-edit; strip the original.
5. Verify the stripped original.
```

The discipline survives even if the user pushes for speed --
the strip is cheap; doing it last is also cheap; doing it
first risks a coverage gap that surfaces only by code review.

### Patching around a missing or broken effect by mangling the test

When an in-place edit reveals that the effect this test
`start`s does not expose the right shell / var, or its
`expect` set is wrong for what the test needs to forward, the
correct move is `relux:effect-edit` (or `relux:effect-write`
if the effect does not exist). Reaching for `> docker exec
...` raw shell commands in the test body to work around a
missing exposure leaves the rest of the suite blind to the
effect's true surface and accumulates per-test hacks that
should have been one effect change.

Don't:

```relux
// effect Pg exposes shell `psql` but not `psql_port`; the
// test wants to assert on the port it bound. Body grows raw
// docker calls.
test "stores rows on the bound port" {
    start Pg
    shell s {
        > docker inspect -f '{{...}}' pg-${__RELUX_RUN_ID}
        <? ^([0-9]+)$
        let port = $1
        ...
    }
}
```

Do: hard-switch to `relux:effect-edit` to expose
`pg.psql_port`; return and reference `${Pg.psql_port}`
directly. The other 10 tests that need the same port
benefit from one effect edit instead of 10 raw-shell
patches.

### Skipping `relux run` on a "cosmetic" edit

A rename, a docstring tweak, a marker-condition tightening,
an inline timeout adjustment -- none of these *seem* like
they could fail. But:

- A rename that collides with another test (same `"name"`
  string under the same scope) surfaces at run time, not
  check.
- A docstring tweak that introduces a stray `"` inside the
  block is a parse error -- check catches it, but only if you
  run check.
- A marker-condition edit that intended to scope an existing
  `# skip` more narrowly may instead flip the test from
  skipped to running (or vice versa); the run is where the
  flip becomes visible.
- An inline timeout adjustment that lowered the leash below
  what the real-world match needs surfaces only when a real
  run takes that long.

Run `relux check && relux run` after every edit, regardless
of how cosmetic the change looks. The check-only false
confidence trap is the same one called out in
`relux:effect-edit` > *Skipping `relux run`*: necessary but
not sufficient.
