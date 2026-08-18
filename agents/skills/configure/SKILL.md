---
name: relux:configure
description: Tune `Relux.toml` knobs -- shell command/prompt, match/test/suite timeouts, parallel jobs, flaky retries -- with a strong bias toward shipped defaults. Use when the user asks to change the shell Relux spawns, bump a timeout because something raced, parallelize the suite, enable flaky retries, rename the suite, or otherwise edit `Relux.toml`. Also fires when the agent encounters a `match-timeout` cancellation in `events.json` and the obvious fix is at the manifest level rather than per-test. Insist on keeping defaults unless the user has a concrete, evidenced signal -- a timeout cancellation, a known boot time, a measured wall-clock budget. Static per-test tuning via `test "name" ~30s` is the wrong tool; this skill or CI's `--timeout-multiplier` covers slow environments.
---

# Tune Relux.toml

Adjust the suite manifest with discipline. `Relux.toml` ships every
field defaulted; the right edits come from evidence (a cancellation,
a measured budget), not from a hunch. The branches below cover every
knob the manifest exposes -- pick the one the user's motivation
points at and apply only that one.

## When to use

User phrasings:

- "Bump the match timeout to 30s." / "My tests keep timing out."
- "Run tests in parallel." / "Use 4 workers."
- "Enable retries for flaky tests." / "Set flaky retries to 3."
- "Switch the shell to bash." / "Change the prompt."
- "Rename the suite to `myproj-e2e`."

Agent-task signals:

- A test's `events.json` reports `cancelled` with `reason.type ==
  "test-timeout"` or a `match-timeout` failure, and the racing
  operation isn't a single localized match (where an inline
  `<~Ns? pat` would be the better fix).
- The user has just finished `relux:init` and asked what to tune.
- Wall-clock duration of the suite has crossed `[timeout].suite`
  legitimately (test count grew, not because tests got slower).
- A test is marked `# flaky` but `[flaky].max_retries` is 0, so the
  marker has no retry effect.

**Out of scope:** test-level `~Ns` / `@Ns` on `.relux` fixtures (the
suite default covers them -- see `../../references/timeouts.md` >
*Don't put test-level timeouts on `.relux` fixtures*), per-match
inline `<~Ns?` overrides (write directly in the test file when one
specific match needs more time), and the CI `--timeout-multiplier`
flag (CI tuning, not manifest tuning -- see `../../references/ci-integration.md`).
Environment *values* are not a `Relux.toml` knob -- they belong in a
committed `.env` (see `../../references/environment.md`); this skill tunes
the manifest, not the environment.

**Direct invocation (`/relux:configure`).** Ask the user
which knob and what motivated the change before editing. Possible
classes: shell (command/prompt), timeout (match/test/suite), jobs,
flaky (max_retries/timeout_multiplier), name. The decision tree below
branches by class; without a class plus a motivation, the default-bias
rule fires and the answer is "keep the default."

## Pre-flight checks

- [ ] **Required:** read `../../references/project-layout.md` >
      *`Relux.toml`* -- the field table is the source of truth.
- [ ] Locate the suite root by walking upward for `Relux.toml`. The
      file may be partially present (some sections set, others
      commented). Read it verbatim before editing.
- [ ] Identify the knob the user wants to tune and the *evidence*
      driving the change. "It feels slow" is not evidence; a
      `match-timeout` in `events.json` is. If there is no evidence,
      stop and recommend keeping defaults.
- [ ] If tuning a timeout, read `../../references/timeouts.md` >
      *Two kinds* and *Configuration defaults* -- the `~` vs `@`
      distinction determines whether CI's multiplier will help.

## Decision tree

Pick the branch matching the knob class. **Apply one knob at a time;
verify before stacking another change.**

### Shell command or prompt (`[shell]`)

Fields and defaults: `../../references/project-layout.md` > *Sections*.

- Change `command` only when the SUT requires a specific shell (bash
  for here-strings, dash for POSIX-strict tests, custom binary).
  **Warn the user:** only `/bin/sh` is exercised by the upstream
  Relux test suite. Other shells (bash, zsh, fish, busybox ash) may
  interact badly with the prompt sentinel, line-editing escapes,
  or the way Relux drives the PTY -- this is not a tested
  configuration. Treat any non-default shell as experimental;
  expect to debug prompt-detection and fail-pattern edge cases.
- Change `prompt` only when the default `relux> ` collides with text
  the SUT prints. The prompt is a sentinel; collisions cause
  spurious matches. Any unique short string works.

**Discipline.** A different shell changes timing characteristics and
fail-pattern matching. Re-run the suite after the change; do not
chain other tuning until the new shell baseline is clean.

### Timeouts (`[timeout]`)

Fields and defaults: `../../references/project-layout.md` > *Sections*.
`~` vs `@` scope and what the multiplier touches:
`../../references/timeouts.md`.

**Escalation order -- any timeout-related symptom.** Walk the steps
in order; do not skip ahead. Each step is narrower in blast radius
than the next. Reaching for a global bump before exhausting steps
1 and 2 penalises every match in every test on every run.

1. **Tune the problematic operation inline.** A single slow match
   gets `<~Ns? pattern` on its own line. One-line, scope-local.
2. **Apply CI's `--timeout-multiplier`** for host-class slowness --
   cold caches, slower CI agents, shared-runner contention. The
   multiplier scales every tolerance (`~`) timeout for one run
   without touching the manifest.
3. **Bump global defaults in `Relux.toml`** only when the slowness
   is suite-wide -- most matches across most tests legitimately
   need more time than the defaults provide.

Knob-specific discipline:

- `match` is the suite-wide step-3 knob.
- Raise `test` only when an individual test legitimately runs
  longer than the default; most slow-test problems are slow-match
  problems in disguise (handled by `match`).
- If `suite` fires before any individual test times out, the
  answer is `jobs` (parallelism), not a larger budget.

### Parallel jobs (`[run]`)

Field and default: `../../references/project-layout.md` > *Sections*.

- Raise above the default only when the suite is genuinely
  parallel-safe: no shared filesystem paths, no fixed ports, no
  shared external services, no test ordering dependencies.
- Each test gets its own shells and artifact dir, but external
  state (a real database, a real port, a shared cache directory)
  is the user's responsibility.

**Discipline.** Pick `jobs = 2` first and run the suite three times
to confirm stability before raising further. Flakes that appear only
at `jobs > 1` are races in test setup or effect identity, not
nondeterminism to mask with retries.

### Flaky retries (`[flaky]`)

Fields and defaults: `../../references/project-layout.md` > *Sections*.
`timeout_multiplier` formula and the `~` vs `@` scope:
`../../references/timeouts.md` > *Flaky multiplier*.

- The default `max_retries = 0` means `# flaky` markers are
  recorded but have no retry effect. Set to `1` or `2` only when
  (a) tests are marked `# flaky` for known, isolated reasons
  (network jitter, kernel-scheduled delays), and (b) the user
  has decided masking those races is the right business call.

**Discipline.** Flaky retries are a confession of unknown
nondeterminism. They mask the symptom; they do not fix the race.
Before enabling retries, ask the user whether `relux history --flaky`
analysis has been run -- if not, that is the better first step. The
diagnose-flaky skill (Wave 2 leaf; not yet drafted) owns that
analysis.

### Suite name (root)

Field and default: `../../references/project-layout.md` > *Sections*.
Set explicitly when the directory name is generic (`tests/`) or
the suite participates in a multi-suite repo and needs a distinct
identifier in reports. Cosmetic; does not affect discovery,
identity, or execution.

### Port allocation window (`[available_ports]`)

Fields and defaults: `../../references/project-layout.md` > *Sections*.
Narrow `range_start` / `range_end` only when the suite must stay inside a
specific port band (a firewalled CI range, a fixed Docker port-forward).
For the allocation semantics and why the window sits outside the OS
ephemeral range, see `../../references/bifs.md` > *`available_port` ports
are owned by the running test*.

## Verify

After every edit:

```bash
relux check                 # manifest still parses, tests still resolve
```

For timeout / jobs / flaky edits, run the suite once and confirm the
intended effect:

```bash
relux run
```

- Timeouts: the previously-racing operation passes (or, equivalently,
  no new operations now exceed budget).
- `jobs > 1`: outcomes are stable across two or three back-to-back
  runs. If a previously-passing test now fails intermittently, the
  fix is in the test or its effect (isolation), not a higher
  multiplier.
- Flaky retries: the test's outcome trail in `relux/out/<run-id>/`
  shows the retry attempts -- the marker is now active.

## Done when

- The single intended knob is set to the intended value, and no other
  manifest field was touched.
- `relux check` passes against the updated manifest.
- For timeout / jobs / flaky edits, `relux run` confirms the change
  had the intended effect (or surfaced a deeper issue that this skill
  cannot fix from the manifest).
- If the user wanted to change multiple knobs, each one was applied
  and verified independently in series, not as a single bundled edit.

## Cross-skill handoffs

- `relux:init` -- the natural caller. Init defers all tuning to this
  skill.
- Future: diagnose-flaky -- pre-empt enabling `[flaky].max_retries`
  by analysing existing run history (Wave 2 leaf, not yet drafted).

## References

- `../../references/project-layout.md` -- the `Relux.toml` field table and
  layout pitfalls.
- `../../references/timeouts.md` -- `~` vs `@`, inline `<~Ns?` overrides,
  per-test annotations, and which knobs scale with the multiplier.
- `../../references/ci-integration.md` -- `--timeout-multiplier` and
  `--flaky-multiplier` flags, the right place to absorb
  environment-specific slowness without rewriting the manifest.
- `../../references/environment.md` -- where environment values live
  (`.env`), as opposed to the manifest knobs this skill tunes.

## Pitfalls

### Don't tune without evidence

The defaults work for most suites. Every deviation is a future
maintenance liability and a slightly less portable suite. A user
saying "I want a longer timeout" without a concrete `match-timeout`
in `events.json` is asking for a fix they don't yet need. Surface the
question: what cancellation or measurement is driving this?

Don't:

```toml
# "Let's just bump everything to be safe."
[timeout]
match = "30s"
test = "30m"
suite = "1h"
```

Do:

```toml
# Across the whole suite, most match operations settle in 4-6s
# (measured: 38 of 42 tests waited 4s+ on different disconnected
# matches).
# Suite-wide pattern, not one localized match -- bump the default.
[timeout]
match = "10s"
```

The *Escalation order* note at the top of the Timeouts branch covers
single-match and single-host symptoms; this pitfall is about the
no-evidence case.

### Don't tune `[flaky]` to mask a race

`# flaky` plus `max_retries = 2` is a flag that says "we know this
test sometimes loses to a race we have not characterized." It is a
business decision, not a debugging tool. Before enabling retries,
ask the user whether the underlying nondeterminism is known and
deliberately ignored -- if not, the right next step is diagnose-flaky
(not yet drafted) against the existing run history, not editing the
manifest.

### Don't bundle multiple knob changes into one edit

The shape "increase match, enable retries, raise jobs, all at once"
hides which change caused which outcome shift. If the suite goes
green, you don't know which knob earned it; if a test fails, you
don't know which knob broke it. Apply one change, verify with
`relux run`, then apply the next.
