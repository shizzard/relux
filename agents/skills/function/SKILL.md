---
name: relux:function
description: Add, modify, remove, move, or extract `fn` / `pure fn` declarations. Use when the user asks to author a helper function, extract repeated inline code into a function, change a function's body or arity, promote a `fn` to `pure fn` (or demote), move a function between inline modules and `relux/lib/`, or delete a function. Fires on phrasings like "extract this into a helper", "make this a pure function", "move this to lib", "inline this single-caller helper", "add a default-argument version of `http_request`", "delete this function -- nothing uses it". The pure-vs-impure split decides what the function can do; the location decision (inline vs `relux/lib/`) decides who can import it. Marker placement on functions is the `relux:markers` skill's territory; this skill authors the declaration the marker attaches to.
---

# Author and edit functions

Add, modify, remove, move, or extract `fn` / `pure fn` declarations in a
Relux module -- with discipline about purity, location, arity, and what
crosses (or does not cross) the call boundary. The same five-step core
(classify -> pick layer -> apply -> verify -> audit) handles every
operation; step 1 classifies and the rest of the workflow adapts.

## When to use

User phrasings:

- "Extract this into a helper function." / "Pull this out into a function."
- "Make this a pure function." / "This should be a `pure fn`."
- "Add a helper that does X."
- "Move this helper to `lib/`." / "Promote this to a library function."
- "Inline this -- only one test calls it."
- "Add a default-argument version of `http_request`."
- "Rename this function."
- "Delete this function -- nothing uses it."

Agent-task signals:

- About to repeat the same N-line block across two or more tests / effects
  (an Extract opportunity).
- A `pure fn` body uses a shell op or impure BIF -- fails `relux check`.
- A `fn` is referenced from a marker expression or a top-level `let` --
  those positions only see `pure fn` and pure BIFs; promote if the body
  permits, refactor otherwise.
- A function defined inline is being imported from a second module --
  promote to `relux/lib/`.
- A call site's argument count does not match any defined arity -- either
  add the missing arity (idiomatic defaults pattern) or fix the call.

**Out of scope:** marker placement on functions is `relux:markers`
(handoff after authoring if the new function needs a guard); tuning
timeouts or fail-patterns is inline `<~Ns? pattern` for one match and
`relux:configure` for global defaults; authoring tests is
`relux:test-write`; authoring effects is `relux:effect-write`;
reorganizing the `relux/lib/` directory is a future `lib-organize`
leaf (not yet drafted).

**Direct invocation (`/relux:function`).** Ask the user which operation
(add / modify / remove / move / extract), the target file path, and the
function name. Without an operation plus a target, the workflow has
nothing to walk.

## Pre-flight checks

- [ ] **Required:** read `references/functions.md`. The reference is the
      source of truth for `fn` vs `pure fn` semantics, frame-scoped vs
      shell-side state, arity-based dispatch, and the call-boundary
      rules; this skill assumes that material is loaded.
- [ ] **Required:** read `references/bifs.md`. The pure-vs-impure split
      at the BIF level decides what a `pure fn` body can call -- the
      reference enumerates which built-ins are pure (usable inside
      `pure fn`) and which are impure (force the function to be `fn`).
      The purity rubric below assumes this material is loaded.
- [ ] **Classify the operation:** Add, Modify, Remove, Move, or Extract.
      If the user said "change this function" but the *location* is what
      should change, classify as Move (not Modify).
- [ ] Identify the target -- which file, which function name, which
      arity (if the function already exists in multiple arities).
- [ ] For Add / Extract: decide **purity** and **location** using the
      rubrics below.
- [ ] For Modify / Remove / Move: list every caller via grep across the
      project. Functions are looked up by `(name, arity)` within an
      importing scope; a caller in any module that imports this function
      is in scope.
- [ ] If the function is referenced from a marker expression or a
      top-level `let` RHS, the function must be `pure fn`. A `fn` will
      not resolve in those positions.

## Workflow

Pick the operation path based on the classification done in pre-flight.
**Verify** and **Audit** are shared phases run at the end of every path.

### The layer rubrics (used by Add, Move, Extract)

Two axes need a pick when placing a function.

**Purity.** (Pure vs impure BIF lists in `references/bifs.md`;
callable positions for each form in `references/functions.md`.)

- Body only manipulates strings, computes values, or calls other
  `pure fn`s / pure BIFs (`trim`, `uuid`, `which`, etc.) -- write
  `pure fn`.
- Body sends to a shell, matches against output, sets timeout or
  fail-pattern, or calls impure BIFs (`sleep`, `match_ok`,
  ctrl-keys) -- must be `fn`.

The discipline is not "pure is nicer"; it is "if it can be pure,
future callers will need it pure." `pure fn` is callable from
marker / overlay / `let` RHS / shell positions; `fn` is callable
only inside a shell block. Defaulting to `pure fn` keeps options
open.

**Location.**

- Used inside a single module: inline in that module. No import dance.
- Used across two or more modules: a library file, imported via
  `from "lib/<path>" import fn_name`. The grouping rubric
  (effect-scoped vs protocol-scoped vs ask-before-creating-misc)
  lives in `references/imports.md` > *Group library modules by
  scope*; apply it before placing the file.

### Path: Add

Use when no function with this `(name, arity)` exists at the target.

1. **Pick purity** using the rubric.
2. **Pick location** using the rubric.
3. **Compose the signature.** `fn name(arg1, arg2)` or
   `pure fn name(arg1, arg2)`. All arguments are strings (no type
   system); snake_case is parse-enforced.

   Before committing the parameter list, scan each argument: does it
   have a value most callers will use? If yes, the argument is
   *defaultable*. Two consequences:

   - **Place defaultable arguments last.** Required arguments first
     (every caller supplies them); defaultable arguments at the tail,
     ordered most-defaultable last. The signature reads left-to-right
     from "must specify" to "can omit." The ordering is load-bearing
     -- Relux dispatches by `(name, arity)` with no
     skip-to-named-argument syntax, so a defaultable argument stuck in
     the middle cannot be omitted by a shorter arity; every caller is
     forced to pass the default explicitly, which kills the point.
   - **Extract the lower-arity function.** For each defaultable
     argument, define a shorter arity that omits it and delegates to
     the full arity with the default supplied. Repeat against the
     shorter arity: if it has further defaultable arguments, extract
     another shorter arity from it. The result is a chain
     `name(a)` -> `name(a, b)` -> `name(a, b, c)`, each adding one
     parameter, with defaults living exactly once in the delegating
     arity. See `references/functions.md` > *Use arity-based dispatch
     for default arguments*.

4. **Compose the body.** Return value is the last expression. For
   `pure fn`, the body is `let` bindings followed by an expression --
   no shell ops, no impure BIFs, no calls to `fn`. For `fn`, leave the
   caller's shell clean (consume the prompt; `match_ok()` after any
   command).

   **Don't wrap the return in a redundant `let`.** When the result is
   just an interpolation or a single function call, write it as the
   last line; naming it first adds noise (see
   `references/functions.md` > *Arguments and returns* on implicit
   `""`).

   Don't:

   ```relux
   pure fn compose_url(host, port) {
       let url = "http://${host}:${port}/api"
   }
   ```

   Do:

   ```relux
   pure fn compose_url(host, port) {
       "http://${host}:${port}/api"
   }
   ```

   The same applies when the body delegates to another function --
   `fn http_get(url) { http_request("GET", url) }`, not
   `fn http_get(url) { let r = http_request("GET", url) r }`. `let`
   bindings earn their place when an intermediate value is reused, or
   when naming makes a multi-step body readable; not for the final
   expression.

5. **Apply.** Write the declaration. For library location, also add
   `from "lib/<name>" import fn_name` to each caller module.

Then run **Verify**, then **Audit**.

### Path: Modify

Use when a function exists and its body, arity, or purity needs to
change. If the *location* is what should change, switch to the **Move**
path instead.

1. **Locate** the declaration -- read it verbatim, including its arity.
2. **Diagnose what is changing:**
   - *Body* (re-implementing what the function does at the same arity):
     edit in place. If the rewrite crosses the purity line (introduces
     or removes shell I/O), continue with *Purity* below.
   - *Arity* (adding or removing a parameter): Relux dispatches by
     `(name, arity)`. A "signature change" is mechanically **add a new
     arity**; never rewrite an existing arity's parameter list in
     place -- existing callers silently bind to a different function
     (or fail to resolve) the moment the parameter count changes. The
     new arity coexists with the old; both remain valid call paths.
     Migrating existing callers onto the new arity is a *separate*,
     optional decision -- not a mandatory next step. Two arities
     reading as "give me the simple form" and "give me the
     fully-parameterized form" is the defaults pattern (see
     `references/functions.md` > *Use arity-based dispatch for default
     arguments*), not a half-finished migration. Migrate only when the
     lower arity's default is itself wrong (callers are working around
     it) or when pruning the lower arity earns its keep against the
     cognitive cost of leaving it. Otherwise leave both in place; each
     call site picks the arity that fits.
   - *Purity* (`fn` -> `pure fn`): only valid if the body uses no shell
     ops, no impure BIFs, and calls no `fn`. `pure fn` -> `fn` (a
     demotion) is mechanically always allowed but breaks every
     pure-only call site -- `pure fn` was the original choice for a
     reason, and demotion silently disqualifies the function from
     positions where `fn` cannot stand in.
     Before demoting `pure fn` -> `fn`, grep for pure-context call
     sites: top-level `let` RHS (in tests, effects, modules), marker
     expressions (`# skip unless ...`, `# run if ...`, conditional
     markers), overlay expressions, and bodies of other `pure fn`s
     that call this function. If any exist, demoting will break them.
     Consider a **wrapper** instead: leave the existing `pure fn`
     intact and add a new `fn some_name(args)` (different name
     *or* different arity) that performs the shell work and calls
     through to the pure helper for the value derivation part. Pure
     callers keep working; impure callers use the wrapper. Propose
     this shape to the user before committing the demote.
   - *Rename* (same arity, new name): mechanically Add-the-new +
     migrate callers + Remove-the-old. For a single arity with all
     callers in one module, an in-place edit + grep-rename is fine.
3. **Apply.** Edit the declaration. For arity changes, the
   delegating-arity pattern (smaller arity calls larger arity with a
   default) is the idiomatic Relux defaults story -- see
   `references/functions.md` > *Use arity-based dispatch for default
   arguments*.

Then run **Verify**, then **Audit**.

### Path: Remove

Use when the function should be deleted entirely.

1. **Locate** the declaration. Note the arity -- removing one arity of a
   multi-arity function leaves the others intact.
2. **List callers** via grep across the project.
3. **Pick a remove strategy:**
   - *Zero callers* -- delete the declaration directly.
   - *One caller* -- inline the body back into the caller (substitute
     each parameter reference with the literal argument from the call
     site), then delete the declaration and the call expression.
   - *N>1 callers* -- the right operation is probably Move (to
     consolidate to `lib/`) or replace-with-equivalent (point each
     caller at another existing helper), not Remove. Reclassify.
4. **Apply.** Delete the declaration and, for the single-caller case,
   clean up the call site.

Then run **Verify**, then **Audit**.

### Path: Move

Use when the function stays in spirit but should live at a different
location (inline -> `relux/lib/`, library file -> another library file,
or back).

1. **Locate** the source declaration.
2. **Pick the destination** using the location rubric. Promoting inline
   -> `relux/lib/` typically follows the second module needing the
   helper; demoting `relux/lib/` -> inline follows the last cross-module
   caller leaving.
3. **Apply.** Cut the declaration from the source module; paste into
   the destination. Add `from "lib/<name>" import fn_name` to every
   caller module that does not already have it; remove the import from
   any caller module that no longer needs it.

Then run **Verify**, then **Audit**.

### Path: Extract

Use when N (>= 2) occurrences of the same code block should be hoisted
into a function.

1. **Identify the group.** List every occurrence; confirm the variation
   across them is small enough that a parameter list can absorb it. A
   block that varies on shell-name *and* command name *and* expected
   output across three call sites usually needs two parameters; if you
   find yourself reaching for four or five to cover all cases, the
   abstraction is wrong -- either split into two functions or leave it
   inlined.
2. **Pick purity and location** using the rubrics. A block of `send`/
   `match` lines is impure -> `fn`. A block of string composition or
   value derivation is pure -> `pure fn`. If even one occurrence sits
   in a pure-only position (marker expression, top-level `let`), the
   function must be `pure fn`.
3. **Compose the function.** Name it as the verb the group performs
   (`wait_for_prompt`, `parse_pid`, `compose_url`). Parameter names
   match what varies across call sites.
4. **Apply.** Write the declaration at the chosen location; replace
   each occurrence with the function call.

Then run **Verify**, then **Audit**.

### Shared: Verify

```bash
relux check
```

`relux check` catches:

- Parse errors (snake_case violations, malformed signature, `pure fn`
  body using disallowed forms).
- Resolver errors (unknown function name at a caller, missing import,
  arity mismatch at a call site).
- Purity errors (a `pure fn` body that uses a shell op, an impure BIF,
  or calls a `fn`).
- Marker propagation errors (if a function carries a marker, every
  caller inherits it; the resolver flags positions that cannot satisfy
  the inherited marker).

For impure functions, run an exercising test to confirm the body does
the intended work:

```bash
relux run path/to/test_using_fn.relux
```

The function appears in the structured log as its own span; the
`event.html` viewer renders the span entry/exit and every contained
send / match / BIF call.

### Shared: Audit

After every operation, re-walk the chain.

- For **Add** / **Extract**: confirm no clashing definition with the
  same `(name, arity)` is in scope at any caller. Same name with
  different arity is fine; same `(name, arity)` twice in the same scope
  is a resolver error.
- For **Modify** (arity change): confirm every call site uses an arity
  that exists. The dispatch is silent at parse but fails at resolve.
- For **Move**: confirm every caller's `import` resolves and the
  function still satisfies whatever position it is called from
  (pure-only positions still get a `pure fn`).
- For **Remove**: confirm `relux check` reports no dangling references.
- **External-tool guard.** If the function is impure and its body
  invokes a non-standard external tool (`docker`, `kubectl`, `jq`,
  `psql`, a project-specific CLI -- anything outside the POSIX
  baseline you can assume on every target host), confirm the function
  carries a corresponding marker (`# skip unless which("docker")` or
  similar). Function-level markers auto-propagate to callers at
  resolve time, so the marker on the function is what spares every
  test from re-asserting the gate. If the marker is missing, hand off
  to `relux:markers` (Add path against this `fn`) before considering
  the operation done. See `references/functions.md` > *See also* on
  marker-guarding tool-dependent helpers.
- If the function carries a marker, propagation reaches every caller
  transitively. If purity changed (`fn` -> `pure fn`), the propagation
  set grew -- the marker now also reaches `let`-RHS and
  marker-expression callers it did not reach before. Re-check those.

## Done when

- The single intended operation is applied; no unrelated function in
  the module was touched.
- `relux check` passes against the updated file(s).
- For impure functions, `relux run` against a covering test confirms
  the body executes as intended.
- All callers use a valid `(name, arity)` against the updated
  declarations; imports resolve.
- The function lives at the right location for its caller set (inline
  if one module, `relux/lib/` if shared).

## Cross-skill handoffs

- `relux:markers` -- when the function being authored or modified needs
  a `# skip` / `# flaky` / `# run if` guard. This skill writes the
  declaration; `relux:markers` places the marker.
- `relux:effect-write` -- the natural caller when authoring an effect
  whose shell body needs helper functions; that skill hands off here.
- `relux:test-write` -- the natural caller when authoring a test that
  needs a helper or a project-local anchor function for a non-POSIX
  shell; that skill hands off here.
- Future `lib-organize` (not yet drafted) -- the natural caller when
  reorganizing the `relux/lib/` directory.

## References

- `references/functions.md` -- definition syntax, pure vs impure
  semantics, frame-scoped vs shell-side state, arity-based dispatch,
  worked-out pitfalls (leave-shell-clean, defaults-via-arity,
  no-shared-shell-state).
- `references/bifs.md` -- pure vs impure built-ins; a `pure fn` body
  may only call pure BIFs and other `pure fn`s.
- `references/imports.md` -- import resolution from project root; how
  `from "lib/..." import ...` interacts with the `relux/lib/`
  convention.
- `references/project-layout.md` -- the `relux/lib/` directory
  convention and discovery rules.
- `references/markers.md` -- function-level marker propagation,
  including how purity changes the propagation set.

## Pitfalls

The recurring function-language mistakes -- preferring `pure fn`
for value derivation, default arguments via arity dispatch, leave
the caller's shell clean, no implicit caller-state coupling --
live in `references/functions.md` with canonical Don't/Do
examples. The pre-flight read loads them; this section captures
the skill-level discipline that has no reference home.

### Don't extract one-off code

Three identical lines that appear in exactly one test are not a
function. Extract when (a) the block appears in two or more places,
(b) the variation across occurrences is parameterizable in two or
three arguments, and (c) the function name reads as a meaningful
summary of the block at the call site. Premature extraction trades
local readability for a name the caller now has to remember --
expensive when the call site never multiplies.

### Don't promote inline -> `lib/` for a single caller

`relux/lib/` reads as "shared." Moving a helper there with one test
calling it pays a maintenance cost (extra import line, mental scope
split, harder to grep) for a hypothetical second caller that has not
arrived. Promote on the second caller, not before. The same logic
applies in reverse: demote `lib/` -> inline only when the last
cross-module caller is gone.

### Place a shared marker on each `fn` directly

For N functions sharing the same marker condition (e.g., all
requiring `docker`), place the marker on each `fn` directly. **Do
not** consolidate them onto a shared guarded `pure fn` calling
`let _ = mark_<group>()`. Functions auto-propagate markers to
callers at resolve time with zero runtime cost; the consolidate
pattern's guard call adds per-invocation cost on the hot path.
The consolidate pattern is for tests and effects, not function
groups -- see `relux:markers` > *Consolidate*, which explicitly
excludes function groups from that path.
