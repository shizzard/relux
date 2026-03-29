# Send, Match, and Logs

[Previous: Getting Started](02-getting-started.md)

## When you need exact control over what gets sent

The send operator (`>`) appends a newline to everything you send — just like pressing Enter. Most of the time that is what you want: send a command, let the shell execute it. But sometimes you need to send text without that trailing newline. Maybe you are building up a command from parts, or feeding input to an interactive prompt that does not expect a newline.

Raw send (`=>`) sends text to the shell exactly as written — no newline appended, nothing added. The shell receives the bytes and waits for more. You can chain multiple raw sends to assemble a command piece by piece:

```relux
test "multiple raw sends" {
    shell s {
        => echo one
        => -two
        > -three
        <= one-two-three
    }
}
```

Three separate operations build a single command:

1. `=> echo one` sends `echo one` (no newline — the shell is still waiting)
2. `=> -two` sends `-two` (still no newline)
3. `> -three` sends `-three` followed by a newline

The shell now has `echo one-two-three\n` in its input buffer. It executes the command, and the literal match `<= one-two-three` picks up the output.

This example comes from `tests/relux/tests/operators/send.relux` in the Relux source tree (adapted from regex match to literal match for this article).

## Reading test output

Let's run the example and look at what Relux produces beyond the pass/fail result. Create a file `relux/tests/raw_send.relux` with the example from this article, then run it:

```
$ relux run relux/tests/raw_send.relux
running 1 tests
test raw_send.relux/multiple-raw-sends: |.... ok (5.8 ms)

test result: ok. 1 passed; 0 failed; finished in 5.8 ms
```

The line starting with `test` shows the test name, a **progress string** (`|....`), the result, and the duration. The progress string is a compact visual trace of what happened during execution:

- `|` — a shell was opened
- `.` — a send or successful match operation

So `|....` means: open shell, then four operations (our two `=>`s, `>`, and `<=`).

### The output directory

Every test run writes detailed logs to `relux/out/`. After running the test above, the directory looks like this (the `RnwTRJ4AMY` is the run id, and it would be different in your case):

```
relux/out/
├── latest -> run-2026-03-11-14-04-08-RnwTRJ4AMY
└── run-2026-03-11-14-04-08-RnwTRJ4AMY/
    ├── index.html
    ├── run_summary.toml
    └── logs/
        └── relux/tests/raw_send/
            └── multiple-raw-sends/
                ├── event.html
                ├── s.html
                ├── s.stdin.log
                ├── s.stdin.raw
                ├── s.stdout.log
                └── s.stdout.raw
```

Each run gets its own directory, named with a timestamp and a random ID. The `latest` symlink always points to the most recent run — so `relux/out/latest/index.html` is always the quickest way to the results.

Open `relux/out/latest/index.html` in a browser. The index page shows a summary table with one row per test: the test name, its result (pass/fail/skip), the duration, and the progress string. For a single passing test this is underwhelming, but when you have dozens of tests and one fails, the index is where you start — scan the results, click the failing test to jump to its event log.

Each test gets an `event.html` file that records every operation in a timeline: sends, matches, timeouts, shell switches. Each row shows a timestamp (relative to test start), the shell name, the event type, and the event data. Try clicking on the timestamp: it would bring you to the shell-specific event log, where you can only see events for this particular shell. Clicking on timestamp in the shell log would bring you back to the test log at that exact moment. It is very useful when you want to inspect what happened around that particular event in the shell.

For our passing test, the event log has four rows: the two raw sends (`echo one` and `-two`), the send of `-three` (with newline), and the successful match of `one-two-three`. Since the test only spawns one shell, the shell event log will have almost the same.

Alongside the event log, each shell produces four log files:

- **`s.stdin.log`** — every command sent to the shell, with timestamps
- **`s.stdout.log`** — everything the shell printed back, with timestamps
- **`s.stdin.raw`** / **`s.stdout.raw`** — the same data but as raw bytes, without timestamps

The `.log` files are the ones you'll read most often. Here is what `s.stdout.log` looks like for our test:

```
[+0.003s] export PS1='relux> ' PS2='' PROMPT_COMMAND=''
[+0.008s] relux> echo one-two-three
[+0.009s] one-two-three
[+0.009s] relux>
```

The first line is Relux configuring the shell prompt. Then the shell echoes the command, prints the output, and shows the prompt again. The timestamps let you see exactly when each piece of output arrived.

## Error logs

Let's go back to the simplest possible test — the one we started with:

```relux
test "echo and match" {
    shell s {
        > echo hello-relux
        <= hello-relux
    }
}
```

This sends `echo hello-relux` and matches the output `hello-relux`. Now break this test: duplicate the second match. Since first match would consume the "hello-relux" string, we will get a timeout:

```relux
test "echo and match" {
    shell s {
        > echo hello-relux
        <= hello-relux
        <= hello-relux
    }
}
```

Run it:

```
$ relux run relux/tests/hello.relux
running 1 tests
test hello.relux/echo-and-match: |... ok (9.7 ms)

test result: ok. 1 passed; 0 failed; finished in 9.7 ms
```

Wait, what? That is definitely a bug, this should not have worked — where did the second `hello-relux` come from? Let's read the event log for this test (`relux/out/latest/logs/relux/tests/hello/echo-and-match/event.html`) and look at the two match rows.

The first match did not hit the output line `hello-relux` — it hit the echoed **command** `echo hello-relux`, which contains the substring `hello-relux`. The second match then found the actual output.

The shell echoes every command you send before printing its result. That echo is part of the output buffer, and `<=` matches anywhere in it. We have been matching our own commands this whole time!

This is not a bug — it is how PTY shells work. But it changes how you think about matching, and it is exactly what the next article is about.

---

Next: [The Output Buffer](04-the-output-buffer.md) — understand the buffer and cursor model that makes matching predictable
