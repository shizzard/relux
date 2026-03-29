# The Output Buffer

[Previous: Send, Match, and Logs](03-send-match-and-logs.md)

## A working example

The previous article ended with a discovery: the shell echoes every command you send, and `<=` matches anywhere in the buffer — including the echo. Here is a test that demonstrates how to account for that, consuming every piece of output a command produces:

```relux
test "full consumption" {
    shell s {
        > echo hello
        <= echo hello
        <= hello
        <= relux>
    }
}
```

Four operations, each doing exactly one thing:

1. `> echo hello` — send the command.
2. `<= echo hello` — match the echoed command.
3. `<= hello` — match the actual output.
4. `<= relux>` — match the prompt that appears after the command finishes.

After this sequence, the buffer is empty — every byte of the previous command's lifecycle has been consumed. The shell is ready for the next command, and the buffer is in a known state.

This is the pattern you will use more than any other in Relux: **send, match the result, match the prompt.** It leaves the buffer clean before the next command.

## The output buffer

When Relux spawns a shell, it starts collecting everything the shell prints into a byte buffer — the **output buffer**. Every character goes in: the prompt setup command, its output, the first prompt, every command echo, every line of output, every subsequent prompt.

When a match operation succeeds, everything up to and including the matched text is consumed — removed from the buffer. Future match operations only see what remains. The matched text is gone; it will never be found again.

The position where the next match starts searching is called the **cursor**. When a shell is first created, Relux configures the prompt and matches it internally, consuming the setup output. Your test begins with a clean buffer — the cursor sits right at the start, ready for the first command's echo. Each successful match advances the cursor past the matched text. Everything before the cursor has been consumed and is invisible to future matches.

### The cursor in action

Let's trace the buffer and cursor through the working example. For clarity, the buffer content stays the same in these diagrams — only the cursor moves. After `> echo hello`, the buffer contains (simplified for clarity):

```
echo hello<newline>hello<newline>relux>
^cursor
```

`<= echo hello` scans forward from the cursor and matches the echoed command. The cursor advances past the match:

```
echo hello<newline>hello<newline>relux>
          ^cursor
```

`<= hello` scans forward and finds the actual output. The cursor advances:

```
echo hello<newline>hello<newline>relux>
                        ^cursor
```

`<= relux>` scans forward and finds the prompt. The cursor advances past it:

```
echo hello<newline>hello<newline>relux>
                                      ^cursor
```

The buffer is fully consumed. The next command's echo will be the first thing the cursor sees.

### What the cursor skips

When the match operator scans forward from the cursor, it does not care what sits between the cursor and the matching text. There may be prompts, blank lines, ANSI escape sequences, output from other commands — the scan skips over all of it, looking only for the pattern.

This means `<=` is a **substring search**, not an exact match. It finds the first occurrence of the pattern anywhere in the remaining buffer. If the pattern is short or generic, it might match something you did not intend — a prompt fragment, a piece of the echoed command, leftover output from a previous step. The cursor then lands in an unexpected place, and everything after it is wrong.

### The prompt as your anchor

This is why the [Getting Started](02-getting-started.md) article insisted on a static shell prompt. The prompt string `relux>` appears in the buffer after every command finishes. It is a reliable, predictable boundary marker.

When you match the prompt after matching a command's output, you are telling Relux: "I am done with this command. Consume everything up to and including the prompt so the next operation starts at a clean boundary."

Without that prompt match, the cursor sits somewhere after `hello` in the output — but before the prompt. The next match would have to scan past the prompt to find anything, and if the pattern happens to match part of the prompt itself, or the stale output, you are in trouble.

The output buffer is the single most important concept of the Relux DSL and runtime. When writing tests, you must always keep track of where the cursor is. Matching the prompt after each command is the simplest way to stay in control.

### Buffer reset

Sometimes you genuinely do not care about the output — a command prints a long startup banner, or verbose logging that is irrelevant to the test. For these cases, Relux provides a **buffer reset**: a `<=` operator with no pattern. It consumes everything currently in the buffer. It is the equivalent of saying "I don't care what happened, skip to now."

We will see in the best practices section below why this operator should be used with caution.

## Best practices

### Always match the prompt

If you are done examining a command's output, match the prompt. Every time. This is the single most effective habit for avoiding flaky tests.

Without a prompt match, the cursor floats somewhere in the middle of the output. When the buffer contains output from a previous command that was not fully consumed, any pattern that appears in that leftover output will match there first. The cursor advances to an unexpected position, and subsequent matches silently verify stale data.

Matching the prompt anchors the cursor at a command boundary. It is cheap, it is predictable, and it eliminates an entire class of timing-related failures.

In the next article we will introduce `match_prompt()` — a built-in function that does exactly this in a single call, so you do not have to type `<= relux>` every time and hard-code the prompt string in your tests.

### Check the exit code

A command can produce the expected output and still fail. Or it can fail silently, producing no output at all, while the match picks up something else entirely. Checking the exit code after a command catches these problems early:

```relux
test "verify success" {
    shell s {
        > mkdir -p /tmp/test-dir
        <= relux>
        > echo ==$?==
        <= ==0==
        <= relux>
    }
}
```

The `echo ==$?==` / `<= ==0==` pair is a cheap way to verify the previous command succeeded. The `==` delimiters are distinctive enough to avoid accidental substring matches. Without this check, a failing `mkdir` would go unnoticed — the test would continue with a missing directory and fail later with a confusing, unrelated error.

### Buffer reset does not respect causality

The buffer reset operator (`<=` with no pattern) consumes everything currently in the buffer. That "currently" depends on timing — how much output the shell has printed by the instant Relux executes the reset.

If output is still arriving — a command is running, a log line is being flushed — the cursor might land in the middle of a line, or before a line that is about to appear. This creates a race condition: the test might pass on your machine and fail in CI, or pass nine times and fail on the tenth.

In almost every case, there is a better anchor than a buffer reset. Match the prompt. Match a specific log line. Match any known text that marks the boundary you actually care about. These anchors are causal — they mean "this specific thing happened" — rather than temporal — "this is where the buffer happened to be at this moment."

Only use buffer reset when you are certain all relevant output has already arrived and there is no meaningful boundary to match against.

## Try it yourself

Here is a simple test — two echo commands, each followed by a match:

```relux
test "double echo" {
    shell s {
        > echo hello
        <= hello

        > echo hello
        <= hello
    }
}
```

Run this test in your head. For each of the four operations, trace the buffer contents and the cursor position. How many commands would actually be executed in the shell? Write down your predictions.

Then run the test with `relux run` and open the event log at `relux/out/latest/index.html`. Compare the event log against your predictions.

Now think about what the right way to write this test would be.

---

Next: [Built-in Functions](05-built-in-functions.md) — meet `match_prompt()` and the full toolkit of built-in helpers
