---
name: relux:reference
description: Answer the user's question about how Relux works -- syntax, semantics, runtime behavior, lifecycle, identity, scoping, matching, BIFs, markers, project layout, the events.json schema, CI integration -- by routing to the plugin's `references/` library. Use when the user asks "how does X work in Relux", "what's the syntax for Y", "what does Z mean in Relux", "is some behaviour possible", or otherwise needs an explanation grounded in the language and runtime rather than an authoring action. Also fires when the agent is about to answer a Relux-semantics question from memory and a reference exists that owns the canonical rule -- read the reference first, do not rely on recall. Primary entry point for `/relux:reference`. Out of scope -- authoring (`relux:test-write`, `relux:effect-write`, `relux:function`, `relux:markers`), editing (`relux:test-edit`, `relux:effect-edit`), bootstrap and tuning (`relux:init`, `relux:configure`), and installation (`relux:install`, `relux:editor-plugin`); those skills own what to do, this one owns what is true.
---

# Answer questions about Relux from the references

The references bundled with this plugin (`../../references/*.md` relative
to this skill file) are the canonical source of truth for the Relux
language and runtime. This skill is the router: pick the references that
match the user's question, read them in full, then answer.

The catalog below is intentionally one line per reference. The goal is
*which file to open*, not what it says -- if the catalog ever drifted, it
would be worse than useless. Domain rules belong in the references; this
skill points at them.

## When to use

User phrasings:

- "How does <X> work in Relux?" / "What does <Y> mean?"
- "What's the syntax for <Z>?" / "Show me the grammar for ..."
- "Why did <behaviour> happen?" / "Is <thing> possible?"
- "What's the difference between `fn` and `pure fn`?" / "When does an effect get deduplicated?"
- "Explain the events.json schema." / "How do markers propagate?"
- The user typed `/relux:reference` directly.

Agent-task signals:

- About to answer a Relux-semantics question from memory rather than
  from a reference. Stop -- read the reference. Recall drifts; the file
  does not.
- A sibling skill (e.g. `relux:test-write`, `relux:effect-edit`) hits a
  semantics question outside its workflow. The sibling skill keeps its
  authoring focus; this one supplies the rule.

**Out of scope.** This is an *explanatory* skill, not an authoring one.
If the user wants to *do* something -- write a test, add an effect, edit
markers, scaffold a suite, install the binary -- defer to the
appropriate sibling skill. The catalog here is a reading list, not an
editing surface.

## Workflow

### 1. Classify the question

What category does the question fall into? Use the catalog headings
below as the rubric:

- **Project & layout** -- manifest, directory convention, imports.
- **Block structure & syntax** -- top-level forms, statements,
  interpolation.
- **Matching & I/O** -- `<?` / `<=`, multimatch, fail patterns,
  timeouts.
- **Functions, effects, cleanup** -- `fn` / `pure fn`, effect identity
  and expose, cleanup discipline, BIFs.
- **Markers** -- `# skip` / `# run` / `# flaky` forms and propagation.
- **Running, CI, artifacts** -- `relux` CLI, CI integration, the
  events.json schema and operational recipes, the test-log viewer.

A question often spans two categories (e.g. "how does a `start Db` in a
test affect cleanup order?" touches effect identity *and* cleanup). Pick
all of them; reading two short references costs less than guessing.

### 2. Pick the references

Open the catalog in the section below. For each candidate reference,
ask: *if this file did not exist, could I answer the question correctly
from the others alone?* If no, read it.

Bias toward reading more rather than fewer. References are short (most
under 200 lines) and the cost of a wrong answer grounded in stale
recall is much higher than the cost of one extra read.

### 3. Read the references

Use the Read tool, not Bash `cat`. Read the whole file -- references
are short and the section the user needs is not always announced by an
obvious heading.

When a reference points at another reference (Markdown link syntax in
the body), follow the link if it bears on the question.

### 4. Answer the user

- Ground every claim in something you just read. If you cannot point
  at the sentence or section that backs a claim, say so explicitly.
- Cite the reference(s) in the answer, e.g. *"see `effects-identity.md`
  > Identity tuple"*. The user can then go deeper without re-asking.
- Quote sparingly. Paraphrasing what you read is usually the right
  size; copy verbatim only when wording matters (operator forms,
  invariant statements).
- If the references *do not* cover the question, say so. The next
  surface is the upstream Relux documentation at
  <https://shizzard.github.io/relux/latest/> (reference / DSL tutorial /
  suite tutorial mdbooks). Do not invent semantics.

### 5. Offer a follow-up handoff

If the question turned out to be a precursor to an action, point at
the sibling skill that handles it -- *"now that you know how `expose`
works, `relux:effect-write` is the authoring entry"*. Do not invoke the
sibling skill unprompted; the user asked a question, not for work.

## Reference catalog

Reference files live alongside this skill in the plugin. Read them via
`../../references/<file>.md` relative to this skill file.

### Project & layout

- `project-layout.md` -- `Relux.toml` fields, the
  `relux/{tests,lib,out}` directory convention, file discovery, the
  nested-manifest pitfall.
- `imports.md` -- `import <module-path>` forms (wildcard /
  selective / aliased), module path resolution from the suite root, the
  CamelCase-vs-snake_case disambiguation rule.

### Block structure & statements

- `block-structure.md` -- top-level forms (`test`, `effect`,
  `fn`, `pure fn`, `import`), body section order, where `let` / `start`
  / `shell` / `cleanup` belong, multi-`shell`-same-name re-entry.
- `statements.md` -- statements inside `shell` / `fn` /
  `cleanup` bodies (send, match, sleep, let, captures, control codes).
- `interpolation.md` -- `${...}` substitution semantics,
  bare vs interpolated forms, evaluation order.
- `fail-patterns.md` -- background `fail` guards on a
  shell's output, operator forms, scope.
- `timeouts.md` -- match-operation deadlines, the two kinds
  (per-match `~Ns` / per-statement `@Ns`), CI multiplier interaction.

### Matching

- `matching.md` -- the `<?` and `<=` operators, cursor
  semantics, anchoring, captures.
- `multimatch.md` -- the `<{ ... }` block for any-order
  matching, cursor behavior.

### Functions, effects, cleanup

- `functions.md` -- `fn` (runs in caller's shell context)
  vs `pure fn` (no shell context, compile-time evaluable in `let`),
  shadowing rules, where each may live.
- `effects-identity.md` -- effect body section order, the
  `(name, expects)` identity tuple, lifecycle (instantiate ->
  cleanup), dedup semantics, per-test scope.
- `effects-expose.md` -- `expose shell <name>` and
  `expose let <name>`, what callers can reach, expose-not-re-export
  rules for wrapper effects.
- `cleanup.md` -- `cleanup` blocks on tests and effects,
  the fresh implicit shell, uncancellable tokens, what statements are
  legal.
- `bifs.md` -- built-in functions split by purity (pure
  BIFs: `trim`, `upper`, `lower`, `replace`, `split`, `len`, `uuid`,
  `rand`, `available_port`, `which`, `default`; impure BIFs:
  `sleep`, `annotate`, `log`, `match_prompt`, `match_exit_code`,
  `match_ok`, `match_not_ok`, control codes).

### Markers

- `markers.md` -- `# skip` / `# run` / `# flaky` forms,
  polarity (`if` / `unless`), expression shapes, propagation rules
  (skip propagates transitively), layer choice (test / effect / fn).

### Running, CI, artifacts

- `cli-reference.md` -- `relux` subcommands (`init`, `new`,
  `check`, `run`, `history`) and the flags an agent needs.
- `ci-integration.md` -- output formats (JUnit, TAP),
  scaling for slower environments via `--timeout-multiplier`, what to
  archive.
- `events-schema.md` -- the canonical `events.json` shape,
  top-level fields, event / span / buffer-event records.
- `events-failures.md` -- the `outcome` enum for
  non-passing tests (Pass / Fail / Cancelled / Skipped / Invalid) and
  the records they carry.
- `events-recipes.md` -- `jq` recipes for reconstructing
  context from `events.json` (buffer state at a seq, variable scope,
  call stack, dedup audit). Verify field names against
  `events-schema.md` before scripting.
- `viewer.md` -- `event.html` is a self-contained Svelte
  SPA. **Human surface only** -- the agent does not read it. Hand the
  user a link when visual triage helps; use `events-recipes.md`
  against `events.json` for any programmatic reconstruction.

## Cross-skill handoffs

- `relux:init` -- *"how do I set up Relux in a new project?"*
- `relux:install` / `relux:editor-plugin` -- *"how do I install
  Relux / get syntax highlighting?"*
- `relux:configure` -- *"how do I change the shell / a timeout / the
  job count?"*
- `relux:test-write` / `relux:test-edit` -- *"write/change a test
  that ..."*
- `relux:effect-write` / `relux:effect-edit` -- *"write/change an
  effect for ..."*
- `relux:function` -- *"extract this into a helper / make a `pure
  fn`."*
- `relux:markers` -- *"skip this when ... / mark this as flaky."*

If the user's question is a precursor to one of these actions, answer
the question first, then point at the sibling skill. Do not invoke it
silently.

## Pitfalls

### Don't answer from memory

Recall drifts; the references are the canonical rule. Even when the
answer feels obvious, open the file. The marginal cost of a Read is
near zero; the cost of confidently asserting a stale rule (an operator
that was renamed, a field that moved, a propagation rule that flipped)
is large.

### Don't restate reference content into the answer wholesale

The user asked a question, not for a full transcript. Read the
reference, then answer the specific question with a citation. If the
user wants the whole picture, the citation is enough -- they can open
the file.

### Don't invent semantics when the references are silent

If no reference covers the question, say so plainly. The next surface
is the upstream Relux documentation at
<https://shizzard.github.io/relux/latest/> -- reference, DSL tutorial,
suite tutorial. Speculation that sounds plausible but is unbacked is
the worst possible answer.

### Don't read the viewer to answer programmatic questions

`viewer.md` is explicit: `event.html` is a human surface.
For any question that boils down to *what's in the structured log*,
read `events-schema.md` + `events-recipes.md` and query `events.json`
directly.

### Don't substitute for an authoring skill

If the user says *"how do I write a test that ..."* with a concrete
target, answer the semantics question if there is one, then hand off
to `relux:test-write`. This skill does not author `.relux` files.
