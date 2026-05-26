---
name: relux:init
description: Bootstrap a Relux test suite in a project that does not yet have one -- create `Relux.toml`, the `relux/{tests,lib,out}` layout, and a smoke test. Use when the user asks to start using Relux in a repo, scaffold a new suite, add Relux to an existing project, set up Relux from scratch, or initialize tests in a directory. Also fires when the agent is about to author a `.relux` file in a project where `Relux.toml` does not exist anywhere on the ancestor chain. Idempotent -- if a suite already exists at or above the current directory, hand off to `relux:configure` for tuning rather than re-scaffolding.
---

# Bootstrap a Relux suite

Scaffold the manifest, directory layout, and an initial smoke test in a
project that does not yet have a Relux suite. The CLI does the file work
(`relux init`, `relux new --test`); this skill is the discipline around
*when* to invoke them, *where* the suite lives, and *what* the first
authoring move should be.

## When to use

User phrasings:

- "Set up Relux in this repo."
- "Start a Relux suite here." / "Scaffold tests for this project."
- "Add Relux to this codebase." / "Initialize a test suite."
- "I want to start writing Relux tests."

Agent-task signals:

- The user has asked you to author a `.relux` file but `Relux.toml` does
  not exist anywhere on the ancestor chain of the current working
  directory.
- `relux check` returns a "no manifest found" error and the project has
  no Relux history.
- A fresh clone of a project with documentation describing a Relux suite
  but where the suite was never committed.

**Out of scope:** if `Relux.toml` already exists, this skill does
nothing -- adjusting shell, timeouts, jobs, or flaky retries is the
`relux:configure` skill's territory. If the user wants a new *module*
inside an existing suite, that is the `relux:test-write` /
`relux:effect-write` / `relux:function` skills' territory; organizing
the `relux/lib/` directory is a future `lib-organize` leaf (not yet
drafted).

**Direct invocation (`/relux:init`).** No clarification needed;
the workflow detects whether a suite already exists by walking upward
from the current directory. If one exists, the skill exits with a
pointer to `relux:configure`. If not, the current directory becomes
the suite root.

## Pre-flight checks

- [ ] `relux --version` -- confirms the binary is on PATH. If not, hand
      off to the `relux:install` skill before continuing; the workflow
      below assumes a working `relux` binary.
- [ ] `pwd` -- the suite root will live here. Confirm the path matches
      the user's intent (project root, not a subdirectory).
- [ ] Walk the ancestor chain for an existing `Relux.toml`:

      ```bash
      D="$PWD"
      while [ "$D" != "/" ]; do
        [ -f "$D/Relux.toml" ] && { echo "found at $D"; break; }
        D="$(dirname "$D")"
      done
      ```

      If found, **stop**: the suite is already initialized. Surface the
      path to the user and hand off to `relux:configure`. Do not
      re-scaffold.

- [ ] Check the cwd for a `relux/` directory that might predate the
      manifest -- a partial scaffold from a prior aborted run. If
      present, surface to the user before overwriting.

## Workflow

### 1. Scaffold the layout

```bash
relux init
```

This creates:

- `Relux.toml` with every default commented out (an empty manifest is
  valid).
- `relux/tests/` and `relux/lib/` (empty).
- `relux/.gitignore` containing `out/` so per-run artifacts are not
  committed.

The default manifest is empty on purpose -- every field has a default
documented in `references/project-layout.md`. Do not edit `Relux.toml`
during init; tuning belongs to `relux:configure`.

### 2. Add a starter smoke test

```bash
relux new --test smoke
```

This writes `relux/tests/smoke.relux` with a single test that runs
`echo hello-relux` and matches the output. The point of the smoke test
is twofold: it gives `relux check` something to validate, and it gives
the user a known-passing baseline to point new tests at.

If the user has a specific first test in mind (an entry point for the
real authoring work), use that name instead of `smoke`. The template is
identical regardless.

### 3. Validate

```bash
relux check
```

Confirms the manifest parses, the test file lexes/parses, and the
resolver is happy. A failure here points at a malformed default
template, a stale `relux` binary, or a stray `Relux.toml` in a
subdirectory (the layout pitfall in `references/project-layout.md`).

### 4. First run

Ask the user whether they want to run the smoke test now. On consent:

```bash
relux run
```

The first run produces `relux/out/<run-id>/` with per-test artifacts
(`events.json`, `event.html`, `index.html`). Surface the path to
`index.html` so the user can open the run summary.

### 5. Wire `.gitignore`

`relux init` writes `relux/.gitignore` to exclude `out/`. If the
project has a root `.gitignore`, append `relux/out/` to it as well so
the exclusion survives `git clean -fd` from the project root and
matches conventions for repos that consume sub-`.gitignore` files
differently.

```bash
# If a root .gitignore exists and does not already mention relux/out/:
grep -qE '^relux/out/?$' .gitignore || echo 'relux/out/' >> .gitignore
```

### 6. Follow-ups

- Offer the `relux:editor-plugin` skill if the user has not yet
  installed `.relux` syntax support. Do not invoke unprompted.
- Point the user at the suite tutorial at
  <https://shizzard.github.io/relux/latest/suite-tutorial/> for a
  guided next step.

### 7. Tour the CLI surface

Close by naming the four subcommands the user will reach for daily,
in one sentence each. Flags and full shapes live in
`references/cli-reference.md`; don't restate them.

- **`relux new`** -- scaffold a new test, effect, or library module.
- **`relux check`** -- parse and resolve every `.relux` file without
  spawning shells; the fast feedback loop while authoring.
- **`relux run`** -- execute tests, writing `relux/out/<run-id>/`
  with `index.html` plus per-test `events.json` / `event.html`.
- **`relux history`** -- aggregate analysis across past runs
  (flakiness, first-fail bisection, duration trends). Mention it
  now so the user knows where to go later; useless on a fresh
  suite.

## Done when

- `Relux.toml` exists at the cwd (now the suite root).
- `relux/tests/` and `relux/lib/` exist.
- `relux/.gitignore` excludes `out/`; the project-root `.gitignore`
  (if any) excludes `relux/out/`.
- A starter test compiles -- `relux check` passes.
- The user has been offered a first run (and either declined or seen
  the run summary).
- The user has been pointed at the suite tutorial.
- The user has seen the one-sentence brief on `relux new` /
  `relux check` / `relux run` / `relux history`.

## Cross-skill handoffs

- `relux:install` -- pre-flight handoff when `relux --version` fails.
- `relux:editor-plugin` -- offered after step 5 on consent.
- `relux:configure` -- handoff when `Relux.toml` already exists at or
  above the cwd, or after init if the user wants to tune shell /
  timeouts / jobs.
- `relux:effect-write` -- natural next step once the smoke test
  passes. A real test virtually always starts by modelling the SUT
  (or its dependencies) as an effect, and `relux:test-write` would
  hard-switch back here anyway if no effect exists yet; entering
  via `relux:effect-write` skips the round trip.

## References

- `references/project-layout.md` -- Relux.toml fields, directory
  convention, file discovery rules, layout pitfalls.
- `references/cli-reference.md` -- `relux init`, `relux new`,
  `relux check`, `relux run` argument shapes.

## Pitfalls

The nested-manifest rule (a `Relux.toml` inside another suite
becomes a sub-project boundary that breaks outer-suite discovery)
lives in `references/project-layout.md` with the canonical
layout sketch. The pre-flight ancestor walk is the discipline
that keeps `relux init` from creating one; do not skip it.

### Don't edit `Relux.toml` during init

The default manifest is intentionally empty (every field has a
default). Editing it during init couples scaffolding to tuning and
makes the first commit harder to review. Leave defaults; let
`relux:configure` own the tuning step when the user has a concrete
reason to deviate (custom shell, longer timeouts, parallel jobs).

### Don't author tests before `relux check` passes

If the smoke test fails to check after scaffold, something is wrong
with the template, the binary, or the layout. Authoring a real test on
top of an unverified scaffold buries the original fault under new
material. Get `relux check` clean against the smoke test before moving
on.
