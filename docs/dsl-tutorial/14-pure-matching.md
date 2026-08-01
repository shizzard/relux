# Pure Matching

[Previous: Pure Functions](13-pure-functions.md)

Every match you have written so far has read a shell's output. In [Regex Matching](07-regex-matching.md) you scanned the bytes streaming out of a PTY with `<?` and `<=`, waiting for a pattern to appear. But often the thing you want to check is not streaming output at all — it is a value you already have in hand. A variable you built, a capture you saved, the string a function returned. You want to assert something about that value without sending anything to a shell.

That is a **pure match**. The name follows the same idea you met in [Pure Functions](13-pure-functions.md): in Relux, "pure" means shell-independent. A pure function computes a string without touching a PTY; a pure match asserts on a string without touching a PTY. It compares against one complete value, not a streaming buffer, so there is nothing to wait for — it either matches or it fails, right away.

Pure matching comes in the same two flavors as shell matching: `=` for exact equality and `?` for a regex. The forms read left to right — the value, then the operator, then the pattern:

```relux
test "assert a computed value" {
    shell s {
        > echo "release-3.2.1"
        <? ^release-(\d+\.\d+\.\d+)$
        let version := $1

        // Now assert on the captured value, no shell involved:
        version ? ^\d+\.\d+\.\d+$
        version = 3.2.1
    }
}
```

The `<?` on the third line matches shell output and captures `3.2.1` into `$1`, which we save as `version`. The last two lines never touch the shell. `version ? ^\d+\.\d+\.\d+$` compiles the right-hand side as a regex and checks it against the value of `version`. `version = 3.2.1` asserts exact equality — it passes only if `version` is precisely `3.2.1`.

## The `=` form: exact equality

`<expr> = <pattern>` passes only when the value on the left is **byte-for-byte equal** to the pattern on the right. It is not a substring test:

```relux
let os := "linux"
os = linux
```

This passes: `os` is exactly `linux`. But `os = lin` would fail — a substring is not enough — and `os = ubuntu-linux` would fail too, because a superstring is not equal either. The whole value has to match the whole pattern.

This is the key difference from the shell operator [`<=`](03-send-match-and-logs.md), which scans the [output buffer](04-the-output-buffer.md) and succeeds on any substring hit. `<=` answers "does this text appear somewhere in the output?" `=` answers "is this value exactly this?" When you need a substring or a pattern check on a value you hold, reach for `?` instead.

An empty pattern is a special case: `value =` matches only an empty (or unset) value.

## The `?` form: regex and captures

`<expr> ? <pattern>` compiles the pattern as a regular expression and passes when the regex matches anywhere in the value. Like `<?`, it is **unanchored** — add `^` and `$` yourself when you want to pin the match to the whole value.

The `?` form binds captures exactly like `<?` does. A successful match sets the numbered variables `$0` (the full match), `$1`, `$2`, and so on:

```relux
let greeting := "hello world"
greeting ? (hello) (world)
> echo "full=${0} first=${1} second=${2}"
<? ^full=hello world first=hello second=world$
```

After the pure match, `$0` is `hello world`, `$1` is `hello`, and `$2` is `world` — the same numbered captures a `<?` would set, ready to use in the next send. As with regex matching, captures are replaced on the next match, so save anything you need to keep with a [`let`](06-variables.md). The `=` form never binds captures; only `?` does.

## A no-match fails the test

A pure match is an **assertion**. Relux has no error handling and no way to catch a failed match, so a no-match is a hard failure that stops the test immediately, right on that line — exactly like a `<?` that never finds its pattern:

```relux
let status := "500"
status ? ^2\d\d$
```

`status` is `500`, the regex wants a `2xx`, so the test fails on that line. There is no negated form — pure matching only asserts that something *does* match, never that it does not.

Because the value is already in hand, a pure match cannot time out. There is no buffer to fill and nothing to wait for, so it resolves at once. The only way a `?` can fail other than a plain no-match is if the pattern itself is malformed — and that can only happen when interpolation builds an invalid regex at run time. That surfaces as a runtime error, not a no-match.

## A pure match is a statement, not an expression

A pure match is a statement, not an expression. You cannot use it as the right-hand side of something that expects a value — it is not allowed as the value of a `let` or as an overlay value. Write it on its own line.

That does not mean it has no value. Like every statement in Relux, a pure match evaluates to something: the text it matched. For a `?` match that is the whole match, `$0` — exactly what the shell `<?` operator returns. For a `=` match it is the whole value, which for an exact match is the same string. You will rarely reach for that value, because the point of a pure match is the assertion it makes and, for `?`, the captures it binds. But it is real, and it surfaces in one place: a pure match written as the *last* statement of a `fn` or `pure fn` body becomes that function's return value.

```relux
pure fn first_word(s) {
    s ? \w+
}
```

`first_word("hello there")` matches `\w+` against `hello there`, so `$0` — and the function's return value — is `hello`, the matched text, not the whole input. To return a specific capture instead of the whole match, end the function with that capture (a bare `$1`) rather than the match statement, so the capture is the last thing evaluated.

## Where pure matches run

Because a pure match needs no shell, it is valid in more places than a shell operator. You can write one inside:

- a `shell` block, in tests and effects;
- a regular `fn` body;
- a `pure fn` body;
- a test or effect [preamble](06-variables.md#scoping) — alongside `let`, before the first shell block.

The first three are straightforward — a pure match sits among your other statements and asserts as execution reaches it. The last one, the preamble, unlocks a genuinely useful pattern, and it pairs naturally with the pure functions you just learned. The next two sections show both.

## Destructuring in a preamble

An [effect](12-effects-and-dependencies.md)'s preamble — the `let` bindings before its first shell — can *assert* values as well as bind them. A pure match here runs before any shell spawns, so a malformed input is caught up front, at setup, instead of deep inside a shell command where the failure is harder to read.

The real payoff is destructuring. A `?` match in the preamble binds its captures into a frame that later preamble `let`s and a nested `start`'s overlay values can both read. That lets an effect take one packed configuration value and fan its pieces out to a dependency:

```relux
effect Service {
    expect DSN
    // Split the DSN into host, port, and database:
    DSN ? ^postgres://[^@]+@([^:]+):(\d+)/(\w+)$
    let db_host := $1
    start Db as Dep {
        HOST := db_host
        PORT := $2
        DB_NAME := $3
    }
    expose shell Dep.service as service
}
```

The pure match sits after `expect` and before `start`. It asserts the DSN's shape and, in the same step, pulls out the host, port, and database. `db_host` reads `$1` through a `let`; the `start` overlay reads `$2` and `$3` directly. If `DSN` does not have this shape, the test fails during setup, before `Db` is ever started.

One boundary is worth remembering: a shell block owns its own capture frame and does **not** inherit the preamble's. A bare `${1}` inside one of the effect's shells reads that shell's own captures, which start empty — not the preamble's. To carry a preamble capture into a shell, bind it to a `let` first, the way `db_host` does above, and read the `let` inside the shell.

## Extracting with a pure function

The `?` form makes pure functions good at extraction. Because a pure match is legal in a pure function body, a helper can assert an input's shape and return a piece of it:

```relux
pure fn extract_id(s) {
    s ? ^id=(\d+)$
    $1
}

test "extract an id" {
    let payload := "id=42"
    let id := extract_id(payload)   // id is "42"
    shell s {
        > echo "id=${id}"
        <? ^id=42$
    }
}
```

`extract_id("id=42")` matches the regex, binds `$1` to `42`, and returns it. If the argument does not have the `id=<digits>` shape — say `extract_id("nope")` — the pure match fails and the test fails right there, pointing at the call site.

The captures a pure function binds are **local to that call**. Each call starts with an empty frame, and when the function returns, its `$1` is discarded — it does not leak into the caller's captures. Keep the value you extracted by returning it and binding it with `let`, exactly as `let id := extract_id(payload)` does here. This is the extraction idiom: a pure function asserts a value's shape with `?` and returns a capture; the caller binds the result.

## What lands in the test log

Every pure match records a small trio of events into the structured log. A `pure-match-start` carries the value, the pattern, and whether it was a regex (`?`) or an exact match (`=`). It is followed by a `pure-match-done` on a hit — with the matched text and any captures — or a `pure-match-failed` on a miss. In the test log viewer, a failed pure match shows exactly where it ran (a shell block, a `fn` or `pure fn` body, or a preamble) alongside the value and the pattern that rejected it, so a failure that has no buffer to inspect still tells you everything you need.

## Best practices

### Keep `=`, `:=`, and `==` straight

The one-character gap between assert and bind is the easiest mistake to make with pure matching. You mean to check a value and you type `:=`, which silently reassigns it instead; or you reach for `==` out of habit from other languages and get a parse error.

Read every bare `=` as a question — "is this value exactly this?" — and every `:=` as a command — "make this value this." When you want to compare, `=`. When you want to store, `:=`. And remember there is no `==`: all values are strings, so equality is spelled with a single `=`.

### Use `=` when you mean exact equality

Once you are comfortable with `?`, it is tempting to use it everywhere and anchor it: `version ? ^3\.2\.1$`. It works, but it is harder to read and easier to get wrong — you have to escape the dots, remember both anchors, and a reader has to parse a regex to see that you are just checking for one specific string.

When you want an exact value, `=` says so plainly: `version = 3.2.1`. No escaping, no anchors, no ambiguity. Reserve `?` for when you genuinely need a pattern — a shape, a prefix, a capture — not for a value you already know in full.

### Keep comments off pattern lines

A pattern runs to the **end of the line**. That means a trailing comment does not end the pattern — it becomes part of it:

```relux
// Broken: the pattern is now "^ready$   // wait for readiness"
status ? ^ready$   // wait for readiness
```

The regex above can never match `ready`, because everything after `$`, including the `//` and the comment text, is still part of the pattern. Put the comment on its own line instead:

```relux
// Correct: the comment is a separate line
status ? ^ready$
```

This applies to every match operator — `<=`, `<?`, `=`, and `?` — because they all read their pattern to the end of the line. It is one of those mistakes that produces a baffling no-match until you spot it.

### Watch interpolated patterns

Like `<?`, a `?` pure match interpolates variables into its pattern before compiling the regex. If an interpolated value contains regex metacharacters — `.`, `*`, `+`, `(`, `[`, and so on — they are interpreted as regex syntax, not literal text. A variable holding `192.168.1.1` used as a pattern does not match that literal IP; each `.` matches any character. When the value comes from your own `let` and you know its contents, this is fine. When it comes from captured output or the environment, be aware that it may compile into something you did not intend — or fail to compile at all.

## Try it yourself

Write a test that destructures a database URL entirely with pure matching — no shell needed to parse it:

1. Bind a composed value with `let dsn := "postgres://app@db.local:5432/shop"`.
2. Use a single `?` pure match to capture the host, port, and database into `$1`, `$2`, and `$3`.
3. Assert with `=` that the host is exactly `db.local`.
4. Save the port to a named variable with `let port := $2`.
5. Open a shell, echo the saved port back, and match it with `<?` — proving the value survived across the shell boundary even though the shell's own captures start empty.

This combines regex captures, the exact-equality assertion, and the rule that a shell does not inherit preamble or prior captures — all the pieces from this article.

---

Next: [Cleanup](15-cleanup.md) — teardown blocks that run when a test or effect finishes
