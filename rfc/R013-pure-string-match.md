# R013: Pure String Match

- **Status**: accepted
- **Created**: 2026-05-21
- **Revised**: 2026-07-28
- **Supersedes**: the original R013 draft (the `|?` / `|=` pipe-operator design); R009 (Variable Match Operator)

## Abstract

Give Relux a way to match a string-valued expression against a literal or regex
pattern without routing it through a shell. Rather than introduce a new operator
family, this revision reuses the kind glyphs the language already has - `=`
(literal / contains) and `?` (regex) - as infix operators on a value, and
relocates variable binding from `=` to `:=` to make room for them. The result is
one uniform match rule across shell matches, fail patterns, marker conditions,
and value matches, with no new match glyph and no change to marker syntax. A
value-match statement is an assertion: a non-match fails the test immediately,
exactly like a shell `<?` that never matches - Relux has no error handling, so
there is nothing to propagate. A value-match used as a marker condition is a
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

### Marker conditions already do value matching

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
the glyph is the only thing that says where the bytes come from. A value match is
different - it always has an explicit left operand, and that operand already
names the source. The pipe encodes nothing the left-hand side did not already
encode; it exists only to dodge a collision. In a grammar where every glyph is
meant to carry meaning, that is the signal it does not belong.

The ideal value-match operator is therefore just the bare kind glyph, used
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
| **match a value** | **`expr = lit`** , **`expr ? re`**| new in bodies; identical to marker syntax          |
| soft / hard timer | `~Ns` `@Ns`                       | unchanged                                          |
| **bind / reassign**| **`:=`**                         | relocated off `=`                                  |

The kind glyphs `=` (literal / contains) and `?` (regex) now mean the same thing
in every position. The source is named by whatever prefixes them: `<` for the
shell buffer, `!` for a fail-guard on the buffer, or an explicit left-hand
expression for a value. One rule, no new match glyph.

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
(value-match) versus `x := e` (reassign) differ by one character. The failure
direction is the safe one - drop the `:` and the intended bind becomes an
assertion that either fails loudly as a test failure or leaves `x` stale and
visible downstream, rather than silently swallowing an assertion the way a
`==`-style operator would.

### Value-match statements

Two statement forms, structurally the infix mirror of `<=` / `<?`:

| Form            | Behavior                                                                 |
|-----------------|--------------------------------------------------------------------------|
| `<expr> = <pat>`| LHS is searched for substring `<pat>` (contains). Fails if not present.   |
| `<expr> ? <pat>`| LHS is matched against regex `<pat>`. Fails if no match.                   |

`<pat>` is a string with interpolation, identical to the RHS of `<=` / `<?`.
`<expr>` is any pure expression accepted by `expr()`: bare identifier, quoted
string (with interpolation), function call, qualified variable (`Alias.var`),
capture reference (`$n`), numeric literal.

The literal form is **contains**, not exact-equality - consistent with `<=`,
which is a substring match against the buffer. Exact matching is expressed the
same way it is for the buffer: anchor a regex.

```relux
value ? ^linux$        # exact
value = linux          # contains ("linux", "linux-arm", "ubuntu-linux", ...)
```

This gives the language one uniform rule: `=` / `!=` / `<=` are contains; `?` /
`!?` / `<?` are regex; anchor a regex for exact.

Value-match is **statement-only**. It is not an expression and does not appear in
let-RHS positions, overlay values, or cleanup blocks. To extract a value from a
match, wrap it in a pure fn and return the capture:

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
revisit. No timed forms (value matches are instantaneous). No bare-reset form
(there is no buffer cursor to reset).

### Captures

Successful regex matches populate numeric captures `$0..$n`, read via `$n`
(mirrors shell `<?`). Captures live in a per-block capture frame:

- **Shell context** (a shell statement or regular `fn` body): captures live in
  the shell's existing `Captures` frame.
- **Pure context** (a `pure fn` body): the pure evaluator gains a parallel
  `Captures` frame so `$n` resolves there.
- **Marker context**: captures are ignored - markers are evaluated for boolean
  truth only.

Failed matches never bind captures; failure aborts evaluation before the binding
step.

### Markers: same syntax, aligned semantics

Marker conditions keep bare `=` / `?` - the syntax does not change. Their
*meaning* aligns with the rest of the language: `=` is now contains (substring),
matching `<=`, `!=`, and the body value-match, where a marker `=` previously meant
exact equality. `?` (regex) is unchanged. Exact matching is expressed the same way
everywhere - anchor a regex:

```relux
# skip unless TARGET ? ^(linux|darwin)$   # regex, unchanged
# run if PATH = /usr/local/bin            # contains (was exact)
# skip unless OS ? ^linux$                # exact, via anchored regex
```

Markers remain truthiness tests: `=` returns the LHS if it contains the RHS, empty
otherwise; `?` returns the match or empty. A non-match is falsy, never a failure.
The bare truthy form (`# skip unless X`) continues to work.

With `=` meaning contains in every position, the literal/regex semantics of a
match are identical in a marker condition and a body statement; the two differ
only in failure behavior - a condition is falsy on no-match, a statement asserts.

### Where the value-match statement appears

- shell blocks;
- regular `fn` bodies;
- `pure fn` bodies (a new statement form alongside `let` and reassignment).

Not in test- or effect-level let RHS positions, overlay values, or cleanup
blocks - a value-match is a statement, never a value-producing expression.

### Value matches are assertions; markers are conditions

Relux has no error-handling constructs, and value matching does not add any. A
value-match behaves by syntactic position, the same way the match glyphs already
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
test. The value-match assertion carries the same failure context a shell match
does (value, pattern, is_regex, call stack, vars in scope), so the report is as
rich.

Mechanically, an assertion reached deep inside a pure-fn call must unwind back to
the statement and then to the test boundary - exactly how a shell-match failure
already unwinds (the VM returns `Err(Failure)` up to the test runner). To carry a
value-match failure the same way, pure evaluation gains a fallible return -
`eval_pure_expr` / `eval_pure_fn` return `Result<String, PureEvalError>` - where
the error is the *unwinding vehicle*, not a value the DSL can observe or handle:

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum PureEvalError {
    #[error("value match failed: value did not satisfy pattern")]
    MatchFailure {
        value: String,
        pattern: String,
        is_regex: bool,
        span: IrSpan,
        call_stack: Vec<PureFrame>,   // for the failure report, not for handling
    },
}

#[derive(Debug, Clone)]
pub struct PureFrame {
    pub name: String,        // the pure fn being called
    pub call_site: IrSpan,   // where the call appears in source
}
```

A failed match is the only failure the evaluator raises; all other invalid states
(undefined identifiers, malformed regex, arity mismatches, cycles) are still
caught at lowering time. `call_stack` is innermost-first and is reversed for
outermost-first display in the report.

### The one place position matters: the marker decision phase

A value-match statement can live in a `pure fn` body, and a marker condition can
call a `pure fn`, so a statement-position match can be *reached* during marker
evaluation. Markers cannot fail a test, so in that phase a reached non-match
resolves the condition to false instead of failing - the same falsy outcome the
truthiness form produces directly. This is not error handling; the marker
decision phase is simply, structurally, non-failing.

Concretely (after the hierarchical `.env` refactor, marker decisions run per-test
at resolve time, on the per-test `LayeredEnv`, before any shell starts):

- **`decide_markers`** treats a reached `MatchFailure` as `met = false`, kept
  distinct from `LoweringBail::invalid` (a malformed regex still bails to
  `Invalid`; a value that does not match is just "condition not met"). The
  `RecordingSink` accumulates ops by side effect, so the ops trail survives for
  the viewer.

At runtime there is no special case: every context that reaches a failed
value-match fails the test - shell-block statement, test-level `let`,
effect-level `let`, overlay value. Three of the four runtime call sites already
return a `Result`; **`eval_effect_let` must grow a `Result` signature** so an
assertion inside an effect-level `let` can fail the test like any other. There is
no lowering-time / constant-folding pure-eval call site, so no lowering path needs
to turn a match failure into an `InvalidReport`.

### Structured-log recording

The single `EventKind::PureMatch` event used by markers today is replaced by a
trio mirroring shell match's `MatchStart` / `MatchDone` / `Timeout` shape:

- `EventKind::PureMatchStart { value, pattern, is_regex }` - emitted before the
  match runs. `value` is the haystack inline (shell match reads it from the
  buffer instead).
- `EventKind::PureMatchDone { matched, captures }` - emitted on success.
- `EventKind::PureMatchFailed {}` - emitted on failure, before `PureEvalError`
  propagates. Empty payload; the preceding `PureMatchStart` in the same span
  carries the context.

Sequences: success is `PureMatchStart` + `PureMatchDone`; failure is
`PureMatchStart` + `PureMatchFailed`, then the error propagates. Events are
recorded before the error propagates, so the timeline always shows the attempt.

The `MatchKind` enum (currently defined in both `relux-ir/pure_sink.rs` and
`observe/structured/span.rs`) is removed; `is_regex: bool` is used everywhere,
consistent with `MatchStart.is_regex`, `FailPatternSet.is_regex`, and
`FailPatternTriggered.is_regex`. `PureEvalSink::record_match(...)` is replaced by
`record_match_start` / `record_match_done` / `record_match_failed`, and `SinkOp`
mirrors the same trio.

### Failure record

`Failure::PureMatch` is a new variant in `report/result.rs`.
`FailureRecord::PureMatch` is the serialized counterpart in
`observe/structured/failure.rs`; it carries `value`, `pattern`, `is_regex`,
`span`, a unified `call_stack`, and `vars_in_scope`, and has no `buffer_tail`
field (value matches have no buffer). Pure-fn frames from
`PureEvalError.call_stack` integrate into the unified `call_stack` as
`StackFrame { kind: "pure-fn-call", ... }`, appended after shell frames and
reversed during translation so the unified list is top-down in display order.

Console and HTML renderers branch on the `Failure` discriminant. The `PureMatch`
arm renders a "value match did not satisfy pattern" header, `value:` and
`pattern:` lines, an `at <span> (value match)` line, omits the buffer-tail
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

5. **Marker `=` shifts from exact equality to contains.** The syntax is unchanged
   (bare `=`), but `X = v` in a marker now tests containment, consistent with `<=`
   and the body form. Anchored regex (`X ? ^v$`) preserves exact matching. Marker
   `?` is unchanged.

Marker *syntax* does not change - bare `=` / `?` stay, and there is no operator
migration to `|?` / `|=`. The only marker-facing change is the `=` semantic
alignment in item 5, kept so the "contains everywhere, anchor for exact" rule
holds without exception.

### Migration table

| Before                              | After                                   | Notes                                              |
|-------------------------------------|-----------------------------------------|----------------------------------------------------|
| `let x = e`                         | `let x := e`                            | declaration                                        |
| `x = e`                             | `x := e`                                | reassignment                                       |
| `start E as a { K = e }`            | `start E as a { K := e }`               | overlay value                                      |
| `# skip unless X ? p` (regex)       | unchanged                               | marker `?` unchanged                                |
| `# run if OS = linux` (exact)       | `# run if OS ? ^linux$`                 | marker `=` is now contains; anchor for exact        |
| `> echo "${v}"` + `<? p` + `$1`     | `pure fn f(s) { s ? p; $1 }` + `let id := f(v)` | value match, no shell round-trip           |
| exact value check                   | `v ? ^x$`                               | anchor a regex, as with `<? ^x$`                    |

## Architecture and change surface

### `relux-lexer`

Add `Token::Colon` (`#[token(":")]`). No new match token is needed - `=` and `?`
already exist. Verify the interpolation literal-merge step (`coalesce_parts`)
still handles `?` and `=` inside pattern bodies as pattern content.

### `relux-parser`

- `operator.rs`: add `op_bind` (composes `Colon + Eq` into `:=`) and the
  value-match operators `op_value_match_literal` (bare `Eq`, infix) and
  `op_value_match_regex` (bare `Question`, infix).
- `stmt.rs`: reassignment parses `name := expr`; `let` parses `let name := expr`;
  add the value-match statement form (LHS expr, then `=` / `?`, then a pattern)
  recognized in shell-block, regular-fn-body, and pure-fn-body contexts. The
  parser tries the value-match production and the reassignment production so that
  `x = p` (match) and `x := p` (bind) are disambiguated by the `:` .
- Overlay parsing (`start ... { KEY := expr }`) moves to `:=`.

### `relux-ast`

Add `AstStmt::ValueMatchRegex { lhs, pattern, span }` and
`AstStmt::ValueMatchLiteral { lhs, pattern, span }`. Binding statements switch to
the `:=` token. No `negate` field. Marker AST (`AstMarkerCondBody`) is unchanged.

### `relux-ir`

- `IrPureStmt` gains a `Match { lhs, pattern, is_regex, span }` variant.
  `eval_body` evaluates the LHS, emits `record_match_start`, runs the match, and
  on success emits `record_match_done` and binds captures, on failure emits
  `record_match_failed` and returns `Err(PureEvalError::MatchFailure)`.
- `regex_validate.rs` validates constant `?` patterns at lowering, as shell match
  does.
- `eval_pure_expr` / `eval_pure_fn` / `eval_body` become
  `Result<String, PureEvalError>`; `eval_pure_call` pushes a `PureFrame` on the
  error path.
- `marker.rs` catches `PureEvalError` from the evaluator and treats it as
  condition false, distinct from `LoweringBail::invalid`. Marker AST and glyphs
  are unchanged.
- `pure_sink.rs`: `PureEvalSink` gains the match-start / done / failed methods;
  `SinkOp` gets the trio; `MatchKind` is removed.
- The pure evaluator gains a `Captures` frame parallel to `VarScope`, populated on
  a successful match in a pure-fn body; `$n` reads check it first.

### `relux-runtime`

- `vm/mod.rs`: handle the new shell-statement match variant; success emits
  Start+Done and updates the shell capture frame, failure emits Start+Failed and
  builds `Failure::PureMatch`.
- `lib.rs` (test-level `let`), `effect/mod.rs` (`eval_overlay`, `eval_effect_let`):
  propagate `PureEvalError` and translate to `Failure::PureMatch`.
  **`eval_effect_let` must change from `()` to a `Result` signature.**
- `observe/structured/`: the trio of new event kinds; remove `PureMatch` and
  `MatchKind`; add builder emit methods and `FailureRecord::PureMatch`.
- `report/result.rs`: `Failure::PureMatch`.
- `report/console.rs`, `report/event_html.rs`: branch on the `Failure`
  discriminant for the value-match rendering.

TS bindings regenerate via `just build-viewer`; the vendored
`vendor/relux-viewer.js.gz` is rebuilt and committed in the same change.

### `viewer/`

New timeline event kinds (`PureMatchStart` / `Done` / `Failed`) with a distinct
row treatment from shell-match rows; a detail panel showing value, pattern,
is_regex, and captures; a failure-detail branch on `FailureRecord::PureMatch`;
colocated Vitest coverage.

### Editors and highlighting

`:=` and the value-match use of `=` / `?` get entries in
`editors/vscode/syntaxes/relux.tmLanguage.json`,
`editors/intellij/.../ReluxLexer.flex` (and related token/highlighter files), and
the canonical hljs grammar at
`crates/relux-runtime/src/report/highlight-relux.js`.

### Documentation

Assignment relocation touches nearly every tutorial and reference article that
shows a `let` or reassignment. Beyond the mechanical `=` -> `:=` sweep: a new
reference article on value matching; `03-built-in-functions` (pure fns are now
fallible); `02-syntax` (the `:=` binding form and the value-match statements);
`00-semantics` and the events.json schema (new event kinds and
`FailureRecord::PureMatch`); a `dsl-tutorial` chapter or extension; a
`suite-tutorial` helper-fn example.

## What changed from the original draft

- **Dropped `|?` / `|=`.** Value matching uses bare `=` / `?`; no new match glyph.
- **Markers keep their syntax, not their `=` meaning.** Bare `?` / `=` stay (no
  `|?` / `|=` migration), but marker `=` aligns to contains like every other
  literal match; exact is anchored regex. The draft's *syntactic* migration is
  dropped; the semantic alignment is kept so the uniform rule holds.
- **Assignment relocates to `:=`.** This is the new enabling change and the
  dominant breaking cost.
- **Reframed failure as fail-fast, not propagation.** A value-match assertion
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
2. **Reassignment footgun.** `x = e` (match) versus `x := e` (bind) differ by one
   character. The failure direction is loud, and `let` is keyword-protected, but
   it is worth confirming this is acceptable versus, for example, requiring a
   leading keyword for bare reassignment.
3. **Negated value match.** Deferred. If an "assert does not match" body form is
   wanted, a follow-up RFC can add it without disturbing this design.
