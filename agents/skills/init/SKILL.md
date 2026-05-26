---
name: relux:init
description: Bootstrap a Relux test suite in a project that does not yet have one -- create `Relux.toml`, the `relux/{tests,lib,out}` layout, and a smoke test. Use when the user asks to start using Relux in a repo, scaffold a new suite, add Relux to an existing project, set up Relux from scratch, or initialize tests in a directory. Also fires when the agent is about to author a `.relux` file in a project where `Relux.toml` does not exist anywhere on the ancestor chain. Idempotent: if a suite already exists at or above the current directory, hand off to `configure-relux-toml` for tuning rather than re-scaffolding (Wave 2 leaf; not yet drafted).
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
configure-relux-toml skill's territory (Wave 2 leaf; not yet drafted).
If the user wants a new *module* (test / effect / library file) inside
an existing suite, that is the write-test / write-effect /
write-library-fn skills' territory (Wave 2 leaves; not yet drafted).

**Direct invocation (`/relux:init`).** No clarification needed;
the workflow detects whether a suite already exists by walking upward
from the current directory. If one exists, the skill exits with a
pointer to configure-relux-toml. If not, the current directory becomes
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
      path to the user and hand off to configure-relux-toml. Do not
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
during init; tuning belongs to configure-relux-toml.

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

Close by briefing the user on the four subcommands they will reach
for daily. One sentence each, an example invocation, no deeper detail
-- `references/cli-reference.md` is the source of truth when they need
flags.

- **`relux new`** -- scaffold a new test, effect, or library module.

  ```bash
  relux new --test login          # relux/tests/login.relux
  relux new --effect postgres     # relux/lib/postgres.relux
  relux new --lib helpers/curl    # relux/lib/helpers/curl.relux
  ```

- **`relux check`** -- parse and resolve every `.relux` file under the
  suite without spawning shells. The fast feedback loop while
  authoring.

  ```bash
  relux check
  ```

- **`relux run`** -- execute tests. With no arguments, runs the whole
  suite; `--file` narrows to specific modules or directories.

  ```bash
  relux run
  relux run --file relux/tests/smoke.relux
  ```

  Each invocation writes `relux/out/<run-id>/` with `index.html` and
  per-test `events.json` / `event.html`.

- **`relux history`** -- aggregate analysis across past runs. The
  entry point for flakiness, first-fail bisection, and duration
  trends.

  ```bash
  relux history --flaky
  relux history --first-fail
  relux history --durations
  ```

  Reads every `run_summary.toml` under `relux/out/`. Useless on a
  fresh suite -- mention it now so the user knows where to go later,
  not as something to run today.

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
- Future: the configure-relux-toml skill (handoff when `Relux.toml`
  already exists at or above the cwd, or after init if the user wants
  to tune shell / timeouts / jobs) and the write-test skill (natural
  next step once the smoke test passes) -- both Wave 2 leaves, not yet
  drafted.

## References

- `references/project-layout.md` -- Relux.toml fields, directory
  convention, file discovery rules, layout pitfalls.
- `references/cli-reference.md` -- `relux init`, `relux new`,
  `relux check`, `relux run` argument shapes.

## Pitfalls

### Don't scaffold inside an existing suite

`relux init` refuses to overwrite an existing `Relux.toml` in the cwd,
but it does not walk upward. Running it inside `existing-suite/sub/`
creates a *nested* manifest, which `references/project-layout.md`
documents as a sub-project boundary -- discovery from the outer suite
will silently stop at the inner manifest. The pre-flight ancestor walk
catches this; do not skip it.

Don't:

```bash
cd existing-suite/sub
relux init                   # creates a nested Relux.toml; outer suite loses sub/
```

Do:

```bash
# Pre-flight detected Relux.toml at existing-suite/.
# Hand off to configure-relux-toml; do not re-init.
```

### Don't edit `Relux.toml` during init

The default manifest is intentionally empty (every field has a
default). Editing it during init couples scaffolding to tuning and
makes the first commit harder to review. Leave defaults; let
configure-relux-toml own the tuning step when the user has a concrete
reason to deviate (custom shell, longer timeouts, parallel jobs).

### Don't author tests before `relux check` passes

If the smoke test fails to check after scaffold, something is wrong
with the template, the binary, or the layout. Authoring a real test on
top of an unverified scaffold buries the original fault under new
material. Get `relux check` clean against the smoke test before moving
on.
