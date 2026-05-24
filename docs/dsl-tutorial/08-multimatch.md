# Multimatch

[Previous: Regex Matching](07-regex-matching.md)

Most match operations expect output to arrive in a predictable order — one line, then the next, then the next. But some commands produce output where the order genuinely does not matter. A request that triggers two independent jobs in parallel will print their completion messages in whatever order they happen to finish. A batch operation that pulls reports from several sources will interleave them differently every time. The interleaving is incidental; the test only cares that every expected line showed up.

The match operators you have seen so far cannot express that. Each `<?` or `<=` matches one pattern at the cursor's current position; once the cursor advances past a match, those bytes are gone. To say "I expect lines A and B in any order", you have to commit to an order in your test — and accept the flake whenever reality picks a different one.

Multimatch closes this gap. The `<{ ... }` operator takes a block of patterns and waits for *every* one of them to appear, in any order, before the block completes. The cursor stays still while the block runs and advances once, at block exit, past the farthest match.

```relux
test "multimatch any-order reversed" {
    shell s {
        > printf 'job-b: done\njob-a: done\n'
        <{
            ? ^job-a: done$
            ? ^job-b: done$
        }
    }
}
```

The `printf` emits two lines: `job-b: done` first, then `job-a: done`. The `<{ ... }` block lists patterns in the opposite order — `job-a` first, then `job-b`. Both patterns match successfully because the order in the block is independent of the order in the output. Run this test, change which line `printf` emits first, and it still passes. That is the property multimatch buys you.

## The `<{ ... }` block

A multimatch block opens with `<{` and closes with `}`. Inside, each line is one pattern:

```relux
<{
    ? <regex>
    = <literal>
    ? <another regex>
}
```

The `?` prefix selects regex matching — the same syntax and behavior as a single `<?`. The `=` prefix selects literal matching — the same substring search as a single `<=`. You can mix them freely; each pattern is independent.

Each inner pattern works the same way it would as a single match. Variable interpolation, anchors, character classes, alternation — everything is identical. The difference is what happens when each pattern succeeds: instead of completing the match operation and advancing the cursor, it just marks that one pattern as satisfied, and the block keeps waiting for the others.

The block completes only when every pattern has matched at least once. If one of them never appears, the block fails — we will see that in a moment.

Two small rules:

- The empty form `<{ }` is a parse error. There is nothing to wait for, so Relux rejects the block at compile time rather than letting it match instantly.
- Comments are permitted between inner lines, the same way they are anywhere else in Relux.

## The atomic-cursor rule

In a regular `<?` or `<=`, the [output buffer](04-the-output-buffer.md) cursor advances past every successful match. The next match starts searching from the new cursor position. This works because the patterns run one at a time, in source order.

Multimatch cannot use that model. If pattern A matched and advanced the cursor first, pattern B would have to search past A's match-end — but if B's match was earlier in the buffer, it would have been consumed and gone. The "any order" guarantee would not hold.

To make this work, the block is atomic with respect to the cursor: while the block is open, the cursor sits at its block-entry position and does not move. Every byte that arrives is offered to every still-unmatched pattern, scanning from the same starting point. Once a pattern matches, it stops participating in further scans — but the cursor stays put. Other patterns keep scanning the same buffer slice from the same anchor.

When the last pattern finally matches, the cursor advances once, to the maximum of the per-pattern match-end offsets. After the block, the rest of your test sees a buffer where everything up to (and including) the farthest match has been consumed.

The trade-off is straightforward: per-pattern matches inside the block do not consume the buffer the way `<?` does outside the block. That is what gives multimatch its "any order" property — and what makes it different from chaining several `<?` calls. Chained `<?` calls would impose an order; multimatch deliberately does not.

## Mixing literal and regex patterns

The `?` and `=` prefixes can appear in the same block. A common case is a literal anchor for a known string plus a regex for a value you can't predict ahead of time:

```relux
test "multimatch literal plus regex" {
    shell s {
        > printf 'batch complete\n12 items processed\n'
        <{
            ? ^\d+ items processed$
            = batch complete
        }
    }
}
```

Two lines arrive in source order: `batch complete`, then `12 items processed`. The literal pattern matches the first line; the regex pattern matches the second. The block completes when both have matched, and the cursor advances past the regex match — which ends farther into the buffer.

If the patterns matched at different positions, the cursor would still advance to whichever is farthest. The closer match's trailing bytes (here, the newline after `batch complete`) get consumed too — they fall behind the cursor along with the farther match's bytes. This means a multimatch block always leaves the buffer in a clean post-block state, regardless of which pattern reached farthest in the output.

## When a pattern doesn't appear

If one of the patterns never matches, the block waits — and eventually times out, the same way a single `<?` or `<=` does when its expected output never arrives. The whole block shares one timeout, inherited from the prevailing shell timeout.

When the timeout fires, the test fails with a per-pattern report showing which patterns matched and which timed out:

```text
Error: multimatch in shell `s` did not satisfy all patterns
    │
    │   Note: multimatch did not satisfy all patterns (2 matched, 1 timed out)
    │         matched:     ? ^job-a: done$
    │         matched:     ? ^job-b: done$
    │         timed out:   ? ^job-c: done$
```

The header counts how many patterns made it and how many did not. Each pattern is listed below with either `matched:` or `timed out:` next to it, in the same `? ` / `= ` form you wrote in the block. You see immediately which expected lines failed to arrive.

## Best practices

### Captures don't bind in multimatch

After a single `<?`, `$1` and `$2` hold the values from the capture groups in your pattern. After a `<{ ... }` block, capture variables hold whatever they had **before** the block — multimatch ignores capture groups entirely.

```relux
// Looks fine, silently wrong: $1 holds nothing useful here.
<{
    ? ^port=(\d+)$
    ? ^host=(.+)$
}
> curl http://localhost:${1}/health
```

`$1` either resolves to the empty string (if no prior `<?` set it) or to whatever an earlier `<?` left there. The parentheses in the multimatch patterns are syntactic only; they accept the regex but do not bind the result.

If you need to extract a value, do the match outside the block with a regular `<?`:

```relux
<{
    = batch complete
    ? ^\d+ items processed$
}
<? ^port=(\d+)$
let port = $1
> curl http://localhost:${port}/health
```

Multimatch is for "wait for these things to all appear", not for parsing. When you need a value, use a single `<?` after the block.

## Try it yourself

Write a test that:

1. Spawns a command that emits two log lines together — for instance `printf 'b\na\n'` so both lines arrive in one write.
2. Uses `<{ ... }` with two patterns to match the two lines, one as a regex (`? ^...$`) and one as a literal (`= ...`).
3. After the block, sends a third command (`echo done`) and matches its output with a regular `<? ^done$`.

If the third match succeeds, the multimatch block correctly advanced the cursor past both inner matches. If the third match fails or times out, open the test log viewer and look at the buffer pane for the failing event — the consumed region tells you where the cursor actually landed, and that should make the failure easy to diagnose.

---

Next: [Functions](09-functions.md) — extract reusable test logic into named, parameterized functions
