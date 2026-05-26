---
name: relux:effect-edit
description: Modify an existing `effect` declaration -- changing its `expect` set, `expose` surface, `start <Dep>` graph, `let` / shell body, or cleanup -- with discipline about identity dedup deltas, caller-surface breakage, wrapper transparency, and verification rhythm. Use when the user asks to change an effect's body, add or remove an `expect` var, rename or drop an `expose`d shell or var, wire in a new dep, switch which shell is the service-running one, retune fail patterns, or split a bundled effect into Config+Service / layer chain. Fires on phrasings like "add a port to `Postgres`", "expose the service shell on `Db`", "drop `migrations_path` from `Database`", "split this effect's config rendering out", "the cleanup is wrong, it deletes the artifact". Authoring new effects is `relux:effect-write`; moving an effect between files or removing it are mechanical operations (grep + relocate / grep + delete callers) and have no skill of their own.
---

# Modify an existing effect

Edit an `effect` declaration in place. The discipline covers what
breaks at each of the five mutation dimensions -- `expect`,
`expose`, `start <Dep>`, `let` / shell body, cleanup -- and when
those breaks become caller-visible (surface) or instance-visible
(identity).

Walk the checklist on **every** Modify, even when you only intend
to touch one dimension. Expect and expose often co-mutate (adding
a port: it goes in `expect`, and the shell that binds it gets
exposed). A body-only edit can still leak through `expose var x`
when `x` is a `let` value the edit changed.

## When to use

User phrasings:

- "Add a port to `<Effect>`." / "Drop `<var>` from `<Effect>`'s expects."
- "Expose the service shell on `<Effect>`." / "Stop exposing
  `<Effect>.<x>` -- nothing reads it."
- "Wire `<Effect>` to start `<Dep>` first." / "Change `<Effect>`'s
  dep on `<OldDep>` to `<NewDep>`."
- "Retune the fail patterns on `<Effect>`'s service shell."
- "The cleanup is wrong -- it deletes the rendered config."
- "Split `<Effect>` -- the config rendering belongs in its own leaf."

Agent-task signals:

- A test references a var or shell on an effect that does not exist
  in the effect's `expose` set. The edit is "add the expose" (not
  "rewrite the test"), unless the caller's expectation is wrong.
- `relux check` reports an identity drift after an `expect` change
  -- two `start <E>` sites that used to share an instance now fan
  out, or vice versa.
- An effect's shell body bundles config-file rendering with the
  service launch. The Extract recognition note applies; walk Modify
  on this effect to remove the rendering, and hand off the new
  Config leaf to `relux:effect-write`.
- A wrapper effect dropped a re-exposed dep var or shell and now
  callers depending on the dep's surface fail at `relux check`.
  The wrapper-transparency rule (from `relux:effect-write`) is
  load-bearing on edits too.

**Out of scope:**

- **Authoring a new effect** -- `relux:effect-write`. If a Modify
  splits an effect into two pieces, that skill handles the new
  leaf; this skill handles the surviving original.
- **Moving an effect between modules** -- mechanical: cut the
  declaration to the new file, update imports at every caller. No
  rubric to encode; no skill.
- **Removing an effect** -- mechanical: grep every `start <E>`
  site, decide per-site (delete the caller, replace with another
  effect, inline the logic), then delete the declaration. The
  decisions are case-by-case; no rubric beyond "find the callers
  and ask the user."
- **Extracting effect logic from a test** -- that is pure
  `relux:effect-write` plus `relux:test-edit` to swap the inline
  ops for a `start <NewEffect>`. Not this skill.
- **Marker placement** on the effect -- `relux:markers`. Handoff
  after the edit if the modification introduces a need for `@skip`
  / `@flaky` / `@if`.

**Direct invocation (`/relux:effect-edit`).** Ask the user which
effect, which of the 5 dimensions are changing, and whether the
edit is part of a split (Extract). Without an effect plus at least
one dimension, the checklist has nothing to walk.

## Pre-flight checks

- [ ] **Required:** read `references/effects-identity.md`. Source
      of truth for body-section order, `expect` semantics, overlay
      evaluation, identity tuple, and lifecycle. The dedup-delta
      analysis under *Dimension 1: `expect`* relies on the identity
      tuple definition there.
- [ ] **Required:** read `references/effects-expose.md`. Source of
      truth for `expose` forms (`expose shell s [as alias]`,
      `expose var x`), caller access (`Alias.shell` / `Alias.var`),
      non-exposed shell termination, and the wrapper re-export
      rule. The surface-breakage analysis under *Dimension 2:
      `expose`* relies on it.
- [ ] **Conditional:** read `references/cleanup.md` if the edit
      touches Dimension 5. Covers fresh-shell discipline,
      don't-stop-services, don't-set-fail-patterns, idempotency,
      and the artifact-preservation rule.
- [ ] **Conditional:** read `references/fail-patterns.md` if the
      edit retunes fail patterns in a shell body. Slot scope is
      the shell; fn frames snapshot and restore. The inline-only
      rule for service shells (set in the shell body, not in a
      called `fn`) applies on edits too.
- [ ] Locate the effect (module file + line range). Read the full
      declaration verbatim before editing. Effects are
      structurally small but every section is load-bearing.
- [ ] Identify which of the 5 dimensions the change touches. Most
      edits touch 2-3. Mark all of them now; the checklist below
      depends on this list.
- [ ] **If touching `expect` or `expose`:** enumerate call sites
      and their surface references.
      - `rg "start <E>"` -- find every test or effect that
        instantiates this effect; capture the alias binding from
        each `start <E> as <Alias>` (or default name).
      - For each call site, read the containing test/effect body
        and list every `<Alias>.<shell>` / `<Alias>.<var>`
        reference *within that body's scope*. These are the
        surface references that break if the effect's surface
        shifts. A flat `rg "<Alias>\."` is not sufficient: aliases
        are scope-local, so the same identifier may legitimately
        bind to unrelated effects elsewhere in the suite, and a
        bare grep picks up those collisions as false positives.
- [ ] **If touching `expect` on a wrapper:** the dep's identity
      may also need to propagate. Re-check the dep chain --
      changes to a wrapper's `expect` set commonly require the
      same `expect` change on its dep, per the dedup-propagation
      rule from `relux:effect-write`.

## Workflow

The five Modify dimensions are a **checklist**, not a tree. Walk
each one in order; for dimensions the edit does not touch, the
walk is a one-line confirmation ("not changing -- skip"). Skipping
a dimension entirely risks missing a leak (e.g., a `let` value
that is `expose var`-d makes Dimension 4 leak through Dimension
2).

### Dimension 1: `expect`

The `expect` block. Semantics, identity tuple, and the
unique-resource taxonomy: `references/effects-identity.md`.

**Adding a var.**

- Every caller's `start <E> {...}` overlay must now supply it (or
  inherit it from the lexical env). Missing overlays surface as
  resolve-time errors at `relux check`.
- The new var enters the identity tuple. The var's *evaluated
  value* at each call site (overlay value if supplied, otherwise
  resolved from the lexical env) becomes part of identity. Call
  sites whose evaluated values differ on the new var split into
  separate instances (more setup spans, more cleanup runs); call
  sites whose evaluated values agree stay merged.
- There is no "this var is constant, so it is safe" shortcut.
  The only way to know whether dedup shifts is to evaluate the
  var at every call site and compare. A var that was *present*
  in every overlay before (and therefore looks "always set") was
  still ignored for identity; promoting it into `expect` brings
  whatever per-site differences it carries into the tuple. If
  every site happens to evaluate to the same value today, dedup
  is preserved -- but the var is now in the tuple, so any future
  caller that overrides it will split.
- If this effect is a dep of a wrapper, the wrapper's `expect`
  must usually grow to include the new var, so the wrapper can
  forward it through. Without that propagation the wrapper
  silently fixes the new var to its lexical default at every
  caller site, defeating the per-caller overlay.

**Removing a var.**

- Identity tuple shrinks. Call sites that previously held
  separate instances on this var now merge into one. **This is a
  silent change.** Two tests that ran against different ports
  (and got distinct ephemeral instances) will suddenly share a
  single instance with whichever overlay won the dedup race.
  Surface this to the user before removing.
- Callers may keep passing the var in their overlay -- legal but
  no longer identity-relevant (`references/effects-identity.md`
  > *The `expect` contract* covers the "contract, not sandbox"
  rule).

**Renaming a var (`expect old_name` -> `expect new_name`).**

- Mechanical: rename in `expect`, in any `let` / shell body
  references, and in every caller's overlay key.
- Not identity-affecting -- the tuple's *values* are unchanged.

**Required actions for any `expect` change:**

- Run the call-site grep (pre-flight item). For each site,
  classify the var's value at that site as **statically known**
  (literal, `let` of a literal, interpolation over literals) or
  **runtime-resolved** (`uuid()`, `available_port()`, `rand()`,
  `which(...)`, anything reading the effective env or a parent
  effect's `let` that itself depends on runtime values).
- For statically-known sites, **list the dedup delta** before
  the edit lands: "after this change, tests A and B will share
  one `<E>` instance; tests C and D will split." The user can
  confirm or veto on the spot.
- For runtime-resolved sites, the dedup delta is **unknowable
  statically.** Warn the user explicitly: name the call sites
  and the runtime-resolved value(s) that make them
  unanalyzable, and offer a post-change verification path --
  run the suite after the edit and read setup-span counts for
  `<E>` from `relux/out/<run-id>/<test>/events.json` (one span
  per shared instance per test). The delta surfaces in span
  counts.
- If the user confirms (statically) or accepts the post-change
  analysis offer (runtime), edit the `expect` block, then walk
  the rest of the checklist (a Dimension 1 change frequently
  forces changes in Dimensions 3, 4, 5 too).

### Dimension 2: `expose`

The effect's caller-visible surface. Forms, caller access, the
non-exposed-shell termination rule, and the wrapper re-export
rule: `references/effects-expose.md`.

**Adding an expose.**

- Additive -- no existing caller breaks. Safe.
- Confirm the new exposure is *useful* to callers per the
  Expose rubric in `relux:effect-write`: the service-running
  shell is always useful and always exposed; other shells and
  vars are exposed only when a caller has a reason to operate on
  them. The "don't expose setup-only shells" pitfall in
  `references/effects-expose.md` still applies.

**Removing an expose.**

- Surface-breaking. Every `<Alias>.<shell>` / `<Alias>.<var>`
  reference at a call site that named this exposure breaks at
  resolve time.
- The grep (pre-flight item) lists the breaks. Decide per site:
  edit the caller to drop the reference, or keep the exposure
  and revisit why removal seemed right.
- **Wrapper-transparency check.** If this effect is a wrapper,
  dropping a re-exposed dep var/shell un-layers the chain (the
  Layering Transparency rule in `relux:effect-write` is
  load-bearing on edits too). Layer-own additions can be dropped
  freely; the transparency baseline -- the dep's surface flowing
  through -- cannot.

**Renaming an expose (`expose shell s as new_alias`).**

- Update the alias on the `expose` line, then update every
  caller's `<OldAlias>.<x>` reference to `<NewAlias>.<x>`. Pure
  mechanical rename.

**Switching which shell is exposed.**

- If the previously-exposed shell was the service-running one
  and you exposed a different shell instead, the previously
  load-bearing shell now terminates -- effects' non-exposed
  shells run to end-of-block and exit. If the service was being
  kept alive by that shell's process running indefinitely,
  *the service stops*. This is almost never what the edit
  intends; surface the question to the user before the edit.

### Dimension 3: `start <Dep>` (deps)

`start <Dep>` lines and their `as <Alias>` / overlay blocks.
`start` syntax and overlay semantics: `references/statements.md`.

**Adding a dep.**

- A new identity edge. The dep's `expect` propagates: this effect
  must supply each of `<Dep>`'s expected vars via overlay (or pass
  through from its own `expect` set, which is the common case for
  wrappers).
- The new dep's exposed shells / vars become available in this
  effect's body under `<Alias>.<x>`. Use them in subsequent
  shells; do not re-expose them unless this effect is meant to be
  a transparent wrapper (the wrapper-transparency rule).

**Removing a dep.**

- The dep's `<Alias>.<x>` references in this effect's body must
  also go (or be replaced). `relux check` catches this.
- **Orphan-dep detection.** If this was the only caller of
  `<Dep>`, the dep is now unreferenced. That is not an error
  (unused effects are legal), but flag it to the user -- the dep
  is dead code if nothing else starts it.

**Replacing a dep (`start <OldDep>` -> `start <NewDep>`).**

- The new dep's `expect` set may differ from the old one. Walk
  the overlay: drop forwarded vars `<NewDep>` does not expect;
  add overlays for vars `<NewDep>` expects that `<OldDep>` did
  not.
- Cycle check: if `<NewDep>` transitively starts this effect, the
  graph cycles. Resolver catches it, but cleanest to notice
  before the edit.

**Reordering deps.**

- `start` order determines which span the others nest under in
  the structured log and which `let` / shell statements may
  reference which `<Alias>.<x>`. Reordering can break the body
  if it references aliases out of order. Identity is
  unaffected.

### Dimension 4: `let` / shell body

`let` bindings, `shell <name> { ... }` blocks, the send / match
/ sleep statements inside them.

**`let` changes.**

- Values evaluated once at instantiation (pure-context: same
  rules as `relux:function`'s `pure fn`). Pure refactors are
  safe in isolation.
- **Leak check.** If a `let` is `expose var`-d, the body-only
  edit is silently a surface change -- see
  `references/effects-expose.md` >
  *Editing a `let` silently edits the exposed surface*. Surface
  the change to the user as if it were an expose edit.

**Shell body changes.**

- Most edits are local: a new match, a different sleep, a
  reworded prompt assertion. No caller surface implication.
- **Fail patterns** must be set inline in the shell body, not in
  a called `fn`. Frame scope reverts the slot on return; see
  `references/functions.md` > *`fn`* (per-frame state) and
  `references/fail-patterns.md`.
- **Service-running shell** edits: the foreground-run discipline
  from `relux:effect-write` > *Composing the service shell*
  still applies (no `&` / `nohup` / `docker run -d`; containers
  use `docker run --rm -i`).
- **Artifact paths** under `${__RELUX_RUN_ARTIFACTS}` are
  scanned by the runtime and surfaced in `event.html` (see
  `references/project-layout.md` > Built-in environment
  variables). Writes outside that path are invisible to the
  viewer.

**Adding or removing a `shell` block.**

- A shell block's lifecycle: instantiated, body runs, exits at
  end-of-block *unless* exposed (Dimension 2). A new shell that
  is exposed becomes part of the surface (revisit Dimension 2).
  A removed shell that was exposed is a Dimension 2 change too.
- The service-running shell is always exposed and must always be
  present somewhere in the body. If the edit removes the shell
  that runs the service, ensure another shell takes that role
  before the edit lands.

### Dimension 5: cleanup

Cleanup semantics -- fresh implicit shell, reverse topological
order, the no-fail-patterns / no-service-kills / idempotency /
artifact-preservation rules -- all live in
`references/cleanup.md`. Read it before touching this dimension.
The skill-level discipline below is *when and why* to edit, not
*how* the block behaves.

**Adding cleanup.**

- Confirm one is actually needed. Most effects don't have one --
  service processes exit on shell termination; the run dir is
  preserved as a test artifact. The usual cases are external
  resources outside the run dir (real databases, cloud
  resources, shared queues / cache keyspaces).

**Removing cleanup.**

- Surface to the user the mechanism that will clean up what
  cleanup used to handle. If the answer is "nothing, the state
  will leak," the removal is wrong.

**Editing cleanup body.**

- The artifact-preservation pitfall in `references/cleanup.md`
  applies as much on edits as on initial authoring -- do not
  delete files under `${__RELUX_RUN_ARTIFACTS}`.
- **Wrapper cleanup direction:** when this effect is a wrapper,
  its cleanup runs *before* its dep's (root-to-leaf, per
  `references/cleanup.md` > *When it runs*). Unwind only what
  the wrapper added; the dep handles its own teardown.

### Extract (split): recognize + delegate

When the Modify is part of a split -- a bundled Service whose
shell body inlines config rendering, or a layer that bundles two
provisioning steps into one effect -- the operation is
**Modify + Write**, not a separate path.

**Recognition shapes** (mirror the decompositions in
`relux:effect-write`):

- **Bundled Service.** The shell body renders a config file
  inline (`<<HEREDOC`, `cat > config.toml`) and then launches the
  service that reads it. Split:
  1. Write `<Svc>Config` as a leaf via `relux:effect-write`
     (same `expect` set, renders the file, exposes
     `config_path`, no cleanup).
  2. Modify the original (this skill): remove the rendering
     statements from its shell body, add
     `start <Svc>Config as Cfg` in the deps section, and change
     the launch line to read `${Cfg.config_path}`.

- **Bundled layer.** The shell body does two distinct
  provisioning steps in sequence (launch + migrate, launch +
  seed, migrate + seed). Split:
  1. Write the **lower** layer as a fresh effect via
     `relux:effect-write`, transparently wrapping the previous
     base.
  2. Modify the original (this skill): change its `start <Dep>`
     to point at the new lower layer, remove the duplicated
     provisioning step from its shell body, keep only the steps
     this layer adds.

**Handoff sequencing.** Write the new leaf *first* (it is
self-contained; `relux:effect-write` walks its own checklist).
Then Modify here, with `start <NewLeaf>` already resolvable.
Doing the Modify first leaves the suite in an inconsistent state
where `start <NewLeaf>` does not resolve -- `relux check` would
fail and you'd be unable to verify the Modify in isolation.

**Surface preservation.** The split should be transparent to
callers: the original effect's `expose` set is the union of what
callers currently see; after the split, the surface must be at
least the same union (additions are fine; subtractions break
callers and re-enter Dimension 2 as a separate edit).

## Verify

After every Modify, in order:

```bash
relux check                 # parse, resolve, regex compile, identity wiring
```

`relux check` catches:

- Resolve-time breakage from removed `expect` vars / `expose`
  surface (Dimensions 1 and 2).
- Body refs to deleted aliases (Dimension 3).
- Malformed regex in fail patterns (Dimension 4).
- Section-order violations (`expect` after `let`, etc.).

```bash
relux run                    # mandatory after every Modify
```

`relux run` catches:

- **Identity dedup deltas** (Dimension 1) -- count the setup
  spans for this effect in `relux/out/<run-id>/<test>/events.json`
  or via the per-test `event.html` timeline. If the dedup map
  changed, the span count changed.
- **Wrapper-transparency drift** (Dimension 2) -- a re-exposed
  dep var or shell that was silently dropped surfaces as a
  resolve error in *callers' tests*, not in the effect under
  edit. The run exercises the callers.
- **Body regressions** (Dimension 4) -- fail-pattern misfires,
  prompt-detection failures, timing changes that race the new
  send/match logic. `relux check` cannot catch any of these.
- **Cleanup ordering** (Dimension 5) -- a cleanup that errors on
  already-cleaned state, or unwinds in the wrong order, surfaces
  as failure annotations in the teardown spans of any test that
  shares this effect.

A green `relux run` is the gate. A `relux check` pass on its own
is necessary but never sufficient for a Modify.

## Done when

- The intended dimensions changed and the unintended ones did
  not. (Diff review: scan for accidental edits to dimensions
  outside the user's intent.)
- Every call site that grepped during pre-flight either (a)
  still resolves under the new surface or (b) was updated in the
  same edit.
- For an `expect` change: the dedup delta was surfaced to the
  user before the edit, and the user confirmed it.
- For an `expose` removal: every `<Alias>.<x>` reference in the
  grep was edited or the removal was rolled back.
- For a wrapper edit: the transparency baseline still holds --
  every dep var/shell the dep itself exposes is still reachable
  through this effect (either re-exposed or via a transparent
  alias).
- For a split: the new leaf was authored via `relux:effect-write`
  first; the Modify here happened second; both halves' surfaces
  union to at least the original's surface.
- `relux check` passes.
- `relux run` passes -- including any tests that share this
  effect via dedup. A passing run on the directly-edited tests
  alone is not sufficient.

## Cross-skill handoffs

- `relux:effect-write` -- for the new-leaf half of any Extract
  split. Always called *before* the Modify here; Modify needs
  the new leaf to resolve.
- `relux:function` -- when the shell-body edit grows a helper.
  Authoring a helper in `relux/lib/` is `relux:function`'s
  Add path; modifying an existing helper is its Modify path.
- `relux:markers` -- when the Modify introduces a guard need
  (`# skip unless ...`, `# flaky`, `# run if ...` on the effect
  or any of its fns). Handoff after the edit lands.
- `relux:configure` -- when the dedup delta from an `expect`
  change interacts with `[run].jobs`. Splitting one shared
  instance into N per-test instances amplifies setup cost; the
  user may want to revisit parallelism.
- `relux:test-edit` -- when the Modify breaks caller tests'
  surface (a `<Alias>.<x>` reference that no longer resolves
  after this edit) and the fix is on the test side rather than
  rolling back the edit.

## References

- `references/effects-identity.md` -- body-section order,
  `expect` semantics, overlay evaluation, identity tuple,
  lifecycle. Dimension 1's dedup-delta analysis depends on it.
- `references/effects-expose.md` -- expose forms, caller
  access, non-exposed shell termination, wrapper re-export rule.
  Dimension 2's surface-breakage analysis depends on it.
- `references/cleanup.md` -- fresh-shell discipline, allowed
  operations, idempotency, artifact preservation. Dimension 5.
- `references/fail-patterns.md` -- slot scope, inline-only rule
  for service shells. Dimension 4 when retuning patterns.
- `references/functions.md` -- frame scope, pure vs impure,
  arity dispatch. Dimension 4 when the edit touches a helper.

## Pitfalls

The recurring effect-language mistakes -- expose-var leak from
body-only `let` edits, dropping a wrapper's re-exposed dep
surface, setting fail patterns inside a `fn`, cleanup that
deletes artifacts under `${__RELUX_RUN_ARTIFACTS}` -- live in
`references/effects-expose.md`, `references/fail-patterns.md`,
`references/functions.md`, and `references/cleanup.md` with
canonical Don't/Do examples. The pre-flight reads load them;
this section captures only the Modify-specific disciplines that
have no reference home.

### Editing `expect` without simulating the dedup delta

`expect` changes do not surface as `relux check` errors when the
change is identity-shifting (rather than resolve-breaking).
Adding a var that all callers happen to share at the same value
is invisible to check; *removing* a var that callers held
distinct on silently collapses N instances into one. The dedup
delta is only visible at `relux run` time, in span counts.

Don't:

```relux
# Before: expect (data_dir, port) -- two tests on distinct ports,
# distinct instances.
# After: expect (data_dir) -- two tests' overlays differ only on
# port; they now share one instance. The second test silently runs
# against the first test's port.
effect Server {
  expect data_dir
  let port = "8080"           # was: expect (data_dir, port)
  shell run {
    send "./server --data ${data_dir} --port ${port}"
  }
}
```

Do: enumerate callers, evaluate overlays under the proposed new
tuple, list the merges/splits to the user, get a confirmation
before the edit lands.

### Skipping `relux run`

`relux check` catches resolve-time breakage. It catches **none**
of the identity-dedup deltas (Dimension 1), wrapper-transparency
drift visible only at call sites (Dimension 2), body regressions
(Dimension 4), or cleanup-ordering errors (Dimension 5). A
check-only verification is silently false confidence. After a
Modify, always `relux check && relux run`; scan setup-span counts
in `events.json` for dedup deltas when Dimension 1 was touched,
and scan failure annotations across every test that shares this
effect for Dimensions 2 / 5 fallout.
