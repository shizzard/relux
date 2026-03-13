# Regex Matching

[Previous: Variables](06-variables.md)

The previous articles introduced [literal matching](03-send-match-and-logs.md) (`<=`) for checking that specific text appears in the output, and [variables](06-variables.md) for storing and reusing values. But there is a gap between the two: how do you extract a *part* of the output and store it in a variable?

Consider a command that prints a version string like `server v3.2.1 started on port 8080`. With literal match, you can verify the whole string appeared — but you cannot pull out `3.2.1` or `8080` separately. You might need the port number to connect from another shell, or the version to include in a log message. Literal match gives you all-or-nothing: the entire matched text, or nothing at all.

Regex matching solves this. The `<?` operator matches output using a regular expression, and **capture groups** let you extract specific pieces of the matched text into numbered variables that you can use in subsequent operations.

Here is a test that extracts a date from command output and uses each part separately:

```relux
test "parse a date" {
    shell s {
        > echo "2026-03-08"
        <? ^(\d{4})-(\d{2})-(\d{2})$
        > echo "year=${1} month=${2} day=${3}"
        <? ^year=2026 month=03 day=08$
    }
}
```

This test comes from `tests/relux/tests/variables/capture_groups.relux` in the Relux source tree.

The `<?` operator matches the output against the regex pattern `^(\d{4})-(\d{2})-(\d{2})$`. The three parenthesized groups capture the year, month, and day. After the match, `${1}`, `${2}`, and `${3}` hold those values — and they can be used in the next send, just like any other variable.

## The `<?` operator

The regex match operator `<?` works like [literal match](03-send-match-and-logs.md) (`<=`) in most ways: it scans forward from the [cursor](04-the-output-buffer.md), waits up to the timeout for a match, and advances the cursor past the matched text when it succeeds. The difference is in how it interprets the pattern.

Where `<=` treats its payload as a plain substring to find, `<?` compiles it as a regular expression. The regex flavor is Rust's [`regex`](https://docs.rs/regex/latest/regex/) crate — a Perl-compatible syntax without lookahead or backreferences, but with full support for character classes, quantifiers, anchors, alternation, and capture groups. Multi-line mode is enabled by default, so `^` and `$` match the start and end of each line, not just the start and end of the entire buffer.

Like `<=`, the `<?` operator with an empty pattern acts as a [buffer reset](04-the-output-buffer.md) — it consumes everything currently in the buffer without matching anything specific.

A simple regex match looks almost identical to a literal match:

```relux
test "basic regex match" {
    shell s {
        > echo hello-relux
        <? ^hello-relux$
    }
}
```

## Capture groups

Parentheses in a regex pattern create **capture groups**. When the match succeeds, each group's matched text becomes available through a numbered variable: `${1}` for the first group, `${2}` for the second, and so on. `${0}` holds the full match — everything the regex matched, not just the groups.

Here is a test that shows all three levels — full match, first group, second group:

```relux
test "full match via capture group zero" {
    shell s {
        > echo "hello world"
        <? (hello) (world)
        > echo "full='${0}' first='${1}' second='${2}'"
        <? ^full='hello world' first='hello' second='world'$
    }
}
```

`${0}` is `hello world` (the entire matched text), `${1}` is `hello`, and `${2}` is `world`.

If you access a capture group that does not exist — say `${5}` when the regex only has one group — it resolves to the empty string, just like an [undefined variable](06-variables.md):

```relux
test "missing capture group returns empty string" {
    shell s {
        > echo "one=1"
        <? ^one=(\d+)$
        > echo "five='${5}'"
        <? ^five=''$
    }
}
```

## Captures are replaced on every match

Each `<?` match replaces **all** capture groups from the previous match. If the first match produced `${1}` and `${2}`, and the second match has only one group, `${2}` becomes empty — it does not retain its old value:

```relux
test "captures overwritten by next match" {
    shell s {
        > echo "key=abc val=xyz"
        <? ^key=(\w+) val=(\w+)$
        > echo "g1=${1} g2=${2}"
        <? ^g1=abc g2=xyz$
        > echo "only=one"
        <? ^only=(\w+)$
        > echo "g1=${1} g2='${2}'"
        <? ^g1=one g2=''$
    }
}
```

After the second `<?`, `${1}` is `one` and `${2}` is gone. The captures from the first match are completely discarded.

## Saving captures to named variables

Because captures are replaced on every match, you can save a captured value into a named variable with [`let`](06-variables.md) to keep it around:

```relux
test "capture into variable" {
    shell s {
        > echo "key=alpha"
        <? ^key=(\w+)$
        let saved = ${1}
        > echo "other=beta"
        <? ^other=(\w+)$
        > echo "saved=${saved} current=${1}"
        <? ^saved=alpha current=beta$
    }
}
```

`let saved = ${1}` reads the current value of `${1}` (which is `alpha`) and stores it in a named variable. When the second match replaces captures, `${1}` becomes `beta` — but `saved` still holds `alpha`.

## `let` with a regex match expression

You can combine `let` and `<?` in a single statement. When you write `let result = <? pattern`, Relux performs the match *and* assigns the return value to the variable. The return value of a regex match is the full match text — the same as `${0}`:

```relux
test "let from match expression captures full match" {
    shell s {
        > echo "code=42"
        let result = <? code=(\d+)
        > echo "result='${result}' group='${1}'"
        <? ^result='code=42' group='42'$
    }
}
```

`result` gets `code=42` (the full match), while `${1}` gets `42` (the first capture group). This is the same behavior as other expressions you have seen in the [everything has a value](06-variables.md) table — `<?` returns the full match text, and `let` stores it.

## Variable interpolation in patterns

Like all operators in Relux, `<?` supports [variable interpolation](06-variables.md) in its pattern. Variables are resolved before the pattern is compiled as a regex:

```relux
test "interpolation in regex pattern" {
    shell s {
        let key = "version"
        > echo "version=42"
        <? ^${key}=(\d+)$
        > echo "captured ${1}"
        <? ^captured 42$
    }
}
```

The pattern `^${key}=(\d+)$` becomes `^version=(\d+)$` after interpolation.

## Best practices

### Use regex only when you need it

You might default to `<?` everywhere since it is strictly more powerful than `<=` — any literal match can be written as a regex. But regex matches are harder to read, easier to get wrong, and can match more than you intended.

Literal match `<=` is a simple substring search. It does exactly one thing and it is obvious what it matches. When you do not need capture groups, anchors, or wildcards, `<=` is the better choice. Reserve `<?` for when you genuinely need regex capabilities: extracting values, matching variable output, or anchoring to line boundaries.

### Always save captures to named variables

Capture groups like `${1}` are convenient — you match a pattern, and the extracted value is right there. It is tempting to use `${1}` directly in several places without saving it to a named variable first.

The problem is not with the code as you write it today. The problem is with the code as someone changes it five years from now. Test code is still code — it evolves, gets refactored, gets extended. Capture groups are silently replaced on every `<?` match. If someone inserts a new regex match between your capture and its use — a perfectly reasonable edit — `${1}` now refers to something completely different. No error, no warning, just a test that fails in a confusing way that takes hours to debug.

Save the capture to a named variable immediately after the match, before doing anything else. Then use the named variable everywhere:

```relux
# Fragile — ${1} can be silently replaced by a later edit:
<? ^port=(\d+)$
> curl http://localhost:${1}/health

# Durable — the port is safe no matter what happens next:
<? ^port=(\d+)$
let port = ${1}
> curl http://localhost:${port}/health
```

The named variable survives any number of subsequent matches. It makes the code self-documenting (the name `port` says more than `${1}`), and it insulates the test from future edits.

### Anchor your patterns

A regex without anchors will match anywhere in the remaining buffer — the echoed command, a fragment of the prompt, leftover output from a previous step. This is the same problem as with [literal match](04-the-output-buffer.md), but worse, because regex metacharacters like `.` and `*` match more broadly.

Use `^` and `$` to pin your match to a specific line:

```relux
# Might match the echoed command or something unexpected:
<? version=\d+

# Matches exactly one complete line:
<? ^version=\d+$
```

This does not mean you should anchor every pattern — sometimes a substring regex is what you need. But when you have a choice, anchoring is safer: it documents your intent and prevents accidental matches.

### Be careful with interpolated regex patterns

Variable interpolation in `<?` patterns lets you define reusable regex fragments — declare a pattern once at the test level and use it in multiple matches. This is handy for repeated patterns like timestamps, UUIDs, or version strings.

The catch is that after interpolation, the variable's value becomes part of the regex. If the value contains regex metacharacters — `.`, `*`, `+`, `(`, `[`, and so on — they are interpreted as regex syntax, not as literal text. A variable holding `192.168.1.1` does not match the literal IP address; the `.` matches any character, so it also matches `192X168Y1Z1`.

When the variable comes from your own `let` and you know the value, this is fine — just be aware of what you are putting into the pattern. When the variable comes from captured output or an environment variable, the content is unpredictable and the regex may compile into something you did not intend, or fail to compile entirely.

## Try it yourself

Write a test that does the following:

1. Run a command that produces output with two key-value pairs on the same line — something like `echo "host=db.local port=5432"`.
2. Use a single `<?` with two capture groups to extract both values into `${1}` and `${2}`.
3. Immediately save both captures to named variables (`let host = ${1}`, `let port = ${2}`).
4. Run another command that produces different output and match it with `<?` — this will overwrite the capture groups.
5. Verify that the named variables still hold the original values by echoing them back and matching the result.

This exercise combines capture groups, the save-to-variable pattern, and the ephemeral nature of captures — all the pieces from this article.

---

Next: [Functions](08-functions.md) — extract reusable test logic into named, parameterized functions
