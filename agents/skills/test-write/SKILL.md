---
name: relux:test-write
description: Author a new `test` declaration -- name, docstring, `start` deps, `let` bindings, `shell` body following the command-match-anchor rhythm, optional cleanup -- with discipline about one behavior per test, test isolation, mandatory detail docstring, and context-budget management when required effects are not yet implemented. Use when the user asks to write a new test, add a relux test for a command or binary, verify a behavior with PTY matches, or assert a shell program's output. Fires on phrasings like "write a test for `<binary>`", "add a relux test that runs X and matches Y", "test that `<feature>` works", "verify `<behavior>` with a relux test", "I need a test that asserts X happens after Y". Modeling a service / dependency is `relux:effect-write` (with the hard-switch protocol below when the test needs one that does not exist yet); modifying or relocating an existing test, or splitting a fat test into siblings, is `relux:test-edit`.
---

# Author a new test

Write a `test` declaration that asserts one user-visible behavior of a
shell program. The discipline covers what the test does and does not
assert (one behavior per test), how to switch out to `relux:effect-write`
cleanly when a required service is missing, the mandatory detail
docstring, the body's command-match-anchor rhythm, when to override
defaults, and what cleanup the test does (usually none).

The five-step core -- decompose -> walk body rhythm -> author -> verify
-> audit -- handles every authoring shape; the decomposition step
classifies the test (what shell, what effects, what helpers, what
markers) and the rest of the workflow walks straight through.

## When to use

User phrasings:

- "Write a test for `<binary>`." / "Add a relux test for `<command>`."
- "Test that `<feature>` works." / "Verify `<behavior>` with a relux test."
- "I need a test that runs X and asserts Y."
- "Add a test that exercises `<edge case>`."
- "Write a test that uses Postgres and checks the migration applies."

Agent-task signals:

- The user has asked for behavioral coverage of a CLI, REPL, or
  interactive shell program and the suite does not already cover it.
- A bug report names a behavior under test; a regression test belongs
  next to the related tests.
- A new feature on the SUT lands without a covering test; the test
  belongs in the suite before the feature is considered shippable.
- The user pastes a manual reproduction (a sequence of commands and
  expected outputs) and wants it captured as an automated test.

**Out of scope:**

- **Modeling a service** -- `relux:effect-write`. When the test needs
  a service that does not exist yet, follow the hard-switch protocol
  in *Before you start: required-effect handover* below rather than
  bundling effect authoring into this skill's session.
- **Modifying an existing test** -- `relux:test-edit`. Renaming,
  retuning matches, switching shells, adding or removing `start`
  deps, relocating under a different `<sut>/<scope>/`, or splitting
  one fat test into siblings (which hard-switches *back* here for
  the new sibling) all belong there.
- **Authoring helper functions** -- `relux:function`. Helpers used in
  the test body (`compose_url`, `parse_pid`, project-local anchor
  functions like `match_pyrepl`) are that skill's territory; this
  skill writes the test that calls them.
- **Marker placement** -- `relux:markers`. Handoff after authoring if
  the test needs `# skip` / `# flaky` / `# run if`.
- **Iteration after authoring** -- the `relux check` -> `relux run`
  -> diagnose -> fix loop belongs to future `run-and-fix` (not yet
  drafted). This skill's *Verify* step runs both commands once; if
  they pass, the test is done. If they fail, the user (or the future
  `run-and-fix` skill) takes over.

**Direct invocation (`/relux:test-write`).** If the prompt
already names both the **SUT** and the **behavior**, skip this
block entirely and proceed to pre-flight / workflow. Otherwise,
the first action is to ask the user for whichever is missing,
via `AskUserQuestion` (not free-form prose). SUT first; behavior
is not framable until the SUT is pinned.

1. **Which SUT?** -- present candidates inferred from the working
   directory or any visible SPEC. Always ask SUT first.
2. **What single user-visible behavior should the test assert?**
   -- options: "I'll describe it" (the user uses the implicit
   *Other* slot to type free-form), "Smoke / basic-startup test
   for this SUT" (the user explicitly opts into a smoke test),
   and "Propose one specific behavior from the SPEC/source" (the
   agent inspects and proposes a single concrete behavior -- not
   a generic "smoke test" -- and the user confirms).

Do **not** proceed past this ask until the user has answered.
Authoring against an inferred SUT or behavior produces a guess,
not a test. The autonomous-mode pressure to "make the reasonable
call and continue without stopping" does **not** override this
-- see *Pitfalls > Silent inference from suite state*.

**After the ask, transitions are mechanical, not negotiated.**
Once SUT and behavior are pinned, downstream prerequisites
proceed *automatically* and *announced*, not re-asked: the
`relux:init` handoff (if the suite is uninitialised) and the
`relux:effect-write` hard-switch (if a service SUT has no
modelling effect) are skill-mandated, not user-decision-points.
Do **not** invoke `AskUserQuestion` a second time to ask the
user whether to do these handoffs; say "stopping for the
relux:init handoff now" (or equivalent for effect-write) and
hand off. Scope and dep services are derivable from the user's
answers; inspect the working directory and SUT source, do not
ask.

## Pre-flight checks

- [ ] **Required:** read `../../references/block-structure.md`. Source of
      truth for the test block skeleton, section ordering, the docstring
      slot, and naming rules.
- [ ] **Required:** read `../../references/statements.md`. Source of truth
      for send (`>`, `=>`), `let` / reassignment, `sleep`, comments.
- [ ] **Required:** read `../../references/matching.md`. Source of truth for
      `<?` / `<=`, the buffer/cursor model, the
      command-match-anchor rhythm, captures, and the echo trap.
- [ ] **Conditional:** read `../../references/multimatch.md` if the test
      needs to wait for several patterns in unspecified order.
- [ ] **Conditional:** read `../../references/fail-patterns.md` if the test
      sets a background guard (`!?` / `!=`).
- [ ] **Required:** read `../../references/interpolation.md`. Source of
      truth for what goes inside `${...}` (names only), bare-vs-braced,
      and the let-bind-the-call rule.
- [ ] **Conditional:** read `../../references/timeouts.md` if the test
      overrides the default match / test timeout.
- [ ] **Conditional:** read `../../references/effects-identity.md` and
      `../../references/effects-expose.md` if the test `start`s an effect.
- [ ] **Conditional:** read `../../references/cleanup.md` if the test
      needs cleanup (most tests do not).
- [ ] **Conditional:** read `../../references/functions.md` and
      `../../references/bifs.md` if the test calls helpers or BIFs.
- [ ] Locate the suite root (the directory containing `Relux.toml`).
      Tests live under `relux/tests/`. If the suite is not yet
      initialised, hand off to `relux:init` before continuing.
- [ ] Inventory the existing `relux/tests/` layout. The first
      authoring decision in *Before you start: decompose the test*
      is the file's placement (which `<sut>/<scope>/`); the
      inventory feeds that choice.
- [ ] **Classify the SUT.** The shape decides whether the SUT
      itself needs an effect:
      - **CLI / one-shot** -- exercised through the suite default
        shell; no effect.
      - **REPL / alternate shell** -- exercised via a shell
        override at file or test level; no effect, as long as the
        binary is stateless and tears down with the PTY.
      - **Service / daemon** -- has a startup / ready-wait
        lifecycle and holds state. **Requires an effect for the
        SUT itself, even with zero external deps**, since the
        effect owns start, ready-wait, expose, and cleanup. If no
        effect models it, the required-effect handover below fires.
- [ ] **Inventory external services** the behavior depends on
      (databases, queues, mock servers). Each needs an effect; a
      missing one fires the required-effect handover, independent
      of the SUT-trigger.
- [ ] Identify any **environment gates** the test needs (`# skip
      unless CI`, `# run if linux`, `# skip unless which("docker")`).
      Marker placement is `relux:markers`; this skill writes the test
      that the marker attaches to.

## Workflow

### Before you start: required-effect handover

Two independent triggers; either is sufficient:

- **SUT-trigger.** The SUT is a service / daemon and no effect
  models it. Independent of dep count.
- **Dep-trigger.** An external service the behavior depends on
  has no effect.

When either fires, **stop and propose a hard-switch to
`relux:effect-write`** before authoring anything. Do not bundle
effect authoring into this session.

Why a hard-switch and not a mid-task handoff:

- Authoring an effect is heavy work -- decomposition, identity rubric,
  expose surface, shell composition, optional provisioning chain. It
  burns a meaningful slice of the context window.
- Bundling effect-write into this session leaves the test half-formed
  when the context runs short. The user is forced to compact or reset
  mid-author and lose the in-progress test framing.
- A clean handover lets the user finish (and verify) the effect in
  its own session, compact or reset if needed, and re-enter
  `relux:test-write` fresh with the new effect available.

Propose the handover explicitly to the user: name the missing
effect(s), say what each one models, surface that effect-write is
context-heavy and a separate session is the safer move, and stop.
Do not start drafting the test body in the meantime; the test's
shape depends on the effect's surface (which shells, which vars
expose), and pre-authoring guesses will rot.

When the user returns -- in this session or a fresh one -- restart
the workflow from the top: re-walk pre-flight (the effect now
exists), then continue.

### Before you start: decompose the test

**One behavior per test.** A test asserts one user-visible behavior
of the SUT. "Reads the config and starts on the configured port" is
one behavior; "reads the config, starts on the configured port, and
serves /healthz" is two -- split them. If a single sequence of
sends-and-matches genuinely proves only one behavior, fine; if it
proves several, the test name will be vague and the failure record
will be ambiguous about which behavior broke.

The test's **name** is the one-line purpose -- read it as the
assertion the test makes. Names like `smoke`, `basic`, `it_works`
fail this rubric; names like `starts and serves health on the
configured port` pass.

**File placement.** Pick the test file path before authoring. The
rule -- two-level nesting under `relux/tests/<sut>/<scope>/.relux`,
the universal `smoke/` scope, the propose-don't-invent discipline
for non-smoke scopes, the SUT subdirectory and filename
conventions -- lives in `../../references/imports.md` > *Group tests by
SUT then scope*. Skill-level framing: this is the first concrete
commitment of the decompose step. Decide the path now (the
pre-flight inventory feeds the SUT pick; the propose-then-pick
exchange handles the scope), so the body takes shape against a
real file path rather than a placeholder.

**Test isolation.** Two tests run concurrently against the same
suite default by default. Anything they share that is mutable --
filesystem paths, network ports, database names, log files -- must
either be unique per test or come from an effect that handles the
uniqueness for them. Use `uuid()` for unique identifiers,
`available_port()` for ports, and let effects handle service-side
isolation via their `expect`-driven identity. Do not bake a hardcoded
port or `/tmp/foo` path into the test body.

**Helper extraction.** If the test body grows three or more
occurrences of the same send/match sequence, or if the SUT is a
non-POSIX shell (Python REPL, custom CLI, an interactive program
without `$?`-style exit probing), the body wants a helper -- an
extraction or a project-local anchor function. Hand off to
`relux:function` for the helper authoring, then return here with
the helper in scope. Do not inline the same N-line block twice in
the name of "just getting the test out the door".

### The body rhythm

The reference owns the operator semantics; the skill's job is to
say *which* rhythm the body walks and *where* to pull the rules
from when the body crosses a non-obvious operator.

**Command-match-anchor.** Every interaction with the SUT shell
follows the command -> match -> anchor rhythm.
`../../references/matching.md` > *The command-match-anchor loop* is the
canonical reference; read it before composing the body. The
default anchor is `match_ok()` (see `../../references/bifs.md`); when
the SUT is not a POSIX-y prompt, the body needs a project-local
anchor function -- hand off to `relux:function` to extract one
(`match_pyrepl`, etc.), then call it at every anchor site.

**Match operator pick.** Use `<? <regex>` for content matching
(captures available); use `<= <literal>` when the expected text
contains regex metacharacters and the value should be matched
verbatim. Use `<{ ... }` (multimatch) only when the SUT genuinely
produces output in non-deterministic order -- parallel jobs,
interleaved batches; for ordered output, chained `<?`/`<=` reads
clearer (`../../references/multimatch.md` > *Use multimatch only when
order is incidental*).

**Captures.** `$1`, `$2`, ... hold the captures of the most recent
`<?` in the current shell; the next `<?` clobbers them. Bind with
`let` immediately if the value is read later
(`../../references/statements.md` > *Bind captures with `let` before the
next match overwrites them*). Multimatch does **not** populate
captures (`../../references/multimatch.md` > *Captures don't bind in
multimatch*); if a value is needed from a multimatch's coverage
set, follow with a separate `<?` to extract it.

**Fail patterns.** Set `!?` / `!=` inline in the shell body that
runs against the SUT when the SUT has known fatal output
signatures (`FATAL:`, `panic:`, `Segmentation fault`,
`assertion failed`) that should fail the test immediately if seen.
The slot is shell-scoped and frame-scoped -- never wrap `!?` in a
`fn` (the slot reverts on return); see
`../../references/fail-patterns.md` and `../../references/functions.md` >
*`fn`*.

**Anchoring.** Anchor every `<?` regex to the lines it should
match (`^...$`) -- an unanchored pattern picks up the echo of the
command that produced the output, and the test reports success
on input it sent rather than output the program produced
(`../../references/matching.md` > *Echo trap*).

### Authoring the test

1. **Compose the file header.** At the top of the file, add any
   `import` statements the test body needs:

   - Effect imports for every `start <Effect>` the test uses.
   - Helper-function imports for every `fn` / `pure fn` the body
     calls from `relux/lib/`.

   Import syntax and resolution rules:
   `../../references/imports.md`. Effects use CamelCase identifiers;
   functions use snake_case (`../../references/block-structure.md` >
   *Naming rules*).

   Then add any **file-level markers** -- guards that apply to
   every test in the file (`# skip unless CI`, `# skip unless
   which("docker")`). Marker authoring is `relux:markers`; if the
   file needs a marker the file does not yet carry, hand off and
   return.

2. **Compose the test signature.** Pick the test name as the
   one-line purpose of the test (the assertion it makes), in
   plain prose, quoted.

   ```relux
   test "starts and serves health on the configured port" {
       ...
   }
   ```

   Do **not** add a test-level timeout (`~30s` / `@30s`) on the
   declaration; the suite default covers it
   (`../../references/timeouts.md` > *Don't put test-level timeouts on
   `.relux` fixtures*).

3. **Compose the docstring.** The docstring is mandatory. It is a
   `"""triple-quoted"""` block as the first statement inside the
   `test { ... }` body, holding a few lines of **detail** about
   what the test asserts and how -- not a restatement of the name.

   The docstring's job: describe the setup the test runs through,
   the inputs it passes, the matches it relies on for the
   assertion, and any non-obvious framing (which edge case the
   test exercises, why a particular service is started, why
   `uuid()` is used for isolation here). A reader who knows
   Relux but not this test should be able to read the docstring
   and predict roughly what the body does.

   ```relux
   test "starts and serves health on the configured port" {
       """
       Spawns the server with PORT=available_port(), waits for the
       "ready on :<port>" line, hits /healthz from a second shell,
       asserts a 200 response. Exercises the readiness contract
       and the port-flag wiring; nothing else.
       """
       ...
   }
   ```

   Three-line minimum is a useful floor; longer is fine when the
   test exercises non-obvious paths. Skip the docstring entirely
   and the test reads as a black box at every grep / viewer hit.

4. **Compose `start <Effect>` lines** (if needed). One per
   required service, with an `as <Alias>` binding and an overlay
   block forwarding the unique-resource vars the effect
   `expect`s. The overlay shape and identity-tuple semantics are
   in `../../references/effects-identity.md`; the exposed shells and
   vars the body can call (`Alias.shell`, `Alias.var`) are in
   `../../references/effects-expose.md`.

5. **Compose test-scope `let` bindings.** Bindings the test reuses
   across shells or that cleanup needs to see live at the
   test level (not inside a `shell` block). The visibility rules
   for cleanup are in `../../references/cleanup.md` > *Don't depend on
   test-body state inside cleanup*; if a value is needed in
   cleanup, declare its name with `let pid` at the test scope and
   reassign inside the shell body with `pid := $1`.

   Test-isolation values (`let port := available_port()`,
   `let run_id := uuid()`) belong here -- once, at the top, so
   every shell in the test sees the same value.

6. **Compose `shell` blocks.** One block per shell the test
   drives. For each block, walk the command-match-anchor rhythm
   from *The body rhythm* above:

   - Set fail patterns inline at the top if the SUT has known
     fatal signatures.
   - For each interaction: send the command (`>` / `=>`), match
     the output (`<?` / `<=` / `<{`), anchor (`match_ok()` or a
     project-local equivalent).
   - Bind captures with `let` immediately when they are read
     later in the test.
   - Override timeouts inline (`<~5s? ...`) only on the matches
     that legitimately need a longer leash; the suite default
     covers the rest (`../../references/timeouts.md`).

   A test may declare several `shell <name> { ... }` blocks (one
   per parallel shell) or re-enter the same shell name to run
   distinct phases against one PTY process
   (`../../references/block-structure.md` > *Test block*).

7. **Decide cleanup.** The default answer is no. When yes,
   `../../references/cleanup.md` owns both the rationale (cleanup
   is for state outside the test's artifact directory that
   survives shell termination) and the discipline (fresh implicit
   shell, idempotency, no service kills, no fail patterns,
   artifact preservation). Read it before writing the block; the
   skill-level contribution is only "decide now, default to no --
   most tests don't need it."

8. **Mark for external tools.** If the test (or any helper it
   calls, or any effect it starts) invokes a non-standard
   external tool (`docker`, `kubectl`, `psql`, `jq`, project
   CLIs), the test (or its file, or its dep chain) must carry a
   `# skip unless which("<tool>")` marker so hosts without the
   tool skip rather than fail. Marker placement is on the
   tightest layer that captures the real condition --
   `relux:markers` > *The layer rubric*. Hand off and return.

### Shared: Verify

```bash
relux check
```

`relux check` catches:

- Parse errors (malformed test block, out-of-order sections,
  missing docstring quotes, naming-kind violations).
- Resolver errors (unknown effect, missing import, function or
  effect identifier kind mismatch, capture used without a prior
  `<?` to bind it).
- Regex compile errors in match patterns and fail patterns.
- Marker satisfiability errors (a marker that references an
  unknown env var or pure helper).
- Overlay shape errors at each `start <Effect>` site.

`relux check` does **not** spawn shells; it cannot tell whether a
match will actually succeed against the live SUT.

```bash
relux run -f path/to/this_test.relux
```

Runs the test once. The structured log lands in
`relux/out/<run-id>/<test>/events.json`; the per-test viewer
lands in `relux/out/<run-id>/<test>/event.html` alongside the
run-summary `index.html`. Inspect the viewer for the timeline of
sends, matches, captures, and effect setup spans; the failure
panel (if any) lists the buffer state at the time of the failed
match.

A green `relux run` is the gate. A `relux check` pass on its own
is necessary but never sufficient -- a freshly authored test that
has not been run is unverified.

When the run fails, the iterate loop (`relux check` -> `relux
run` -> diagnose -> fix) belongs to future `run-and-fix`. Surface
the failure to the user; this skill ends here.

### Shared: Audit

After the run passes, re-walk the test once.

- **One behavior per test.** Read the test name out loud as an
  assertion. Does the body prove that assertion and nothing else?
  If the body proves several behaviors, the name will be vague
  -- split into separate tests.
- **Docstring present and adds detail.** The `"""..."""` block
  is the first statement in the body; it adds detail beyond the
  name (not a restatement). If the docstring reads as "X works"
  for a test named "X works", expand it.
- **No sleeps that should be matches.** `sleep` ignores
  readiness; a `<?` on the line that proves readiness is faster
  and more reliable (`../../references/statements.md` > *Prefer matches
  over `sleep`*). Audit every `sleep("Ns")` -- if a match would
  do, swap it.
- **Captures bound before clobber.** Every `${1}` / `${2}` /
  ... / `$1` / `$2` / ... reference is either in the line
  immediately after the `<?` that produced it, or has been
  bound to a `let` first (`../../references/statements.md` > *Bind
  captures with `let` before the next match overwrites them*).
- **Anchored patterns.** Every `<? <regex>` is line-anchored
  (`^...$`) unless the test deliberately matches mid-line
  content. Unanchored regex is an echo-trap waiting to happen
  (`../../references/matching.md` > *Echo trap*).
- **Fail patterns inline if set.** Every `!?` / `!=` lives in
  the shell body that owns the slot, not inside a called `fn`
  (`../../references/functions.md` > *`fn`*).
- **Test isolation.** Every shared resource the test touches --
  port, path, database name, run-id -- comes from `uuid()`,
  `available_port()`, or an effect's `expect`-driven identity.
  No hardcoded ports or paths.
- **Cleanup absent or correct.** No cleanup is the common case.
  If a cleanup block exists, it stays outside
  `${__RELUX_TEST_ARTIFACTS}/`, is idempotent, sets no fail
  patterns, and does not try to stop services
  (`../../references/cleanup.md`).
- **External-tool guard present.** If the body / its helpers /
  its effects invoke a non-standard external tool, a `# skip
  unless which("<tool>")` marker covers it on the tightest layer
  that captures the condition.
- **No test-level timeout.** The test declaration does not carry
  a `~Ns` / `@Ns` (`../../references/timeouts.md` > *Don't put
  test-level timeouts on `.relux` fixtures*).
- **File placement matches the scoping rule.** The test lives
  under `relux/tests/<sut>/<scope>/`, with `smoke/` reserved for
  smoke tests and non-smoke scopes confirmed with the user
  (`../../references/imports.md` > *Group tests by SUT then scope*).

## Done when

- The test file lives at `relux/tests/<sut>/<scope>/<name>.relux`
  with the SUT subdirectory matching the service under test, the
  scope matching the rule in `../../references/imports.md` > *Group
  tests by SUT then scope* (`smoke/` for smoke tests; otherwise a
  scope confirmed with the user), and the filename reading as
  the assertion in `snake_case`.
- The test name reads as a one-line assertion of the single
  behavior under test.
- The docstring is present, is the first statement in the body,
  and adds detail beyond the name.
- The body walks the command-match-anchor rhythm at every
  interaction with the SUT shell.
- Patterns are anchored; captures are bound before any clobber;
  fail patterns (if any) are set inline.
- Test isolation is intact: ports / paths / ids come from
  `available_port()` / `uuid()` / effect identity, not hardcoded.
- Cleanup, if present, is idempotent and outside
  `${__RELUX_TEST_ARTIFACTS}/`.
- External-tool guards are in place where required.
- `relux check` passes.
- `relux run` against the new test passes.

## Cross-skill handoffs

- `relux:effect-write` -- the **hard-switch** target when a
  required service has no effect yet (see *Before you start:
  required-effect handover*). Not a mid-session handoff; propose
  the switch to the user, surface the context-budget reasoning,
  and stop until the effect lands in its own session.
- `relux:function` -- when the body needs a helper (repeated
  send/match block, project-local anchor function for a
  non-POSIX SUT, value derivation helper). Hand off, return
  with the helper in scope.
- `relux:markers` -- when the test needs `# skip` / `# flaky` /
  `# run if`, including external-tool guards (`# skip unless
  which("...")`). Hand off after the body is in place.
- `relux:configure` -- if the suite-level match / test / suite
  timeout defaults are wrong for the suite as a whole. Per-test
  timeout overrides are an audit failure
  (`../../references/timeouts.md`); the right fix is the suite
  default.
- `relux:init` -- pre-flight handoff if the suite is not yet
  initialised (no `Relux.toml` on the ancestor chain).
- Future `run-and-fix` (Wave 4 leaf; not yet drafted) -- the
  iterate loop when `relux run` fails. This skill stops at the
  first verify; the loop is its own discipline.
- `relux:test-edit` -- two-way handoff. (a) The natural follow-up
  when the user asks to modify, relocate, or split an existing
  test rather than author a new one; this skill stops at the
  first author and verify. (b) The hard-switch *caller*: when
  `relux:test-edit` runs a split-by-extraction, it hard-switches
  here for the new sibling, then returns to strip the original.

## References

- `../../references/block-structure.md` -- test block skeleton, section
  ordering, docstring slot, naming rules.
- `../../references/statements.md` -- send (`>` / `=>`), `let` /
  reassignment, `sleep`, comments, capture-binding pitfall.
- `../../references/matching.md` -- `<?` / `<=` semantics, buffer /
  cursor model, command-match-anchor rhythm, captures, echo
  trap, anchored regex discipline.
- `../../references/multimatch.md` -- `<{ ... }` semantics, atomic
  cursor, captures-don't-bind pitfall, when to reach for
  multimatch.
- `../../references/fail-patterns.md` -- `!?` / `!=` slot semantics,
  shell-scoped, inline-only for service shells.
- `../../references/interpolation.md` -- `${var}`, `${1}`,
  `${Alias.var}`, bare-vs-braced rule, regex-interpolation-is-raw
  pitfall.
- `../../references/timeouts.md` -- `~` vs `@`, inline overrides, the
  no-test-level-timeouts rule, suite defaults.
- `../../references/effects-identity.md` -- effect lifecycle, the
  `expect` contract, the overlay shape at `start` sites.
- `../../references/effects-expose.md` -- `Alias.shell` / `Alias.var`
  access at the call site.
- `../../references/cleanup.md` -- test cleanup placement, fresh
  shell, idempotency, artifact preservation.
- `../../references/functions.md` -- `fn` vs `pure fn`, frame scope
  vs shell scope, the call-from-test-body rules.
- `../../references/bifs.md` -- `match_ok` / `match_not_ok`,
  `available_port`, `uuid`, `sleep`, control characters.
- `../../references/imports.md` -- import syntax and resolution from
  project root.
- `../../references/project-layout.md` -- `relux/tests/` location,
  built-in env vars (`__RELUX_TEST_ARTIFACTS`, `__RELUX_RUN_ID`).
- `../../references/markers.md` -- marker propagation rules; the test
  inherits markers from imported effects and functions.

## Pitfalls

The recurring test-body mistakes -- the echo trap, unanchored
regex, lost captures, sleep-instead-of-match, regex
interpolation as raw text, test-level timeouts on fixtures,
service kills in cleanup, depending on shell-scope state in
cleanup -- live in `../../references/matching.md`,
`../../references/statements.md`, `../../references/interpolation.md`,
`../../references/timeouts.md`, and `../../references/cleanup.md` with
canonical Don't/Do examples. The pre-flight reads load them;
this section captures the skill-level disciplines that have no
reference home.

### Silent inference from suite state

When invoked without a specific test in mind, the temptation is
to scan the working directory (or `relux/tests/` if a suite
exists), read a shape into the layout or the SPEC, and pick the
SUT / behavior / scope from what you find. Working-directory and
suite state carry no information about which behavior the user
wants captured next. Ask via `AskUserQuestion`; do not default.

**Autonomous-mode pressure does NOT override this.** A session
may carry a directive to "make the reasonable call and continue
without stopping" -- that directive is for *clarifying nits*,
not for *load-bearing inputs*. **SUT and behavior are
load-bearing**: a test authored against an inferred SUT or
behavior is a guess, not a test. The "reasonable call" when
these are unspecified is to ask, because there is no reasonable
call yet to make. The documented failure mode is the agent
picking "the simplest SUT" + "smoke test" as the framing and
proceeding without invoking `AskUserQuestion`; the resulting
test gets thrown away because it was never grounded in the
user's intent.

Don't (autonomous-mode rationalization -- the documented bug):

```
user: /relux:test-write
agent: [scans working dir, sees db_service.py / auth_service.py
        / task_service.py + SPEC.md]
agent: Per the session directive to make reasonable calls
       without stopping, I'll pick the most natural first test:
       a smoke test for db_service (the simplest, no deps).
       Starting with init.
```

Don't (silent inference from layout, no autonomous-mode excuse):

```
user: /relux:test-write
agent: [scans the suite, picks a behavior / SUT / scope from
        what it finds, drafts the test]
```

Do:

```
user: /relux:test-write
agent: [invokes AskUserQuestion with two questions: SUT
        (candidates from the working directory) and behavior
        (options: free-form via Other, smoke / basic-startup,
        or agent-proposed from SPEC/source). Stops until the
        user answers.]
```

### Docstring restates the name

The test name is the one-line purpose. The docstring's job is
*detail*: which inputs the test passes, which match lines carry
the assertion, why a particular service or capture is set up. A
docstring that re-says the name in slightly more words wastes
the slot and teaches every future reader that docstrings are
filler.

Don't:

```relux
test "starts and serves health on the configured port" {
    """
    This test verifies that the server starts and serves health
    on the configured port.
    """
    ...
}
```

Do:

```relux
test "starts and serves health on the configured port" {
    """
    Spawns the server with PORT=available_port(), waits for the
    "ready on :<port>" line, hits /healthz from a second shell,
    asserts a 200 response. Exercises the readiness contract
    and the port-flag wiring; nothing else.
    """
    ...
}
```

### A service SUT with no deps still needs an effect

The hard-switch has two independent triggers (SUT-trigger,
dep-trigger). The trap is to reach the handover, check only for
missing *deps*, find zero, and conclude no handover is needed --
silently demoting the SUT-trigger to a no-op when the SUT is
self-contained. A service / daemon SUT needs an effect for its
lifecycle regardless of dep count; only CLI / one-shot and
stateless REPL SUTs are exempt.

Don't:

```
user: write a test for the new metrics-server (no Metrics effect exists)
agent: [classifies SUT as a service, scans for external deps, finds
        none, concludes no effect needed, authors the test against
        the default shell with an inline start and manual ready-wait]
```

Do:

```
user: write a test for the new metrics-server (no Metrics effect exists)
agent: SUT is a service / daemon with no modelling effect. The
       SUT-trigger of the required-effect handover fires
       independently of the dep-trigger. Hard-switching to
       relux:effect-write; come back here after. Stopping.
```

### Don't bundle effect authoring into the test session

When the test needs a service the suite does not yet model,
authoring the effect and the test in one session is a
context-budget trap. Effect authoring -- decomposition, identity
rubric, expose surface, shell composition, optional provisioning
chain -- is heavy enough that the user often needs to reset or
compact before going further, and a half-formed test left in the
context evaporates with the reset.

Don't:

```
user: write a test for the seeded-postgres health-check (no Postgres effect exists)
agent: [starts drafting the test, then mid-body realises Postgres needs an effect, switches to effect-write, runs out of headroom, asks user to compact]
```

Do:

```
user: write a test for the seeded-postgres health-check (no Postgres effect exists)
agent: The suite has no Postgres effect yet. Authoring one is heavy enough that I
       recommend a hard-switch to relux:effect-write -- finish (and verify) the
       effect in its own session, compact / reset if needed, then come back to
       relux:test-write fresh and the test's shape can settle against the new
       effect's expose surface. Stopping here.
```

### One behavior per test

A test that proves N behaviors fails ambiguously when behavior K
regresses -- the failure record names the test, and the reader
has to read the body to find which assertion broke. Split each
behavior into its own test with a name that reads as the
assertion. The body for each is shorter, the failure record is
sharper, and the suite reads as a behavior catalogue rather than
a sequence of scripts.

Don't:

```relux
test "smoke" {
    """Exercises basic functionality."""
    ...50 lines covering startup, request handling, shutdown...
}
```

Do:

```relux
test "starts on the configured port" { """..."""  ... }
test "serves /healthz with 200 OK" { """..."""  ... }
test "shuts down cleanly on SIGTERM" { """..."""  ... }
```

### Test isolation is the author's job, not the runtime's

Two tests run concurrently by default. Shared filesystem paths
(`/tmp/foo`), shared ports (`8080`), shared database names
(`test_db`), and shared log files create coupling the runtime
cannot detect at parse time -- the tests pass in isolation and
fail under `-j 4`. Build isolation into the test from the start
with `uuid()`, `available_port()`, and effect identity. The cost
is one extra `let` at the top of the test; the benefit is a
suite that runs in parallel without surprise.

Don't:

```relux
test "stores the user" {
    """..."""
    shell s {
        > my-app create-user --db /tmp/users.db --name alice
        <? created user alice
        match_ok()
    }
}
```

Do:

```relux
test "stores the user" {
    """..."""
    let suffix := uuid()
    let db_path := "${__RELUX_TEST_ARTIFACTS}/users-${suffix}.db"
    shell s {
        > my-app create-user --db ${db_path} --name alice
        <? created user alice
        match_ok()
    }
}
```
