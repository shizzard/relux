---
name: relux:effect-write
description: Author a new `effect` declaration -- `expect` set, deps, expose surface, shell body, and cleanup -- with discipline about identity (one effect per service, unique-resources rule), config decomposition (Config + Service when the service rejects ENV), and the wrapper-effect re-expose rule. Use when the user asks to write a new effect, add an effect for a service the suite does not yet manage, wrap an existing effect with seed data / migrations / post-init steps, or decompose a file-configured service into Config + Service. Fires on phrasings like "write an effect for X", "add a Postgres effect", "wrap Db with seeded data", "this service needs a config file -- how do I model it". Modifying, moving, or removing existing effects is out of scope (future leaves, not yet drafted); this skill writes new declarations.
---

# Author a new effect

Write an `effect` declaration that models a single external service --
the SUT, or a dep of it. The discipline covers what goes in `expect`
(identity = unique resources the service contends for), how the body
decomposes when the service rejects ENV-shaped configuration (Config +
Service), which shells survive setup (`expose`), whether cleanup is
needed (usually no), and how wrapper effects re-export their dep
shells.

The five-step core -- decompose -> pick path -> walk body sections ->
verify -> audit -- handles every authoring shape; the path (Plain or
Wrap) determines which body sections the author touches and which
disciplines apply at each.

## When to use

User phrasings:

- "Write an effect for `<service>`." / "Add a Postgres effect."
- "Wrap `Db` with seeded data." / "I need a `SeededDb` on top of `Db`."
- "This service reads a config file -- how do I model it?"
- "Add an effect that runs migrations before the API starts."
- "Set up a fresh Redis instance for each test."

Agent-task signals:

- A test wants to `start <Effect>` but the effect does not exist yet.
- An existing effect's shell body bundles config-file rendering with
  the service launch -- the Config + Service decomposition applies.
- An existing effect's shell body bundles two unrelated services
  (e.g., spawns both Postgres and Redis) -- the
  one-service-per-effect rule applies; split.
- A wrapper effect was authored but a caller cannot reach the wrapped
  dep's shell -- the re-expose rule was missed; this skill's Wrap
  path covers it.

**Out of scope:** modifying an existing effect's body, `expect` set,
or `expose` set; removing an effect; moving an effect between
modules. Those are refactoring operations against a live caller set
and belong to future `effect-edit` / `effect-move` skills (Wave 2
leaves; not yet drafted). Marker placement on the effect is
`relux:markers` (handoff after authoring if a guard is needed);
authoring helper functions used inside the effect's shells is
`relux:function`.

**Direct invocation (`/relux:effect-write`).** Ask the user which
service the effect models, whether it accepts ENV-only configuration
or needs a config file, and whether it layers on top of an existing
effect (Wrap) or stands alone (Plain). Without those three pieces,
the decomposition step has nothing to walk.

## Pre-flight checks

- [ ] **Required:** read `references/effects-identity.md`. Source of
      truth for body-section order, `expect` semantics, overlay
      evaluation, identity tuple, and lifecycle.
- [ ] **Required:** read `references/effects-expose.md`. Source of
      truth for `expose` forms, caller access (`Alias.shell` /
      `Alias.var`), non-exposed shell termination, and the wrapper
      re-export rule.
- [ ] **Required:** read `references/cleanup.md`. Covers cleanup
      placement, allowed operations, shell visibility, fresh-shell
      discipline, and the don't-stop-services /
      don't-set-fail-patterns / idempotency pitfalls.
- [ ] Identify the service the effect will model -- the actual
      binary, container, daemon, or external resource this effect
      manages.
- [ ] Identify whether the service is ENV-configurable or requires a
      config file. If the latter, the Config + Service decomposition
      applies (see *Before you start* below) and you will write two
      effects, not one.
- [ ] Identify whether this effect layers on top of an existing
      effect (Wrap) or stands alone (Plain).
- [ ] Identify whether the service needs **provisioning beyond
      launch** -- schema migrations, test fixtures, topic creation,
      seeded users, bucket layout. If yes, the provisioning chain
      decomposition applies (see *Before you start* below) and the
      proposed chain must be confirmed with the user before
      authoring any layer.
- [ ] List the **unique external resources** the service contends
      for -- filesystem paths (data dir, log dir, socket paths),
      network resources (ports, host bindings), logical shared state
      (database names, S3 prefixes, Redis keyspace prefixes). These
      are the candidates for `expect`.

## Workflow

### Before you start: decompose the service first

Every effect models **one service**. The unit is a single binary,
container, daemon, or external resource that the effect spawns or
layers on top of. Effects that try to manage two peer services at
once are a layering violation -- split into two effects with a dep
chain (or two unrelated effects, if the services have no real
relationship).

Two decompositions decide what you are writing before any code lands.

**ENV is the preferred wiring.** Services that accept ENV-only
configuration get a simple Plain effect: the test passes the unique
resources at the `start` site (overlay), the rest of the config
travels as transparent inherited ENV, and the effect's shell reads
both seamlessly. Default to this shape; only decompose further when
the service forces your hand.

**Config + Service decomposition.** Some services require
configuration via a file (TOML, YAML, ini) rather than ENV. The wrong
move is to render that file from inside the service effect's setup
shell -- it couples the file-format quirk to the service launch and
muddies what the test author has to set. The right move is to split:

1. Write a **`<Svc>Config`** leaf effect first.
   - `expect`s the **same unique-resource set** as `<Svc>` (the
     dedup-identity rule applies through the chain: under-list and
     two `<Svc>` instances with different resources would silently
     share one `<Svc>Config` instance, rendering the wrong file;
     over-list and dedup fragments needlessly).
   - Renders the config file to a deterministic artifact path under
     `${__RELUX_RUN_ID}/<svc>/config.<ext>`.
   - Exposes the full path as a let-bound var:
     `let config_path = "..."` then `expose var config_path`.
   - **No `cleanup`** -- the rendered file is a meaningful test
     artifact for post-mortem inspection.

2. Write **`<Svc>`** second.
   - `expect`s the same unique-resource set.
   - `start <Svc>Config as Cfg` forwarding the expected vars.
   - Reads `${Cfg.config_path}` in its setup shell to launch the
     service.
   - Does **not** re-expose `Cfg.config_path` -- it is internal
     plumbing, not part of `<Svc>`'s public interface.

After decomposition, both halves use the **Plain** path; `<Svc>`
just happens to have one `start` dep on `<Svc>Config`.

**Provisioning chain decomposition.** Most real services need
*provisioning* before they are useful for testing -- a database
needs schema migrations; a Kafka cluster needs topic creation; an
S3 bucket needs an initial directory layout; an auth service needs
seeded users. Provisioning falls naturally into a sequence of
layered steps; each step becomes its own effect that wraps the
previous.

**Ask the user what provisioning the service needs**, then propose
the chain shape before implementing:

1. List the provisioning steps in execution order from leaf to
   root: launch -> schema -> fixtures.
2. Name each step as the cumulative state of the service at that
   layer: `Database`, `DatabaseWithMigrations`,
   `DatabaseWithTestData`.
3. **Surface the proposed chain to the user and get sign-off
   before authoring any layer.** The user may collapse two steps
   ("no fixtures yet"), add a step (separate "migrate schema" and
   "migrate data" passes), or rename steps. Do not write code
   until the shape is confirmed.

For a database with config + service + migrations + test fixtures,
the chain reads root -> leaf as:

```
test (root)
  -> DatabaseWithTestData      # seeds test fixtures
    -> DatabaseWithMigrations  # applies schema migrations
      -> Database              # runs the daemon (service-owning)
        -> DatabaseConfig      # renders postgresql.conf (leaf)
```

Four effects, each starting the next, each re-exposing the
underlying service's surface so the test can talk to a fully
provisioned database via `start DatabaseWithTestData as Db`.

Why N effects, not one effect with N setup shells:

- **Dedup wins.** A test that needs the schema but not the
  fixtures starts `DatabaseWithMigrations` directly and skips the
  fixture layer's setup cost. A test that needs only an empty
  database starts `Database`. Granularity is opt-in.
- **Observability.** Each layer is its own setup span in
  `events.json`. When fixtures fail to seed, the viewer pinpoints
  the failing layer; one bundled effect produces a single setup
  span with all output flattened.
- **Cleanup ordering.** Each layer can carry its own cleanup
  (rarely needed -- see *Shared: Cleanup*); reverse-topological
  teardown ensures higher-level state is gone before lower-level
  state.

**Layering is transparent.** The caller of `DatabaseWithTestData`
must see at least the surface they would see starting `Database`
directly -- same shell names, same var names. Layering effects
pass through everything the service-owning effect exposes:

- **`expect` passes through.** Every layer in the chain `expect`s
  the same unique-resource set as the service-owning layer
  (identity-tuple discipline from the rubric below), forwarded
  at each `start <Inner> as Dep { ... }` site.
- **Exposed shells pass through under their original names.** Each
  layer re-exposes every shell the underlying service exposed,
  via `expose shell Dep.<name>` (no `as` rename). The caller
  reaches `Db.service` regardless of which layer they started.
- **Exposed vars pass through under their original names.** Same
  rule for `expose var Dep.<name>`.

A layer **may add its own exposures on top** of the dep's
surface when the layer itself produces something genuinely useful
to callers -- typically a `let` value the layer computes (an
applied schema version, a seeded user's id, a generated
credential), and occasionally a shell of its own. The Expose
rubric's general rule applies at every layer: expose only what
the caller has a reason to operate on; do not expose what they do
not. For most layering effects, "what the caller needs" collapses
to "the dep's re-exposed surface" -- the layer's own exposures
are empty. Sometimes they are not, and that is fine.

The **layering effect's own shell** is the one that does this
layer's provisioning work (running migrations, seeding fixtures,
creating topics). In most cases it does **not** appear in any
`expose` -- it runs during setup, performs its work against the
dep's exposed shells, and terminates. The caller does not need
to operate on a "migration shell". Expose it only when callers
genuinely need to send commands against the provisioning layer
itself; rare but not prohibited.

The layer's own shell **may still write provisioning artifacts**
under `${__RELUX_RUN_ID}/<svc>/<layer>/` regardless of expose --
artifact-writing is independent of whether the shell is exposed.
The migration log, the applied schema dump, even copies of the
migration SQL files themselves -- whatever rounds out the
artifact set so a post-mortem on a failed test can see exactly
what state the service was in at each layer. Apply the same
artifact-mapping discipline from *Shared: Composing the service
shell* to provisioning shells: configure tools to write under the
run dir, redirect at the shell level when they do not.

After decomposition + transparency, each layer in the chain is
mechanically a **Wrap** effect against the layer below. Building
the chain is N successive applications of the Wrap path; the
transparency baseline (re-export the dep's surface) is mandatory,
the layer's own additions are optional.

### The layer rubrics

Two design picks apply once you know how many effects you are
writing.

**Identity (`expect` set).** `expect` lists the variables that name
unique external resources the effect *contends for*. Concretely:

- **Filesystem resources** -- data dirs, log dirs, socket paths,
  lockfile paths.
- **Network resources** -- ports, host bindings, named pipes.
- **Logical shared state** -- database names in a shared cluster,
  S3 prefixes, Redis keyspace prefixes.

Everything else stays out of `expect`: log levels, debug flags,
feature toggles, internal timeouts. Those ride inherited env
transparently; the resolver does not gate on them and they do not
fragment dedup. Two instances with the same `PORT` + different
`LOG_LEVEL` are the same instance; the second `start` is a `reused`
setup.

When you need N independent instances of an effect that has no
natural resource differentiator, add an `INSTANCE` discriminator to
`expect` and pass distinct values per `start` site -- the
instance-id *is* the unique resource. See
`references/effects-identity.md` > *Force a fresh instance with a
dummy overlay key*.

The rule reframes the rubric from "what matters for behaviour"
(squishy) to "what would two simultaneous instances physically
collide on" (concrete). The latter has an answer for every effect.

**Expose surface.** Decide which shells survive setup and what
publishes to callers.

- **The service-running shell must always be exposed.** This is the
  shell that holds the actual service process. Without
  `expose shell ...`, the shell terminates when setup completes and
  the service dies with it -- the effect is useless. Every other
  expose decision follows from "does the caller need to operate on
  this shell?"; the service shell is the one non-negotiable expose.
- Other shells the caller will operate on must be exposed too.
  Non-exposed shells terminate when setup completes.
- Setup-only shells (init, migrations, seed) **must not** be exposed
  -- leave them out so they free resources. See
  `references/effects-expose.md` > *Don't expose setup-only shells*.
- Let-bound values that callers need to read get
  `expose var <name>`. The target must be a `let` in the same
  effect; expose-as-passthrough of an overlay var requires a let
  shim (`let port = PORT` then `expose var port`).
- Names default to the local name; rename via `as` when the caller
  should read a different identifier (`expose shell s as service`).

### Shared: Composing the service shell

Three disciplines apply to the shell that runs the actual service,
regardless of path.

**Run the service in the foreground.** PTY (shell) termination kills
the process tree the shell launched. A backgrounded service
(`./run.sh &`, `nohup ...`, `setsid ...`) detaches from that tree
and survives the shell's death -- the test ends but the service
keeps running, the next test trips over leftover ports, sockets,
lockfiles, or data files. Always launch the service foreground; the
service runs until the PTY dies, and PTY death is how Relux
guarantees teardown.

For containerised services this means foreground mode *and*
auto-removal of the container resources:

Don't:

```relux
shell svc {
    > docker run -d --name pg-${PORT} postgres
    <? ready
}
```

Do:

```relux
shell svc {
    > docker run --rm -i postgres
    <? listening on
}
```

`--rm` removes the container layer on exit; `-i` keeps it attached
to the PTY so termination propagates. No `-d`, no backgrounding, no
nohup. The same rule applies to other containerisation systems
(Podman, containerd, systemd-nspawn) -- foreground + auto-cleanup.

**Map meaningful service artifacts into the run directory.** Most
services produce artifacts worth keeping for post-mortem -- logs,
core dumps, query traces, intermediate state. Anything written
under `${__RELUX_RUN_ID}/<svc>/` is scanned by the artifact scanner
and surfaced in `event.html`; anything written elsewhere is
invisible to the viewer and lost when the host's `/tmp` or
`/var/log` rolls.

**Ask the user which service artifacts are meaningful** (logs are
the canonical case), then configure the service to write them under
the run dir:

```relux
shell svc {
    > postgres -D ${PG_DATA_DIR} \
        -c log_directory=${__RELUX_RUN_ID}/postgres/ \
        -c log_filename=postgres.log
    <? listening
}
```

For services that hard-code their log path, redirect at the shell
level (`> svc > ${__RELUX_RUN_ID}/svc/svc.log 2>&1`) or symlink the
hard-coded path into the run dir during setup. The goal is that
when a test fails, the relevant log lands beside `events.json` and
the viewer renders it inline.

**Fail patterns: ask, then set inline.** Ask the user whether the
service has known fatal output signatures -- panic markers, segfault
prints, `FATAL:` / `panic:` / `Segmentation fault` lines -- that
should immediately fail any test interacting with the service. If
yes, set the fail pattern **inline in the service shell**, before
the readiness match:

```relux
shell svc {
    !? FATAL
    > postgres -D ${PG_DATA_DIR}
    <? listening
}
```

The fail pattern lives in the shell's slot, which persists from
setup into every `shell Alias.svc { ... }` block in the test -- the
inheritance happens because the *shell* is exposed (its slot
travels with it). Do **not** wrap `!?` in a `fn`; the fail-pattern
slot is frame-scoped (`references/functions.md` > *`fn`*), so the
slot reverts when the function returns and the guard never reaches
the test.

If a negative test legitimately needs to emit the fail-pattern
string (e.g., exercising the panic path), clear the slot inline
before that section with `!?` (no payload) and re-set it afterward
(`!? FATAL` again). See `references/fail-patterns.md` > *Clear the
slot before reusing the shell for unrelated work*.

### Path: Plain

Use when the effect models a single service that you can launch
directly (no user-facing wrapping). Includes the `<Svc>` half of a
Config + Service decomposition (the dep on `<Svc>Config` is internal
plumbing, not a wrap).

1. **Write the header comment block.** Above the `effect` line, list
   the ENV variables the body reads, split into expected and
   transparent:

   ```relux
   # effect Postgres
   #
   # expected (overlay; participate in identity):
   #   PG_DATA_DIR  -- pgdata path; unique per instance
   #   PG_PORT      -- listen port
   #
   # transparent (inherited env; set by caller):
   #   PG_USER, PG_DB  -- credentials and target database
   #   PG_LOG_LEVEL    -- verbosity
   ```

   The test author wires the `start` site against this block; it is
   the effect's public contract for what the caller must provide.

2. **Compose `expect`.** Use the identity rubric. List only the
   unique-resource vars.

3. **Compose `let` bindings.** Derive any paths or computed values
   the shells or `expose var`s will need. `let` may read expected
   vars and call `pure fn` / pure BIFs.

4. **Compose `start` deps** (if any). For the `<Svc>` half of a
   Config + Service decomposition, this is
   `start <Svc>Config as Cfg` forwarding the same expected vars.

5. **Compose `expose`.** Which shells survive, what vars publish.

6. **Compose `shell` bodies.** Setup logic that brings the service
   online: spawn, wait for readiness via `<?` / `<=`, set fail
   patterns inline for known error signatures. For the
   service-running shell, walk *Shared: Composing the service shell*
   above -- foreground run (no `&` / `nohup` / `docker -d`),
   container auto-removal (`--rm`), artifact mapping into
   `${__RELUX_RUN_ID}/`, and inline fail patterns. Helper functions
   for repetitive sequences belong in a library module -- handoff to
   `relux:function`.

7. **Decide cleanup** (most often: no). See *Shared: Cleanup* below.

8. **Mark for external tools.** If the body invokes a non-standard
   external tool (`docker`, `pg_ctl`, `kubectl`, `psql`, project
   CLIs), the effect must carry a `# skip unless which("<tool>")`
   marker -- handoff to `relux:markers` (Add path). The marker
   propagates to every test that `start`s this effect.

Then run **Verify**, then **Audit**.

### Path: Wrap

Use when the effect layers on top of an existing dep effect (seed
data, migrations, post-init steps) and callers need to interact with
both the wrapped dep's shells and (optionally) the wrapper's own.

1. **Write the header comment block** (same shape as Plain).

2. **Compose `expect` mirroring the dep's `expect`.** The wrapper's
   identity must be a strict superset of (or equal to) the dep's --
   same unique resources, plus any the wrapper itself contends for.
   Under-list and dedup desyncs across the chain; over-list without
   a real new resource fragments dedup. Most wrappers' `expect`
   exactly equals the dep's.

3. **`start <Dep> as <Alias>`** forwarding the expected vars:

   ```relux
   effect SeededDb {
       expect DATA_DIR, PORT
       start Db as Dep {
           DATA_DIR
           PORT
       }
       ...
   }
   ```

4. **Re-expose the dep's full surface.** Load-bearing wrapper rule
   -- applies to both shells and vars. `start Db as Dep` runs `Db`
   and gives the wrapper dot-access to `Dep.<name>`, but does
   **not** surface anything of `Dep` to whoever starts the wrapper.
   Without `expose shell Dep.<name>` / `expose var Dep.<name>`,
   callers cannot reach `Db`'s shells or vars -- alive (held by the
   wrapper's guard) but invisible. See
   `references/effects-expose.md` > *Wrapper effects must re-expose
   dependency shells*.

   For provisioning-chain layers, transparency is the rule: every
   shell and var the dep exposed must be re-exposed under its
   original name, no `as` rename. The caller reaches `Db.service`
   regardless of which layer they started:

   ```relux
   expose shell Dep.service    // original name; transparent passthrough
   expose var Dep.port         // same
   ```

   `as` rename is reserved for cases where the wrapper genuinely
   reshapes the surface (e.g., a non-chain wrapper that renames a
   shell from `Dep.s` to a caller-facing `service`).

5. **Compose the wrapper's own shell.** This is where the layering
   work happens (running migrations against `Dep.service`, seeding
   data, etc.). Reads `${Dep.var}` for any values published by the
   dep. If the wrapper itself runs a long-lived service (not just
   a setup shell), the *Shared: Composing the service shell*
   discipline applies in full -- foreground, container `--rm`,
   artifact mapping, inline fail patterns.

6. **Expose the wrapper's own shell only if callers need to operate
   on it.** Most wrappers do their layering in a setup-only shell
   and only re-expose the dep's shells -- the caller starts
   `SeededDb` to get a seeded `Db`, not to send commands to the
   seeder. For provisioning-chain layers, the same rule applies:
   the layer's own shell is typically not exposed (transparency's
   baseline is the dep's surface), but the layer may add its own
   exposures -- a computed `let` var, or occasionally a shell --
   when genuinely useful to the caller.

   The layer's own shell **may still write provisioning artifacts**
   under `${__RELUX_RUN_ID}/<svc>/<layer>/` regardless of expose
   -- migration logs, applied-schema dumps, copies of the migration
   SQL itself. Apply the artifact-mapping discipline from *Shared:
   Composing the service shell*; expose is about caller interaction,
   artifact-writing is independent of expose.

7. **Decide cleanup** (most often: no). See *Shared: Cleanup* below.

8. **Mark for external tools** (same rule as Plain step 8).

Then run **Verify**, then **Audit**.

### Shared: Cleanup

Ask "do I even need cleanup?" first. The answer is usually no.

Cleanup runs in a fresh implicit shell after the effect's exposed
shells terminate; it cannot stop the service (PTY death already
killed children) and cannot see buffer/captures/shell-scoped `let`s
from setup. Cleanup is for **filesystem side effects that live
outside `${__RELUX_RUN_ID}/`** -- sockets in `/tmp`, files in
`/var`, anything the next test would trip over.

Anything written under `${__RELUX_RUN_ID}/` is the run's intentional
output, scanned by the artifact scanner, and surfaced in
`event.html` for post-mortem. **Do not delete it in cleanup.** This
is why the `<Svc>Config` leaf typically has no cleanup -- the
rendered config file is exactly what you want to inspect when a
run fails.

Cleanup runs in reverse topological order: the test (root) tears
down first; effects tear down root -> leaf. A leaf's cleanup runs
last, after every dependent has already cleaned up.

See `references/cleanup.md` > *Pitfalls and best practices* for the
worked examples.

### Shared: Verify

```bash
relux check
```

`relux check` catches:

- Body-section ordering errors (`expect` first, `cleanup` last,
  etc.).
- Resolver errors (unknown dep, missing import, `expose var`
  against a non-let, identifier collisions).
- Overlay shape errors at any `start` site (the resolver validates
  every `start <Effect>` provides the `expect`-declared vars).
- Identity-tuple resolution errors when overlay expressions cannot
  evaluate.

For an exercising test that `start`s the effect:

```bash
relux run path/to/test_using_effect.relux
```

The effect appears in the structured log under a setup span; on
reuse, subsequent `start`s show as `reused` setup spans (no
re-execution). Inspect `event.html` to verify the dep chain, the
artifact files (a `<Svc>Config` file should be visible if you
decomposed), and the cleanup ordering.

### Shared: Audit

After authoring, re-walk the effect and its caller surface.

- **Header comment block present and accurate.** Every ENV var the
  body reads (in `expect` *or* via `${VAR}` interpolation in any
  shell / cleanup) is listed under the right section (expected vs
  transparent). Grep the body for `${...}` references; cross-check
  against the comment block.
- **`expect` lists only unique-resource vars.** Any var listed that
  does not name a filesystem / network / logical shared resource
  (or an instance discriminator) should move to transparent
  inherited env.
- **Non-exposed shells terminate intentionally.** Every `shell`
  block not in `expose shell ...` is setup-only by design. If a
  shell is not exposed but a test needs it, fix the expose, not
  the caller.
- **Wrapper re-exposes dep shells and vars.** If this is a Wrap,
  every dep shell *and* var the caller could have reached by
  starting the dep directly must be re-exposed.
- **Provisioning chain transparency.** For chain layers, every
  re-export from the dep uses the original name (no `as` rename);
  the layer's own provisioning shell is exposed only when callers
  genuinely need to operate on it (rare); any layer-specific
  exposures the wrapper adds (a computed `let` var, an occasional
  own shell) pass the general "useful for the caller" rule from
  the Expose rubric; if the provisioning step produces meaningful
  artifacts (logs, applied schema), the layer's shell writes them
  under `${__RELUX_RUN_ID}/<svc>/<layer>/`.
- **External-tool guard.** If the body invokes a non-standard
  external tool (`docker`, `pg_ctl`, `psql`, `kubectl`, project
  CLIs), confirm the effect carries a `# skip unless which("...")`
  marker. The marker propagates to every test that `start`s this
  effect; missing it means tests on hosts without the tool fail
  loudly instead of skipping. Handoff to `relux:markers` (Add
  path) if missing.
- **No cleanup deleting artifacts.** Any `cleanup` block that
  touches paths under `${__RELUX_RUN_ID}/` is removing test
  evidence. Pull those lines.

## Done when

- The effect declares the right body sections in the right order,
  passing `relux check`.
- `expect` lists only unique-resource vars (or an instance
  discriminator); transparent ENV stays out.
- For Config + Service: both halves exist, both `expect` the same
  set, `<Svc>` reads `${Cfg.config_path}` in its shell, and the
  rendered config file is a documented test artifact under
  `${__RELUX_RUN_ID}/`.
- For Wrap: every dep shell *and* var the caller could have reached
  by starting the dep directly is re-exposed; the wrapper's own
  shells are exposed only when callers need to operate on them.
- For provisioning chains: the proposed chain shape was confirmed
  with the user before any layer was authored; each layer
  re-exposes the dep's surface under original names; the layer's
  own provisioning shell is typically not exposed (exposed only
  when the caller genuinely needs to operate on it), and any
  layer-added exposures pass the general "useful for the caller"
  rule; meaningful provisioning artifacts write under
  `${__RELUX_RUN_ID}/<svc>/<layer>/`.
- The service-running shell is exposed; the service launches in the
  foreground (no `&` / `nohup` / `docker -d`); containerised
  services use `--rm` for auto-cleanup.
- Meaningful service artifacts (logs, dumps) write under
  `${__RELUX_RUN_ID}/<svc>/`; the user has confirmed what is
  worth preserving for post-mortem.
- If the service has known fatal output signatures, a `!?` / `!=`
  guard is set inline in the service shell (not in a function --
  the slot is frame-scoped).
- The header comment block lists every ENV var the body reads,
  split into expected and transparent.
- Non-standard external tools the body invokes are guarded by a
  marker (handoff to `relux:markers` completed).
- Cleanup, if present, touches only state outside
  `${__RELUX_RUN_ID}/`, is idempotent, and does not set fail
  patterns.

## Cross-skill handoffs

- `relux:function` -- for helpers used in shell bodies (`parse_pid`,
  `compose_url`, retry loops). Authoring helpers is `relux:function`'s
  territory; this skill writes the effect declaration that uses
  them.
- `relux:markers` -- for `# skip unless which("...")` guards on
  effects that invoke non-standard external tools, or `# flaky` /
  `# run if` guards on effects with environment-specific
  preconditions. Hand off after authoring.
- `relux:configure` -- `[run].jobs` and effect dedup interact;
  parallel runs amplify the cost of fragmented `expect` sets.
- Future `test-write` (Wave 2 leaf; not yet drafted) -- the natural
  caller of this skill. Future `effect-edit` / `effect-move` (Wave
  2 leaves; not yet drafted) -- for Modify / Remove / Move
  operations not covered here.

## References

- `references/effects-identity.md` -- body-section order, `expect`
  semantics, overlay evaluation, identity tuple, lifecycle.
- `references/effects-expose.md` -- expose forms, caller access,
  non-exposed shell termination, wrapper re-export rule.
- `references/cleanup.md` -- cleanup placement, fresh-shell
  discipline, idempotency, don't-stop-services /
  don't-set-fail-patterns pitfalls.
- `references/block-structure.md` -- overall effect block shape and
  section-ordering rules.
- `references/statements.md` -- `start` syntax, overlay semantics,
  `${Alias.var}` interpolation.
- `references/markers.md` -- effect-level marker propagation
  (markers on effects reach every test that `start`s them).
- `references/project-layout.md` -- the `${__RELUX_RUN_ID}/`
  artifact directory layout and built-in env vars.

## Pitfalls

### Don't bundle multiple services into one effect

Two peer services (an API and a database; a web server and a cache)
get two effects with a dep relationship, not one effect with a setup
shell that spawns both. The one-service-per-effect rule makes the
dep graph explicit, makes dedup work correctly, and lets the
cleanup chain run in the right order.

Don't:

```relux
effect ApiWithDb {
    shell setup {
        > pg_ctl start
        <? ready
        > my-api &
        <? listening
    }
    expose shell setup
}
```

Do:

```relux
effect Db { ... expose shell db }

effect Api {
    expect API_PORT
    start Db as Dep { DATA_DIR; PORT = 5432 }
    expose shell Dep.db as db
    shell svc {
        > my-api --db-port=5432
        <? listening
    }
    expose shell svc
}
```

### Don't put non-identity ENV in `expect`

Listing `LOG_LEVEL`, `DEBUG`, or `RUST_LOG` in `expect` fragments
dedup -- two tests with different log levels each pay the full
setup cost. The effect still reads `${LOG_LEVEL}` from inherited
env; it just must not gate identity on it.

Don't:

```relux
effect Service {
    expect PORT, LOG_LEVEL, DEBUG
    ...
}
```

Do:

```relux
# transparent: LOG_LEVEL, DEBUG
effect Service {
    expect PORT
    ...
}
```

### Don't omit the header comment block

Transparent ENV is invisible at the `start` site -- the test author
has no syntactic hint that the effect's shell reads `${RUST_LOG}`
or `${PGUSER}`. Document the contract above the `effect` line so
the author wiring a new `start` knows what to set.

Don't:

```relux
effect Postgres {
    expect PG_DATA_DIR, PG_PORT
    shell db {
        > pg_ctl -D ${PG_DATA_DIR} -o "-p ${PG_PORT}" start
        > psql -U ${PG_USER} -d ${PG_DB}    // undocumented reads
    }
}
```

Do:

```relux
# effect Postgres
#
# expected (overlay; participate in identity):
#   PG_DATA_DIR  -- pgdata path; unique per instance
#   PG_PORT      -- listen port
#
# transparent (inherited env; set by caller):
#   PG_USER, PG_DB  -- credentials and target database
effect Postgres { ... }
```

### Don't use cleanup to delete artifacts

Anything under `${__RELUX_RUN_ID}/` is the run's intentional output,
visible in `event.html` for post-mortem. Deleting it in cleanup
removes the evidence that makes failures debuggable. Cleanup is for
state that lives *outside* the run dir and would leak otherwise.

Don't:

```relux
cleanup {
    > rm -rf ${__RELUX_RUN_ID}/postgres/    # deletes config + logs
}
```

Do:

```relux
cleanup {
    > rm -rf /tmp/postgres-${PG_PORT}.sock    # outside run dir; OK
}
```

### Don't background the service process

A backgrounded service detaches from the PTY's process tree and
survives shell termination. Relux's teardown guarantee is PTY death
killing the process tree -- breaking that turns every test failure
into a leak: leftover processes hold ports, sockets, and lockfiles
that trip the next run. For containers, `-d` (detached) is the same
mistake; `--rm` without `-d` is the right shape.

Don't:

```relux
shell svc {
    > ./run.sh &                    # detached; outlives shell
    > docker run -d --name pg ...   # same trap
    > nohup my-service &            # explicit detach
}
```

Do:

```relux
shell svc {
    > ./run.sh                      # foreground; PTY death stops it
    > docker run --rm -i ...        # foreground container; auto-cleanup
}
```

If the service self-daemonises (forks into the background by
default), pass the flag that keeps it foreground (`-F`, `-D`,
`--foreground`, depending on the service). Almost every long-lived
daemon has one.

### Don't author a wrapper without re-exposing dep shells

A wrapper that seeds data into `Dep.service` but does not
`expose shell Dep.service as service` leaves the caller unable to
talk to the underlying service -- the shell is alive (held by the
wrapper's guard) but unreachable. The test starts `SeededDb` to get
a seeded `Db`; if `Db`'s shell is invisible, the wrapper is
useless.

Don't:

```relux
effect SeededDb {
    start Db as Dep
    expose shell seed    # Db.service unreachable to callers
    shell seed {
        > psql -c "INSERT INTO ..."
    }
}
```

Do:

```relux
effect SeededDb {
    start Db as Dep
    expose shell Dep.service    # original name; caller reads as 'service'
    shell seed {
        > psql -c "INSERT INTO ..."
    }
    # seed terminates after setup; caller does not operate on it
}
```
