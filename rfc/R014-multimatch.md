# R014: Multimatch

- **Status**: implemented
- **Created**: 2026-05-22

## Abstract

Add a single new statement form, `<{ ?... ?... }` (with timed variant `<~Ns{ ?...
?... }`), that matches multiple patterns against the shell buffer concurrently,
atomically, with no ordering constraint. Each inner pattern is independently regex
(`?`) or literal (`=`). The block completes when *every* pattern has matched at
least once; the cursor advances to the maximum match-end across all patterns. On
the block's timeout, the test fails with a record listing which patterns matched
and which did not. This is the last shell-side primitive needed for testing
systems whose output ordering is non-deterministic.

## Motivation

### Non-deterministically ordered output

A user request frequently triggers two or more independent jobs that run in
parallel and write their reports to the same stream. The interleaving of those
reports is not stable - across runs, identical inputs can produce different
orderings:

```
job-a: started
job-b: started
job-a: complete (id=17)
job-b: complete (id=23)
```

```
job-b: started
job-a: started
job-b: complete (id=23)
job-a: complete (id=17)
```

Today's single-match `<?` / `<=` forces one ordered sequence per statement; the
test author must guess one ordering and accept flakes when reality deviates.
There is no in-language way to express "I expect all of A, B, C to appear, in any
order."

The standard workarounds - capturing the whole batch into a buffer then running a
series of substring assertions, or matching against a giant regex enumerating
permutations `(A.*B.*C|B.*A.*C|...)` - are either fragile (cursor drift,
prompt-trapping) or combinatorially explosive.

The shell-match cursor model is also incompatible: each `<?` advances the cursor
past its match-end, so a second pattern cannot be expected to match bytes the
first has already consumed. Multimatch solves this by treating per-pattern
match-end as internal state and advancing the user-visible cursor only once, at
block exit.

### The last shell-side primitive before 1.0

Pure string match (R013) closes the gap between marker conditions and body
assertions. Multimatch closes the gap between sequential and concurrent shell
output. Together they exhaust the routine assertion vocabulary an
integration-test author needs from the language.

## Proposal

### Syntax

One new statement, in two forms:

```
<{ <line>+ }            # inherits the prevailing match timeout
<~Ns{ <line>+ }         # block-level tolerance timeout, same syntax shape as <~Ns?
<@Ns{ <line>+ }         # block-level assertion timeout, same syntax shape as <@Ns?
```

`<line>` is exactly one of:

| Line | Behavior |
|---|---|
| `? <pattern>` | Inner regex match. Fails the block if `<pattern>` does not appear before the block timeout. |
| `= <pattern>` | Inner literal match. Fails the block if substring `<pattern>` does not appear before the block timeout. |

`<pattern>` is a string literal with interpolation, identical in shape to the
right-hand side of `<?` / `<=` today. Comments are permitted between lines.

The decomposition is mnemonic: `<` reads from the shell buffer; `?` selects regex
semantics; `=` selects literal semantics; `<~Ns` adds a per-statement timeout;
`<{` groups the read across multiple patterns. Braces follow the language's
existing block style. The closer is `}` (not `}>`); the opener `<{` already
carries the kind discriminator, so the closer reads as a plain block closer.

The empty form `<{ }` is a parse error - there is nothing to wait for. A
single-pattern form `<{ ? foo }` parses and runs, semantically equivalent to
`<? foo`; a future linter rule may suggest the single-match form.

### Where it appears

As a statement:

- Shell blocks.
- Regular `fn` bodies.
- Cleanup blocks (cleanup inherits the full shell-statement grammar per the
  project's existing rule).

Not valid in `pure fn` bodies (no buffer), nor in `let` right-hand sides
(multimatch is a statement, not an expression - mirroring `<?`).

### Captures

Regex patterns inside the block may include groups syntactically, but in v1 the
groups bind nothing - multimatch does not write to the shell's `Captures` frame.
Pre-existing captures from a prior single `<?` remain readable after the block,
unchanged; `$n` continues to resolve at runtime exactly as it does today. A user
who writes `$n` expecting it to extract from a multimatch pattern will see
either stale-capture data from an earlier match or the existing unbound-capture
runtime failure - identical to writing `$n` with no prior `<?` at all.

Named capture groups are the documented v2 path: they read identically here and
in a single match, and they sidestep the otherwise-ambiguous numeric-capture
indexing across N patterns. A v2 follow-up may also lift `$n` to a lowering-time
diagnostic if static capture-flow tracking is added to the IR.

### Semantics

The block is atomic with respect to the shell cursor: while the block runs, the
cursor sits at its block-entry position. Every byte that arrives is offered to
every still-unmatched pattern. A pattern transitions to "matched" the first time
its regex/substring succeeds against the buffer slice
`[block_entry, current_buffer_end]`; once matched, it no longer participates in
subsequent scans. The block completes when all patterns are matched.

At block completion the cursor advances to the maximum of the per-pattern
match-end offsets. Patterns may match on overlapping byte ranges; duplicate
inner patterns are independent slots that each match independently and may land
on the same bytes.

If the block timeout fires before all patterns have matched, the block fails.
The failure record carries the full pattern list with per-pattern
matched/unmatched status, the buffer tail, the call stack, and vars in scope.

Fail patterns remain active during the block. A fail-pattern trigger aborts the
block with `Failure::FailPattern` exactly as it would abort a single `<?` - any
per-pattern `MultiMatchPatternDone` events already emitted stay on the structured
log.

### Structured-log recording

Four new event kinds, mirroring the single-match `MatchStart` / `MatchDone` /
`Timeout` shape but accommodating partial state:

```rust
EventKind::MultiMatchStart {
    effective: TimeoutValue,
    patterns: Vec<MultiMatchPattern>,
},
EventKind::MultiMatchPatternDone {
    index: usize,           // index into MultiMatchStart.patterns
    elapsed: Duration,
    buffer_seq: EventSeq,   // -> the per-pattern `Matched` buffer event
},
EventKind::MultiMatchDone {
    advance_to: EventSeq,   // -> the per-pattern `Matched` whose match ends farthest
},
EventKind::MultiMatchTimeout {
    unmatched: Vec<usize>,  // pattern indices that did not match
},
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MultiMatchPattern {
    pub pattern: String,
    pub is_regex: bool,
}
```

Sequences:

- Success: `MultiMatchStart` + N `MultiMatchPatternDone` (emitted in
  match-completion order, not source order) + `MultiMatchDone`.
- Timeout: `MultiMatchStart` + 0..N `MultiMatchPatternDone` + `MultiMatchTimeout`.
- Fail-pattern abort: `MultiMatchStart` + 0..N `MultiMatchPatternDone`, then the
  existing `FailPatternTriggered` and `Failure::FailPattern` propagation. No
  `MultiMatchDone` / `MultiMatchTimeout` emitted.

Each `MultiMatchPatternDone` emits a `BufferEventKind::Matched` event with the
same shape as single-match (`before + matched + after` equals the buffer state
at the moment the pattern succeeded; `before` carries any stale prefix that
was already in the buffer). The per-pattern matched text and end offset live
on that buffer event - they are not duplicated on `MultiMatchPatternDone`.

`MultiMatchDone.advance_to` references the per-pattern `Matched` event whose
pattern ends farthest in the buffer. The viewer applies the cursor advance
once, at `MultiMatchDone` time, by dropping `len(before) + len(matched)`
bytes of the referenced event from its reconstructed buffer. Other per-
pattern `Matched` events inside the multi-match span are not, on their own,
cursor-advancing - they describe their pattern's match against the undrained
buffer for rendering and audit, and the actual block-end drain is the single
advance applied at `MultiMatchDone`.

The runtime drains its in-memory buffer at block exit without emitting a
new buffer event. No synthetic "drain" event is needed.

The viewer renders the block as a single row with per-pattern bullets - check /
cross / pending - and on failure surfaces the unmatched pattern list at the top
of the failure panel.

### Failure record

```rust
Failure::MultiMatch {
    patterns: Vec<MultiMatchPattern>,
    matched: Vec<usize>,         // pattern indices that matched
    span: IrSpan,
    context: FailureContext,     // call_stack, buffer_tail, vars_in_scope
},
```

`FailureRecord::MultiMatch` is the serialized counterpart on the structured log.
Console and HTML renderers branch on the failure discriminant and render the
per-pattern table with explicit OK / FAIL markers per pattern, plus the standard
buffer tail, call stack, and vars-in-scope sections.

### Examples

```relux
test "two parallel job reports appear in any order" {
    > trigger-batch alice
    <{
        ? ^job-a: complete \(id=\d+\)$
        ? ^job-b: complete \(id=\d+\)$
    }
}
```

```relux
test "literal plus regex with a bounded wait" {
    > trigger-batch alice
    <~10s{
        = batch complete
        ? ^\d+ items processed$
    }
}
```

```relux
fn await_parallel_jobs() {
    <~30s{
        ? ^job-a: done$
        ? ^job-b: done$
        ? ^job-c: done$
    }
}

test "delegated multimatch wait" {
    > start-three
    await_parallel_jobs()
}
```

## Migration

Purely additive. No existing programs change behavior. No removed event kinds,
no removed AST nodes, no breaking API on `relux-ir`.

The `Failure` enum gains a new variant (`MultiMatch`); the `EventKind` enum
gains four variants. External consumers of `events.json` see new event kinds and
a new failure discriminant, but exhaustiveness in their deserializers is the
consumer's contract - this is a minor schema extension. The conventional commit
is `feat:`, not `feat!:`.

## Open questions

None remaining. Earlier design questions - whether captures bind in v1; whether
negative inner patterns are needed; whether per-pattern timeouts are useful;
whether the closer is symmetric `}>` - were settled during the brainstorming
pass:

- No captures in v1; named groups are the documented v2 path.
- No negative inner patterns; fail patterns cover the use case.
- No per-pattern timeouts; block-level only.
- Asymmetric `}` closer to match the rest of the language.
