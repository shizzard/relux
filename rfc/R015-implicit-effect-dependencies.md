# R015: Implicit Effect Dependencies

- **Status**: implemented
- **Created**: 2026-08-02

## Abstract

Let a `start`'s overlay reference a sibling `start`'s exposed variable by alias:
`start Api { DB_PORT := Db.port }`. The reference induces an *implicit
dependency* - `Api` is instantiated only after `Db` is ready, and reads `Db`'s
exposed value. Independent siblings still start in parallel; only the
data-dependent edge is serialized.

The reference form (`Alias.var`) already parses and already resolves at runtime
as a flat dotted-key lookup. What blocks it today is a compile-time rule that
classifies every qualified variable as impure. This RFC replaces that with a
truthful model: a qualified variable is a pure lookup whose legality is a
*scoping* question, decided when the start DAG is enriched. The rule applies
uniformly to test-level and effect-body start-lists, allows forward references,
detects reference cycles by reusing the existing `CycleReport`, and unifies pure
and shell-body interpolation under one scoping rule - which tightens shell-body
qualified references to compile-time validation (a breaking change).

## Motivation

### Values that belong to one effect are plumbed through the test

An effect frequently produces a value another effect needs: a port, a socket
path, a generated password, a container id. Today the only way to route that
value is to hoist it to the enclosing start-list and feed both effects:

```relux
test "connects to the db" {
  let port := available_port()
  start Database as Db {
    PORT := port
  }
  start Api {
    DB_PORT := port
  }
}
```

`port` conceptually belongs to `Database`. It lives at test level only because
`Api` needs it. The pattern has three costs:

- **The value is decided in the wrong place.** If `Database` picks its own port
  internally (retry-on-collision, container-assigned), the test-level `let`
  cannot express it at all - the real value is not known until `Database` has
  started.
- **The dependency is invisible.** `Database` and `Api` start concurrently; that
  they are ordered by a shared value is implicit in the duplicated `port`
  binding, not stated.
- **Boilerplate scales with fan-out.** Every consumer restates the same overlay
  key against the same hoisted `let`.

What the author wants to write is the direct thing:

```relux
test "connects to the db" {
  start Database as Db
  start Api {
    DB_PORT := Db.port
  }
}
```

`Database` exposes `port`; `Api` reads it; `Api` waits for `Database`. The
ordering is stated by the reference itself.

### The mechanism already exists; only a mislabel blocks it

`Db.port` already parses as `AstExpr::QualifiedVar` in overlay-value position, and
qualified variables already resolve at runtime as flat `qualifier.name` keys in
the variable scope (`crates/relux-ir/src/evaluator.rs:163-168`). Effect bodies
already inject a dependency's exposed variables under exactly these dotted keys
(`crates/relux-runtime/src/effect/mod.rs:596-605`). The scheme is sound because a
dot is impossible in a plain variable name, so plain and qualified names never
collide.

The one thing standing in the way is `IrPureExpr` / `IrInterpolation::lower_pure`
rejecting every `QualifiedVar` as a `purity_violation`
(`crates/relux-ir/src/expr.rs:220`, `crates/relux-ir/src/interpolation.rs:34-45`).
That rejection conflates two independent properties, addressed next.

## Proposal

### Syntax

No new tokens. An overlay value may be a qualified reference to a sibling alias:

```relux
start Database as Db
start Api {
  DB_PORT := Db.port        # Api depends on Db; reads Db's exposed var `port`
}
```

- `Db` must be a sibling `start`'s alias in the *same* start-list (declared via
  `start <Effect> as <Alias>`). A `start` without an alias cannot be referenced.
- Aliases are CamelCase, following the same naming rule as effect names; the
  qualifier before the dot is always an effect alias
  (`crates/relux-parser/src/effect.rs:187`). The referenced variable after the
  dot follows the exposed-variable naming rule.
- `Db` must `expose` `port` as a variable. Referencing a non-exposed or internal
  binding is a compile error.
- Only variables are referenceable. Overlay values are strings; exposed shells
  are not string values and are out of scope for this form.
- The bare-key shorthand (`DB_PORT`, meaning `DB_PORT := DB_PORT`) is unchanged.

### The scoping rule

A qualified variable is a **pure** dotted-key lookup - no side effects, no shell
interaction. Its legality is a **scoping** question:

> A qualified reference `A.x` is legal exactly where `A`'s bindings are in scope.

The "impure" label is retired. "Pure" reverts to its real meaning - free of
impure BIFs. Whether a qualified reference resolves is decided by *what is in
scope*, and the in-scope set depends on lexical position:

| Context | In-scope qualifiers |
|---|---|
| A `start`'s overlay | The sibling aliases of the enclosing start-list |
| An effect's shell body | The effect's own dependency aliases (all of them) |
| Module `let`, preamble `let`, pure-match pattern | None |

The last row preserves the old ban precisely where it was correct: those
contexts have no alias in scope and can never resolve a qualifier, so a qualified
reference there is still rejected - now with an accurate diagnostic
(`unknown qualifier A`) instead of `not pure`. `lower_pure` keeps rejecting
`QualifiedVar` unconditionally for those genuinely-unresolvable contexts; it is a
true invariant, not an ordering artifact.

### DAG enrichment: a separate pass

The dependency graph is built in a dedicated step *after* a start-list is fully
collected, so declaration order carries no meaning and forward references are
legal. For each test and each effect body, `enrich_start_dag`:

1. Builds the full `alias -> {exposed vars}` map for the start-list. The effect
   side already builds this map for expose validation
   (`crates/relux-ir/src/effect.rs:562-586`); the test side gains the symmetric
   map (`crates/relux-ir/src/test_def.rs` collects `starts` but not the map
   today).
2. Walks every start's overlay. For each `QualifiedVar A.x`, validates that `A`
   is a sibling alias in the list and that `A` exposes `x`; records the edge
   `this_start -> A`.
3. Walks every shell-body interpolation and validates its `QualifiedVar`s against
   the effect's own dependency map (see *Shell-body interpolation*).
4. Cycle-checks the recorded edges (see *Cycle detection*).
5. Stores the dependency edges on the start-list IR for the runtime scheduler.

Because validation moves into this pass, the `ShallowLayeredEnv` does **not**
need to carry dotted names, and overlay/shell-body lowering does not thread a
qualifier scope. Lexical lowering captures `QualifiedVar` as data; the enrichment
pass owns all qualified-reference legality.

Expect-satisfiability at the start site is unaffected: `DB_PORT := Db.port` still
presents the overlay *key* `DB_PORT`, so the existing check
(`crates/relux-ir/src/effect.rs:365-380`) continues to pass. The new obligation
(`Db` exposes `port`) is the enrichment pass's responsibility.

### Cycle detection

Forward references make a reference cycle representable
(`A { X := B.out }` and `B { Y := A.out }` in one list). This is a distinct graph
from the existing effect-definition cycle detector, which runs *during* recursive
`resolve_effect` as a termination guard over the definition graph
(`crates/relux-ir/src/lowering_context.rs:592-602`, `818`) and structurally
cannot observe sibling-instance edges. That detector stays exactly where it is.

The enrichment pass runs its own topological check over the sibling-edge set - a
few lines over the edge map it already builds - and on a cycle emits the **same**
`CycleReport` diagnostic type, so the error renders identically to a definition
cycle. The reporting is reused; the detection site is not.

### Runtime scheduling

Today `EffectManager::instantiate` evaluates every overlay serially up front
(phase 1), then acquires all siblings concurrently (phase 2)
(`crates/relux-runtime/src/effect/mod.rs:104-199`). An implicit dependency breaks
that split: a dependent's overlay cannot be evaluated until its referenced
siblings are `Ready` and have exposed their variables.

The batch becomes a per-start schedule driven by the enrichment edges. Each start
awaits the completion of the starts it depends on; once they are `Ready`, their
exposed variables are injected into the caller scope as flat `A.x` keys - the
same mechanism effect bodies already use - and only then is the dependent's
overlay evaluated, its identity key derived, and the effect acquired. The
topological order emerges from the awaits; the enrichment pass has already
guaranteed acyclicity, so no runtime sort is needed. Independent starts run
concurrently, as they do today.

Rollback generalizes along the edges: a failed acquire fails the starts that
depend on it (they can never evaluate), and every successful acquire in the batch
is released, preserving the current all-or-nothing guarantee. Because
`bootstrap_effect` recurses into `instantiate` for effect-body starts
(`crates/relux-runtime/src/effect/mod.rs:477-485`), effect-body start-lists get
the same scheduling with no additional code.

### Effect identity

No special handling. A dependent's dedup identity
(`EffectInstanceKey::from_expects`) includes its overlay values, one of which is
now the referenced sibling's exposed value. Since the sibling is `Ready` before
the dependent's key is derived, that value is known at keying time. Two `Api`
instances wired to different `Database` ports therefore get distinct identities,
which is correct: they are different instances.

### Shell-body interpolation

Unifying qualified-reference handling under one scoping rule folds in the
shell-body path. Today shell-body interpolation uses the permissive
`IrInterpolation::lower`, which accepts any `QualifiedVar` with **no validation**:
`${Typo.host}`, or `${Db.internal}` naming a variable `Db` does not expose, lowers
clean and resolves to `""` at runtime. Under the scoping rule the enrichment pass
validates shell-body qualified references against the effect's dependency map, so
these become compile errors.

This is a correctness improvement and a **breaking change** (see *Migration*).

### Examples

Direct value hand-off:

```relux
test "api talks to db" {
  start Database as Db
  start Api {
    DB_PORT := Db.port
  }
}
```

Forward reference (declaration order does not constrain dependencies):

```relux
test "forward reference" {
  start Api {
    DB_PORT := Db.port      # Db is declared below
  }
  start Database as Db
}
```

Fan-out from one producer, consumers still parallel with each other:

```relux
test "two consumers" {
  start Database as Db
  start Api    { DB_PORT := Db.port }   # both wait for Db,
  start Worker { DB_PORT := Db.port }   # but run in parallel with each other
}
```

Inside an effect body, uniform behavior:

```relux
effect SeededApi {
  start Database as Db
  start Api as Api {
    DB_PORT := Db.port
  }
  expose shell Api.http as http
}
```

Rejected - reference cycle:

```relux
test "cycle" {
  start Producer as A { X := B.out }   # A -> B
  start Consumer as B { Y := A.out }   # B -> A   (compile error: CycleReport)
}
```

## Migration

### Breaking changes

- **Shell-body qualified references are now validated at compile time.** A
  `${Alias.var}` in a shell body where `Alias` is not a dependency of the effect,
  or where the dependency does not `expose var`, was previously accepted and
  resolved to the empty string; it is now a compile error. Suites relying on the
  silent-empty behavior must remove or correct the reference. The error names the
  offending qualifier and variable.

The overlay feature itself is purely additive. The existing test-level `let`
hand-off pattern continues to compile and run unchanged; authors may adopt
`Alias.var` incrementally. Documentation should present the alias form as the
preferred idiom for effect-produced values while keeping the `let` pattern valid.

## Architecture and change surface

### `relux-lexer`

No change. `Alias.var` already tokenizes.

### `relux-parser`

No change expected. `Db.port` already parses to `AstExpr::QualifiedVar` in
overlay-value position via the shared `expr()` parser.

### `relux-ast`

No change. `AstExpr::QualifiedVar` / `AstStringPart::QualifiedVarRef` already
exist.

### `relux-ir`

- Overlay-value lowering captures `QualifiedVar` as data instead of rejecting it;
  legality is deferred to the enrichment pass. `lower_pure` retains its
  unconditional `QualifiedVar` rejection for module/preamble/pure-match contexts.
- New `enrich_start_dag` pass, invoked from test lowering and `IrEffect::lower`
  once `starts` are collected. Test lowering gains the symmetric
  `alias -> {exposed vars}` map.
- Dependency edges stored on the test / effect start-list IR.
- Sibling reference-cycle check reusing `CycleReport` / `EffectCycleEntry`.
- New `InvalidReport` variants for `unknown qualifier`, `not a sibling alias`,
  and `variable not exposed by <alias>`; the shell-body path reuses them.

### `relux-runtime`

- `EffectManager::instantiate` restructured from serial-then-parallel phases to
  per-start scheduling keyed on the enrichment edges: await dependency starts,
  inject their exposed variables as flat keys, then evaluate the overlay, derive
  identity, and acquire. Independent starts stay concurrent.
- Rollback propagates failures along edges while retaining all-or-nothing
  release.
- No change to `bootstrap_effect`'s dependency handling beyond inheriting the new
  scheduler through its recursive `instantiate` call.

### `viewer/` and structured log

The `EffectSetup` span already carries the evaluated overlay. Optionally record
the implicit-dependency edge (which overlay value was sourced from which sibling
alias) so the viewer can show provenance and the setup ordering. Open question
below.

### Editors and highlighting

Highlight `Alias.var` in overlay-value position (currently emphasized only inside
`${...}`) in `editors/vscode/syntaxes/relux.tmLanguage.json`,
`editors/intellij/.../ReluxLexer.flex`, and the canonical hljs grammar
`crates/relux-runtime/src/report/highlight-relux.js`.

### Documentation

Update `docs/reference/` (effects semantics, overlays, effect identity, syntax)
and the effects material in `docs/suite-tutorial/`. Present the alias form as the
preferred idiom for effect-produced values.

## Open questions

- **Structured-log provenance.** Should the log explicitly record each
  implicit-dependency edge (and that an overlay value was sourced from a sibling),
  or is the existing overlay-on-`EffectSetup` recording plus setup ordering
  enough for the viewer?
- **Sequencing.** Land the overlay-dependency feature and the shell-body
  tightening as one atomic change, or ship overlay dependencies first and follow
  with the shell-body validation? Recommendation: one change - the tightening
  falls directly out of the unified scoping rule, and splitting it would leave two
  inconsistent qualified-reference regimes in the interim.
- **Diagnostic for the legacy pattern.** Should the resolver hint, on a
  test-level `let` fed identically into multiple sibling overlays, that the alias
  form may express the intent more directly - or is that too opinionated for a
  lint?
