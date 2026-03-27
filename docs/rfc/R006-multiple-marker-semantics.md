# R006: Multiple Marker Semantics

- **Status**: draft
- **Created**: 2026-03-27

## Motivation

A test, effect, or function can carry multiple condition markers. The tutorial currently states that "the exact combination semantics for multiple markers are not yet established." This RFC formalizes the semantics that are already implemented and explains the expressiveness they provide.

## The Rule

**Each marker independently decides "skip" or "don't skip". If any marker decides to skip, the item is skipped.** This is a logical OR over skip decisions.

The implementation iterates through the marker list. Each marker evaluates its condition and determines whether it triggers a skip. The first marker that triggers a skip short-circuits evaluation and the item is skipped. If no marker triggers a skip, the item runs.

## Emergent Properties

This single rule produces two intuitive combination semantics depending on the marker kind.

### `skip` markers combine with OR

```relux
# skip if "${SLOW}"
# skip if "${SKIP_NETWORK}"
test "network test" { ... }
```

Skip if SLOW is set **or** SKIP_NETWORK is set. Any reason to skip is sufficient.

### `run` markers combine with AND

```relux
# run if "${OS}" ? ^(linux|darwin)$
# run if which("jq")
test "needs unix and jq" { ... }
```

Run only if OS matches **and** jq is installed. Every condition must hold.

This AND emerges naturally: `run if X` means "skip if not X". OR over those skip decisions gives skip if ¬A ∨ ¬B, which is equivalent to run if A ∧ B (De Morgan's law).

### Mixing `skip` and `run` is allowed

Because both kinds reduce to the same underlying mechanism — each marker contributes a skip predicate, OR'd together — they compose freely:

```relux
# run if "${OS}" ? ^(linux|darwin)$
# skip unless which("jq")
test "needs unix and jq" { ... }
```

This is equivalent to the pure-`run` version above. Users choose whichever phrasing reads most naturally for each condition.

### `flaky` composes with both

The `flaky` marker is orthogonal to skip/run. It sets the flaky flag independently and does not participate in skip decisions. A flaky marker can coexist with any combination of skip and run markers:

```relux
# flaky
# run if "${CI}"
test "flaky CI test" { ... }
```

## Expressiveness

The system can express:

- **OR of skip conditions**: multiple `# skip` markers
- **AND of run conditions**: multiple `# run` markers
- **Any mix**: each condition phrased as skip or run independently

The system cannot express:

- **AND of skip conditions** ("skip only if both A and B are true")
- **OR of run conditions** ("run if either A or B") across separate markers

These limitations are theoretical. In practice:

- OR over values is handled by regex within a single marker: `# run if "${OS}" ? ^(linux|darwin)$`
- OR over function calls (e.g., `which("curl") or which("wget")`) is not meaningful in a deterministic DSL — if you cannot branch on which tool was found, you must commit to one tool, and the marker should guard on that specific tool
- AND-of-skip ("skip only when two conditions are both true") has no known practical use case

## Changes Required

### Documentation

- Update the tutorial (chapter 15, "Multiple markers" section) to remove the "not yet established" caveat and document the formalized semantics
- Update `docs/semantics.md` to expand the one-line summary with the emergent OR/AND properties
- Remove this item from "Planned RFCs" in `README.md`

### Implementation

None. The current `eval_marker` implementation already implements these semantics correctly.
