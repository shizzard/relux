# R013: Pure String Match

- **Status**: draft
- **Created**: 2026-05-21

## Abstract

Introduce a pure string-match primitive usable in every pure expression position and every
statement position where shell match operators run today. Four new operators (`|?`, `|=`,
`|!?`, `|!=`) match a string-valued left-hand side against a regex or literal pattern,
optionally negated, without involving a shell. Marker condition syntax migrates onto the
same operators - the existing bare `?` and `=` forms are removed. As part of this change,
pure-fn evaluation becomes fallible: a match failure raises a typed error that propagates
through pure-fn calls, surfaces as a test failure in shell / fn contexts, and is caught and
recorded as "condition false" in marker contexts.

## Motivation

### Asserting and extracting from string values

A user who holds a string value - the return of a function, a `let` binding, an interpolation
- and who wants to assert its shape or pull out a substring has no in-language way to do so.
The current workaround is to echo the value through a shell and match the echoed output:

```relux
let value = compose_payload("alice")
> echo "${value}"
<? ^id=(\d+);user=(\w+)$
let id = $1
let user = $2
```

This is dishonest. The match has no business going through a PTY - it's a pure operation on
a known string. The shell roundtrip:

- pollutes the event log with a `send` plus a `match` plus the prompt match that follows;
- depends on the prompt matching cleanly afterwards to leave the buffer in a usable state;
- risks the echoed-command trap (matching the echo of the `echo` rather than the output).

The information is in scope. We should be able to match on it directly.

### Marker conditions already do pure matching, in isolation

Marker conditions today accept `X ? "pattern"` (regex) and `X = "value"` (exact equality):

```relux
# skip unless TARGET_OS = "linux"
# skip unless ARCH ? ^(x86_64|aarch64)$
```

The evaluation goes through the same pure expression evaluator that pure-fn calls use.
It records its work into the structured event log via `PureEvalSink::record_match`. But
the mechanic stops at the marker boundary - the same expressive power is unavailable
inside test bodies, function bodies, or pure-fn bodies.

There is no reason for the asymmetry beyond "we never got around to it."

### Unifying the two

This RFC removes the asymmetry by introducing one primitive shape - pure string match -
that markers and regular code both use, with one syntax. The cost is that pure-fn
evaluation has to become fallible (a match in a pure-fn body can fail), and marker
condition syntax has to migrate (we keep one operator family in the language, not two).

## Proposal

### Syntax

Four new operators:

| Operator | Behavior |
|---|---|
| `<expr> \|? <pattern>` | LHS string is matched against regex `<pattern>`. Fails if no match. |
| `<expr> \|= <pattern>` | LHS string is searched for substring `<pattern>` (multi-line LHS is fine). Fails if not contained. |
| `<expr> \|!? <pattern>` | Fails if LHS matches the regex. |
| `<expr> \|!= <pattern>` | Fails if LHS contains the substring. |

`<pattern>` is a string literal with interpolation, identical to the right-hand side shape
of `<?` / `<=` today. `<expr>` is any pure expression - variable, interpolation,
pure-fn call, pure BIF call.

The pipe metaphor reads as "feed the LHS into the match." The symbols mirror `<?` / `<=`
structurally so the literal-vs-regex distinction is the same everywhere in the language.

No timed forms - pure matches are instantaneous. No bare-reset form - there is no buffer
cursor to reset.

### Where it appears

As a **statement**, the new operators are valid in:

- shell blocks;
- regular `fn` bodies;
- `pure fn` bodies (a new statement form alongside `let` and assignment);
- the let-from-match expression: `let id = ${value} |? ^id=(\d+)$` binds `id` to the
  full match (group 0), mirroring `let x = <? pattern`.

The let-from-match form is valid in any context where the underlying pure-match statement
is - shell-scoped `let`, test-level `let`, effect-level `let`, overlay values.

Cleanup blocks are unchanged - they remain restricted to `>` / `=>` / `let` / assignment.

As a **condition**, the new operators replace the existing marker condition syntax:

```relux
# skip unless TARGET |? ^(linux|darwin)$
# run if PATH |= /usr/local/bin
# flaky if HOSTNAME |!? ^build-
```

The bare truthy form (`# skip unless X`) continues to work.

### Captures

Successful regex matches populate numeric captures `<0>..<n>` exactly as shell `<?` does.
Captures live until the next pure or shell match in the same block (or until the end of
the enclosing function scope). A failed match never binds captures - failure aborts
evaluation before the binding step is reached.

### Examples

```relux
test "extract id from helper output" {
    """
    Verify the helper returns a well-formed id string.
    """
    let payload = build_id_payload("alice")
    ${payload} |? ^id=(\d+);user=(\w+)$
    let id = <1>
    let user = <2>
}
```

```relux
pure fn version_major(s) {
    s |? ^v?(\d+)\.\d+\.\d+$
    <1>
}
```

```relux
fn assert_clean(output) {
    ${output} |!? (?i)error|panic|fatal
}
```

```relux
# skip unless TARGET |? ^(linux|darwin)$
# run if PATH |= /usr/local/bin
```

### Semantics: pure-fn evaluation becomes fallible

The pure expression evaluator changes from infallible to fallible. Its return type shifts
from `String` to `Result<String, PureEvalError>`. A new error type carries the match
payload:

```rust
pub enum PureEvalError {
    MatchFailure {
        value: String,
        pattern: String,
        kind: MatchKind,
        negate: bool,
        span: IrSpan,
        call_stack: Vec<PureFrame>,
    },
}
```

A failed match is the (currently only) failure mode. Other invalid states - undefined
identifiers, malformed regex, arity mismatches, cycles - continue to be caught at
lowering time, before the evaluator runs.

### Failure propagation and reinterpretation

A `PureEvalError` raised at any depth bubbles up through the evaluator. Each surrounding
context surfaces it in the way that fits:

| Context | Surface |
|---|---|
| Shell-block statement | Runtime `Failure` with `FailureContext` (call stack, vars in scope, the failing value and pattern). Test fails. |
| Regular `fn` body | Same - failure surfaces in the calling shell statement. |
| Test- or effect-level `let` RHS | Test marked invalid via `InvalidReport`. The diagnostic cites the match failure with span and value. |
| Overlay value | Same as test-level let - invalid. |
| Marker condition | Caught and reinterpreted as condition `false`. The full chain of recorded ops plus the match attempt is kept on the `MarkerRecording` so the viewer can show the user *why* the marker did not fire. |
| Shell-scoped `let` RHS or interpolation | Test failure. |

The marker rule is the single special case: markers cannot fail tests by definition, so a
match failure is treated as "condition not met." This preserves the existing semantic of
`# skip if X = "y"` - the marker does not fire when the condition is false - while letting
the underlying primitive be a real assertion everywhere else.

### Structured-log recording

The existing `PureEvalSink::record_match` is emitted by the new operators in every
context, runtime and lowering. The viewer's events list renders pure matches as
first-class events distinct from shell matches: a `source: Pure` discriminator on the
match event keeps the schemas aligned but visually separated. On failure, the marker-eval
row in the viewer gains an explicit "match failed - condition false" indicator pointing at
the value and pattern that did not match.

## Migration

### Breaking changes

1. **Marker `?` operator is removed.** Replace with `|?`.

   ```relux
   # before
   # skip unless ARCH ? ^(x86_64|aarch64)$
   # after
   # skip unless ARCH |? ^(x86_64|aarch64)$
   ```

2. **Marker `=` operator is removed and its semantics shift.** Replace with `|=`. The new
   operator is substring, not exact equality. To preserve exact equality, use anchored
   regex.

   ```relux
   # before (exact equality)
   # skip unless TARGET_OS = "linux"
   # after (substring - matches "linux", "linux-arm", "ubuntu-linux", ...)
   # skip unless TARGET_OS |= linux
   # after (exact equality preserved)
   # skip unless TARGET_OS |? ^linux$
   ```

3. **Pure-fn evaluator becomes fallible.** External embedders of the `relux-ir` crate see
   a `Result` return on `eval_pure_expr` / `eval_pure_fn`. Counts as a major bump.

A single `feat!:` conventional commit carries the breaking change through release-please
to the next major version.

### Migration table

| Before | After | Notes |
|---|---|---|
| `# skip unless X ? "p"` | `# skip unless X \|? p` | Regex, identical behavior. |
| `# skip if X = "v"` | `# skip if X \|= v` | **Substring now**; for exact use `\|? ^v$`. |
| `> echo "${value}"` + `<? pat` | `${value} \|? pat` | Pure - no shell roundtrip. |
| `> echo "${value}"` + `<= lit` | `${value} \|= lit` | Substring on the value. |

## Open questions

- Whether the AST and IR use one shared node for the statement, expression, and marker
  forms, or three structurally identical but distinct nodes. The shapes are identical;
  decision is parser ergonomics. Settled during implementation planning.
- Whether `negate` is an extra field on `MatchKind`, or a separate `bool` on the recording.
  Tradeoff is a larger enum vs. a per-event flag. Settled during implementation planning.
