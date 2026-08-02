# R013: Pure String Match

- **Status**: implemented
- **Created**: 2026-05-21
- **Revised**: 2026-07-28
- **Supersedes**: the original R013 draft (the `|?` / `|=` pipe-operator design); R009 (Variable Match Operator)

## Abstract

Give Relux a way to match a string-valued expression against a literal or regex
pattern without routing it through a shell. Rather than introduce a new operator
family, this revision reuses the kind glyphs the language already has - `=`
(literal) and `?` (regex) - as infix operators on a value, and
relocates variable binding from `=` to `:=` to make room for them. The result is
one match *kind* rule - `=` literal, `?` regex - across shell matches, fail
patterns, marker conditions, and pure matches, with no new match glyph and no
change to marker syntax or semantics. The literal comparison follows the source:
a shell buffer is an unbounded stream, scanned for a substring (contains); a
value is a complete, bounded string, compared for exact equality. A
pure-match statement is an assertion: a non-match fails the test immediately,
exactly like a shell `<?` that never matches - Relux has no error handling, so
there is nothing to propagate. A pure-match used as a marker condition is a
truthiness test that never fails.

## Motivation

### Asserting and extracting from string values

A test that holds a string value - the return of a function, a `let` binding, an
interpolation - has no in-language way to assert its shape or pull out a
substring. The current workaround echoes the value through a shell and matches
the echoed output:

```relux
let value = compose_payload("alice")
> echo "${value}"
<? ^id=(\d+);user=(\w+)$
let id = $1
let user = $2
```

This is dishonest. The match has no business going through a PTY - it is a pure
operation on a known string. The shell round-trip pollutes the event log with a
send plus a match plus the prompt match that follows, depends on the prompt
matching cleanly afterwards to leave the buffer usable, and risks the
echoed-command trap (matching the echo of the `echo` rather than its output).
The information is already in scope; we should match on it directly.

### Marker conditions already do pure matching

Marker conditions today accept `X ? regex` and `X = literal`:

```relux
# skip unless TARGET_OS = linux
# skip unless ARCH ? ^(x86_64|aarch64)$
```

That evaluation runs through the same pure expression evaluator that pure-fn
calls use. But the mechanic stops at the marker boundary: the same expressive
power is unavailable inside test bodies, function bodies, or pure-fn bodies.
There is no reason for the asymmetry beyond "we never got around to it."

### Why the original pipe design was wrong

The first R013 draft proposed two new operators, `|?` and `|=` ("pipe the value
into the matcher"), and migrated markers onto them - removing bare `?` / `=` and
shifting `=` from exact-equality to substring. Working through the operator
grammar showed the pipe does not earn its place:

Relux operators compose from atomic glyphs - `<` / `>` name a source (read from
/ write to the shell), `~` / `@` name a timer (soft / hard), `=` / `?` name a
kind (literal / regex) - so `<~10s?` is guessable from its parts. In that
grammar, `<` exists to name a source *because a shell match has no left operand*:
the glyph is the only thing that says where the bytes come from. A pure match is
different - it always has an explicit left operand, and that operand already
names the source. The pipe encodes nothing the left-hand side did not already
encode; it exists only to dodge a collision. In a grammar where every glyph is
meant to carry meaning, that is the signal it does not belong.

The ideal pure-match operator is therefore just the bare kind glyph, used
infix - which is exactly the marker syntax that already exists. The only reason
it cannot be written in a body today is that bare `=` is taken by assignment. So
the clean move is not to invent a match operator; it is to relocate assignment to
its correct glyph, `:=`, and let `=` / `?` become the universal match kind-glyphs
everywhere.

## Proposal

### The reimagined operator system

| Concern           | Glyph(s)                          | Notes                                              |
|-------------------|-----------------------------------|----------------------------------------------------|
| write to shell    | `>` , `=>` (raw)                  | unchanged; `=>` is a pre-existing outlier          |
| read + match shell| `<=` `<?` , timed `<~5s=` `<@2s?` | unchanged; `=` / `?` are the kind                  |
| fail-guard shell  | `!=` `!?`                         | unchanged                                          |
| **match a value** | **`expr = lit`** , **`expr ? re`**| new in bodies; `=` exact, `?` regex; identical to marker syntax |
| soft / hard timer | `~Ns` `@Ns`                       | unchanged                                          |
| **bind / reassign**| **`:=`**                         | relocated off `=`                                  |

The kind glyphs `=` (literal) and `?` (regex) name *how* to match; the source -
named by whatever prefixes them - names *what* is matched and, for a literal, how
it is compared. `<` (shell buffer) and `!` (fail-guard on the buffer) name an
unbounded stream, so a literal `=` scans it for a substring (contains). An
explicit left-hand expression - or a marker condition - names a bounded value, so
a literal `=` compares it for exact equality. `?` is a regex against the source in
every position. One kind rule, no new match glyph.

### Relocating assignment to `:=`

Binding moves from `=` to `:=` at every site:

```relux
let url := format_url("localhost", "8080")   # declaration
url := trim(url)                              # reassignment

start Service as s {
    PORT := available_port()                  # overlay value
}
```

`:=` is the correct home for assignment - it means "bind" across Go, Pascal, and
Python's walrus - and vacating `=` is what lets the match kind-glyphs be uniform.
This is the single largest breaking change in the RFC and the reason it is a
major-version bump. See "Migration."

The declaration form is protected by the `let` keyword: `let x = e` becomes a
parse error (the parser expects `:=` after the name), so the common binding site
cannot silently degrade. The residual hazard is bare reassignment: `x = e`
(pure-match) versus `x := e` (reassign) differ by one character. The failure
direction is the safe one - drop the `:` and the intended bind becomes an
assertion that either fails loudly as a test failure or leaves `x` stale and
visible downstream, rather than silently swallowing an assertion the way a
`==`-style operator would.

### Pure-match statements

Two statement forms, structurally the infix mirror of `<=` / `<?`:

| Form            | Behavior                                                                 |
|-----------------|--------------------------------------------------------------------------|
| `<expr> = <pat>`| LHS is compared to `<pat>` for exact equality. Fails if not equal.        |
| `<expr> ? <pat>`| LHS is matched against regex `<pat>`. Fails if no match.                   |

`<pat>` is a string with interpolation, identical to the RHS of `<=` / `<?`.
`<expr>` is any pure expression accepted by `expr()`: bare identifier, quoted
string (with interpolation), function call, qualified variable (`Alias.var`),
capture reference (`$n`), numeric literal.

The literal form is **exact equality**, not contains. A value is a complete,
bounded string, so the natural literal question is 'is it exactly this?' - unlike
the shell buffer, an unbounded stream that `<=` scans for a substring. Substring
and pattern matching on a value go through `?` (an unanchored regex is a
contains):

```relux
value = linux          # exact: matches only the string "linux"
value ? linux          # contains: "linux", "linux-arm", "ubuntu-linux", ...
value ? ^linux$        # exact via regex, equivalent to `= linux`
```

The rule is split by source rather than uniform across it: buffer-sourced
literals (`<=`, `!=`) are contains because a stream is scanned; value-sourced
literals (`=`, and marker `=`) are exact because a value is compared. `?` / `!?` /
`<?` are regex everywhere.

Pure-match is **statement-only**. It is not an expression and does not appear in
let-RHS positions or overlay values. (A cleanup block is a shell block, so a
pure-match statement is valid there - it simply cannot be a cleanup *value*.) To
extract a value from a match, wrap it in a pure fn and return the capture:

```relux
pure fn extract_id(s) {
    s ? ^id=(\d+)$
    $1
}

test "uses extract_id" {
    let payload := build_payload()
    let id := extract_id(payload)
}
```

No negated forms. For markers, negation is expressed by choosing `# run if ...`
versus `# skip unless ...` against a positive match. For body statements, an
"assert does not match" case was rare enough to defer; a follow-up RFC can
revisit. No timed forms (pure matches are instantaneous). No bare-reset form
(there is no buffer cursor to reset).

### Captures

Successful regex matches populate numeric captures `$0..$n`, read via `$n`
(mirrors shell `<?`). A literal `=` match binds no captures. `$n` reads the
ambient capture frame uniformly in every context - pure or shell - and resolves to
`""` when the frame is empty or the group is unset; there is no purity gate on
`$n`. Only a qualified variable reference (`Alias.var`) remains a purity violation.
Which frame a match writes to, and which frame `$n` reads, depends on context:

- **Shell context** (a shell statement or regular `fn` body): the shell's existing
  capture frame, held in `ExecutionContext`.
- **Pure context** (a `pure fn` body): a capture frame carried as a sibling
  `HashMap` parameter alongside `VarScope` through `eval_body` - not a frame stored
  inside `VarScope`.
- **Test / effect preamble**: a single `body_captures` frame is hoisted across the
  whole pre-`start` preamble; each preamble `let` and pure-match reads it, and a
  regex `=`/`?` match overwrites it.
- **`start` overlays**: inherit the enclosing preamble's final `body_captures`
  frame (the last regex match's groups, empty if none), so an overlay value can
  read `$n` from a preamble match.
- **Shells started from a preamble**: do **not** inherit preamble captures; each
  shell owns its own fresh frame.
- **Marker context**: an empty frame - markers are collected and evaluated before
  the preamble runs, so no capture is in scope and `$n` is `""`.

Failed matches never bind captures; failure aborts evaluation before the binding
step.

### Markers: same syntax, same semantics

Marker conditions keep bare `=` / `?`, and their meaning is unchanged: `=` is
exact equality (as it has always been in markers), `?` is regex. A marker
condition names a value - an environment variable, a pure-fn result - so it shares
the pure-match semantics: literal `=` compares for exact equality, matching the
body pure-match form. This alignment costs nothing, because marker `=` already
meant exact, so there is no marker migration.

```relux
# skip unless TARGET ? ^(linux|darwin)$   # regex, unchanged
# run if OS = linux                       # exact, unchanged
# run if PATH ? bin                        # contains, via regex
```

Markers remain truthiness tests: `=` returns the LHS if it equals the RHS, empty
otherwise; `?` returns the match or empty. A non-match is falsy, never a failure.
The bare truthy form (`# skip unless X`) continues to work.

Because a marker condition and a body statement both match against a value, their
literal/regex semantics are identical; the two differ only in failure behavior - a
condition is falsy on no-match, a statement asserts.

### Where the pure-match statement appears

- shell blocks (including cleanup blocks, which are shell blocks);
- regular `fn` bodies;
- `pure fn` bodies (a new statement form alongside `let` and reassignment);
- test bodies - the pre-`start` pure preamble, alongside `let`;
- effect setup bodies - the pre-`start` pure preamble, alongside `let` and
  `expect`.

A preamble accepts `let` and pure-match statements only, never a bare expression.
A bare `expect`ed variable is a legal pure-match LHS (it is a plain in-scope
name); only a qualified `Effect.var` reference stays a purity violation, and that
form cannot appear in a preamble anyway (nothing is exposed before `start`).

A pure-match is a statement, never a value-producing expression, so it never
appears in a test- or effect-level `let` RHS position or as an overlay value.

### Pure matches are assertions; markers are conditions

Relux has no error-handling constructs, and pure matching does not add any. A
pure-match behaves by syntactic position, the same way the match glyphs already
do:

- **As a statement** (a bare match line in a shell block, `fn` body, or `pure fn`
  body) it is an **assertion**. A non-match fails the test immediately - the same
  fail-fast behavior as a shell `<? regex` that never matches. There is nothing to
  catch, handle, or propagate; the test fails at that point. Treat it as a test
  panic.
- **As a marker condition** (the RHS of `# skip unless` / `# run if` / `# flaky
  if`) it is a **truthiness test**. It returns the match, or empty string on
  no-match, exactly as `?` / `=` do in markers today. A non-match is falsy, not a
  failure. Markers decide skip/run; by definition they never fail a test.

So the failure model is the existing one: an assertion that does not hold fails
the test, like any shell match. There is no propagation model and no "caller
decides how to surface it" - every statement-position match that fails, fails the
test. The pure-match assertion carries the same failure context a shell match
does (value, pattern, is_regex, call stack, vars in scope), so the report is as
rich.

Mechanically, an assertion reached deep inside a pure-fn call must unwind back to
the statement and then to the test boundary - exactly how a shell-match failure
already unwinds (the VM returns `Err(Failure)` up to the test runner). To carry a
pure-match failure the same way, pure evaluation gains a fallible return -
`eval_pure_expr` / `eval_pure_fn` / `eval_body` return
`Result<String, PureEvalError>` - where the error is the *unwinding vehicle*, not a
value the DSL can observe or handle:

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum PureEvalError {
    #[error("pure match failed: value did not satisfy pattern")]
    PureMatchFailed {
        value: String,
        pattern: String,
        is_regex: bool,
        span: IrSpan,
    },
    #[error("malformed pattern: {reason}")]
    MalformedPattern {
        pattern: String,
        reason: String,
        span: IrSpan,
    },
}
```

The error carries **no call stack**, and there is no `PureFrame` type. Rather than
thread a stack onto the error, the pure-fn call chain is reconstructed at the
failure boundary from the still-open span tree: the boundary resolves the
innermost open pure-fn span (`sink.deepest_open_span()`) up through its ancestors
(`log.resolve_stack(leaf)`), the same mechanism every other failure in the system
uses. The spans are already open on the error path - the `?` short-circuit skips
the `leave_pure_fn` that would have closed them - so the chain is read from them
rather than duplicated onto the error. The shared boundary helpers are
`resolve_pure_stack` and `pure_eval_failure` in `report/result.rs`.

Two failure modes reach a live evaluation, not one. `PureMatchFailed` is a clean
no-match. `MalformedPattern` is a malformed *interpolated* regex - constant
patterns are validated at lowering, but an interpolated pattern can only be
compiled at evaluation time, so a bad one surfaces here and is translated to
`Failure::Runtime` (never folded into the match-failure rendering). All other
invalid states (undefined identifiers, arity mismatches, cycles) are still caught
at lowering time.

### The one place position matters: the marker decision phase

A pure-match statement can live in a `pure fn` body, and a marker condition can
call a `pure fn`, so a statement-position match can be *reached* during marker
evaluation. Markers cannot fail a test, so in that phase a reached non-match
resolves the condition to false instead of failing - the same falsy outcome the
truthiness form produces directly. This is not error handling; the marker
decision phase is simply, structurally, non-failing.

Concretely (after the hierarchical `.env` refactor, marker decisions run per-test
at resolve time, on the per-test `LayeredEnv`, before any shell starts):

- **`decide_markers`** treats a reached `PureMatchFailed` as `met = false`, kept
  distinct from `LoweringBail::invalid` (a malformed regex still bails to
  `Invalid`; a value that does not match is just "condition not met"). The
  `RecordingSink` accumulates ops by side effect, so the ops trail survives for
  the viewer.

At runtime there is no special case: every context that reaches a failed
pure-match fails the test - shell-block statement, test-level `let`, effect-level
`let`, overlay value, and (M4.5) a test- or effect-preamble pure-match statement.
`eval_effect_let` grows a `Result` signature so an assertion inside an
effect-level `let` can fail the test like any other. There is no lowering-time /
constant-folding pure-eval call site, so no lowering path needs to turn a match
failure into an `InvalidReport`.

### Structured-log recording

The single `EventKind::PureMatch` event used by markers today is replaced by a
trio mirroring shell match's `MatchStart` / `MatchDone` / `Timeout` shape:

- `EventKind::PureMatchStart { value, pattern, is_regex }` - `value` is the
  haystack inline (shell match reads it from the buffer instead).
- `EventKind::PureMatchDone { matched, captures }` - emitted on success.
- `EventKind::PureMatchFailed {}` - emitted on a clean no-match. Empty payload;
  the preceding `PureMatchStart` in the same span carries the context.

Sequences: success is `PureMatchStart` + `PureMatchDone`; a clean no-match is
`PureMatchStart` + `PureMatchFailed`, then the error propagates. The `Start` is
emitted together with its terminal event once the attempt resolves, so the two
always land as a pair. A malformed interpolated pattern (`MalformedPattern`) emits
**neither** - no orphan `PureMatchStart` reaches the log - and fails via
`Failure::Runtime` instead.

The `MatchKind` enum (currently defined in both `relux-ir/pure_sink.rs` and
`observe/structured/span.rs`) is removed; `is_regex: bool` is used everywhere,
consistent with `MatchStart.is_regex`, `FailPatternSet.is_regex`, and
`FailPatternTriggered.is_regex`. `PureEvalSink::record_match(...)` is replaced by
`record_pure_match_start` / `record_pure_match_done` / `record_pure_match_failed`,
and `SinkOp` mirrors the same trio.

### Failure record

`Failure::PureMatch` is a new variant in `report/result.rs`, carrying `value`,
`pattern`, `is_regex`, `span`, `shell`, and a `FailureContext`. `shell` is the
empty string for a preamble/overlay pure match that runs before any shell.
`FailureRecord::PureMatch` is the serialized counterpart in
`observe/structured/failure.rs`; it carries `span`, `event_seq`, `shell`, `value`,
`pattern`, `is_regex`, `call_stack`, and `vars_in_scope`, and has **no**
`buffer_tail` field (pure matches have no buffer). The unified `call_stack` is
reconstructed at the failure boundary from the still-open span tree (see the
failure model above); pure-fn frames appear as
`StackFrame { kind: "pure-fn-call", ... }`, top-down in display order.

Console and HTML renderers branch on the `Failure` discriminant. The `PureMatch`
arm renders a "pure match did not satisfy pattern" header, `value:` and
`pattern:` lines, an `at <span> (pure match)` line, omits the buffer-tail
section, and renders `vars_in_scope` unchanged.

## Migration

Relux is pre-1.0; a single `feat!:` conventional commit carries the change
through release-please to the next major.

### Breaking changes

1. **Assignment relocates from `=` to `:=`.** Every binding site changes:
   `let` declarations, bare reassignments, and overlay values in `start`. This
   touches essentially every `.relux` file. It is the dominant breaking change.

2. **The structured-log event `EventKind::PureMatch` is removed**, replaced by
   the `PureMatchStart` / `PureMatchDone` / `PureMatchFailed` trio.

3. **The `MatchKind` enum is removed**; `is_regex: bool` is used everywhere.

4. **Pure-fn evaluation becomes fallible** - `eval_pure_expr` / `eval_pure_fn`
   return `Result<String, PureEvalError>`. External embedders of `relux-ir` see
   the signature change.

Markers are unaffected at the DSL surface: bare `=` / `?` stay, there is no
operator migration to `|?` / `|=`, and marker `=` keeps its existing exact-equality
meaning - the body pure-match adopts *that* semantics rather than the reverse. The
only marker-facing change is internal: their evaluation now routes through the
shared matcher and emits the `PureMatchStart` / `Done` / `Failed` trio instead of
the single `PureMatch` event (breaking change 2), and drops the `MatchKind` enum
(breaking change 3).

### Migration table

| Before                              | After                                   | Notes                                              |
|-------------------------------------|-----------------------------------------|----------------------------------------------------|
| `let x = e`                         | `let x := e`                            | declaration                                        |
| `x = e`                             | `x := e`                                | reassignment                                       |
| `start E as a { K = e }`            | `start E as a { K := e }`               | overlay value                                      |
| `# skip unless X ? p` (regex)       | unchanged                               | marker `?` unchanged                                |
| `# run if OS = linux` (exact)       | unchanged                               | marker `=` stays exact equality                     |
| `> echo "${v}"` + `<? p` + `$1`     | `pure fn f(s) { s ? p; $1 }` + `let id := f(v)` | pure match, no shell round-trip           |
| exact value check                   | `v = x`                                 | value `=` is exact equality                         |
| contains value check                | `v ? x`                                 | unanchored regex is a contains                      |

## Architecture and change surface

### `relux-lexer`

Add `Token::Colon` (`#[token(":")]`). No new match token is needed - `=` and `?`
already exist. Verify the interpolation literal-merge step (`coalesce_parts`)
still handles `?` and `=` inside pattern bodies as pattern content.

### `relux-parser`

- `operator.rs` / `stmt.rs`: add the `:=` bind operator (composes `Colon + Eq`)
  and the infix pure-match operators - bare `Eq` (literal) and bare `Question`
  (regex).
- `stmt.rs`: reassignment parses `name := expr`; `let` parses `let name := expr`;
  add the pure-match statement form (LHS expr, then `=` / `?`, then a pattern)
  recognized in shell-block, fn-body, pure-fn-body, and (M4.5) test/effect preamble
  contexts. One production parses expr-or-pure-match so `x = p` (match) and
  `x := p` (bind) are disambiguated by the `:`. A bare `==` in literal position is
  rejected with a dedicated hint (`=` is exact-match, `?` is regex), since `==`
  lexes as two `Eq` tokens and would otherwise be a confusing parse error.
- Overlay parsing (`start ... { KEY := expr }`) moves to `:=`.

### `relux-ast`

Add `AstStmt::PureMatchLiteral { lhs, pattern, span }` and
`AstStmt::PureMatchRegex { lhs, pattern, span }` (plus the M4.5 preamble forms
`AstTestItem::PureMatch` and `AstEffectItem::PureMatch`). Binding statements switch
to the `:=` token. No `negate` field. Marker AST (`AstMarkerCondBody`) is
unchanged.

### `relux-ir`

- `IrPureStmt` gains a `PureMatch { lhs, pattern, is_regex, span }` variant (with
  sibling `IrShellStmt::PureMatch`, `IrTestItem::PureMatch`, and
  `IrEffectItem::PureMatch` for the shell, test-preamble, and effect-preamble
  forms). `eval_body` evaluates the LHS via the shared `apply_pure_match`, which
  emits `record_pure_match_start`, runs the match, and on success emits
  `record_pure_match_done` and binds captures, on a no-match emits
  `record_pure_match_failed` and returns `Err(PureEvalError::PureMatchFailed)`.
- `regex_validate.rs` validates constant `?` patterns at lowering, as shell match
  does.
- `eval_pure_expr` / `eval_pure_fn` / `eval_body` become
  `Result<String, PureEvalError>`. The pure-fn chain is not threaded onto the
  error; it is reconstructed at the failure boundary from the still-open span tree.
- `marker.rs` catches `PureEvalError` from the evaluator and treats it as
  condition false, distinct from `LoweringBail::invalid`. Marker AST and glyphs
  are unchanged.
- `pure_sink.rs`: `PureEvalSink` gains the match-start / done / failed methods;
  `SinkOp` gets the trio; `MatchKind` is removed.
- The pure evaluator threads a capture frame as a sibling `HashMap` parameter
  alongside `VarScope` (not a frame inside it), populated on a successful match;
  `$n` reads it uniformly with no purity gate (M4.5 removed the
  `pure_fn_body_depth` gate entirely).

### `relux-runtime`

- `vm/mod.rs`: handle the new shell-statement match variant; success emits
  Start+Done and updates the shell capture frame, failure emits Start+Failed and
  builds `Failure::PureMatch`.
- `lib.rs` (`run_test_body`) and `effect/mod.rs` (`bootstrap_effect`,
  `eval_overlay`, `eval_effect_let`): run the preamble `let` / pure-match loop over
  a hoisted `body_captures` frame, thread that frame into overlay evaluation,
  propagate `PureEvalError`, and translate it to `Failure::PureMatch` (or
  `Failure::Runtime` for a malformed pattern) via the shared `pure_eval_failure`
  helper. **`eval_effect_let` changes from `()` to a `Result` signature.**
- `observe/structured/`: the trio of new event kinds; remove `PureMatch` and
  `MatchKind`; add builder emit methods and `FailureRecord::PureMatch`.
- `report/result.rs`: `Failure::PureMatch`.
- `report/console.rs`, `report/event_html.rs`: branch on the `Failure`
  discriminant for the pure-match rendering.

TS bindings regenerate via `just build-viewer`; the vendored
`vendor/relux-viewer.js.gz` is rebuilt and committed in the same change.

### `viewer/`

New timeline event kinds (`PureMatchStart` / `Done` / `Failed`) with a distinct
row treatment from shell-match rows; a detail panel showing value, pattern,
is_regex, and captures; a failure-detail branch on `FailureRecord::PureMatch`;
colocated Vitest coverage.

### Editors and highlighting

`:=` and the pure-match use of `=` / `?` get entries in
`editors/vscode/syntaxes/relux.tmLanguage.json`,
`editors/intellij/.../ReluxLexer.flex` (and related token/highlighter files), and
the canonical hljs grammar at
`crates/relux-runtime/src/report/highlight-relux.js`.

### Documentation

Assignment relocation touches nearly every tutorial and reference article that
shows a `let` or reassignment. Beyond the mechanical `=` -> `:=` sweep: a new
reference article on pure matching; `03-built-in-functions` (pure fns are now
fallible); `02-syntax` (the `:=` binding form and the pure-match statements);
`00-semantics` and the events.json schema (new event kinds and
`FailureRecord::PureMatch`); a `dsl-tutorial` chapter or extension; a
`suite-tutorial` helper-fn example.

## What changed from the original draft

- **Dropped `|?` / `|=`.** Pure matching uses bare `=` / `?`; no new match glyph.
- **Markers keep their syntax and their `=` meaning.** Bare `?` / `=` stay (no
  `|?` / `|=` migration), and marker `=` remains exact equality. The draft's
  *syntactic* migration is dropped, and no semantic alignment is needed - markers
  already matched values by equality, which is exactly what the body pure-match
  adopts. The literal comparison is split by source instead: buffer-sourced `<=` /
  `!=` are contains, value-sourced `=` (bodies and markers) is exact.
- **Assignment relocates to `:=`.** This is the new enabling change and the
  dominant breaking cost.
- **Reframed failure as fail-fast, not propagation.** A pure-match assertion
  fails the test immediately, like a shell `<?`; Relux has no error handling, so
  the draft's "callers decide how to surface it" model is dropped. The sole
  non-failing context is the marker decision phase. `eval_effect_let` still needs
  a `Result` signature so an assertion inside an effect-level `let` can fail the
  test.
- **Corrected the marker phase.** Marker decisions run at resolve time per-test
  (post `.env` refactor), not at lowering time; there is no lowering-time
  constant-folding call site to guard.

## Open questions

1. **`=>` raw send.** It is the one non-compositional glyph in the operator
   system (`=` + `>` does not read as "literal write"). This RFC leaves it as-is;
   a separate cleanup could revisit it (e.g. `>>` or `>|`).
2. **Reassignment footgun (resolved).** `x = e` (pure match) versus `x := e`
   (bind) differ by one character. Shipped as designed: the `:` disambiguates,
   `let` is keyword-protected, and a dropped `:` fails loudly or leaves a
   stale-but-visible var - the safe failure direction. Accepted; no leading keyword
   for bare reassignment.
3. **Negated pure match.** Deferred. If an "assert does not match" body form is
   wanted, a follow-up RFC can add it without disturbing this design.
