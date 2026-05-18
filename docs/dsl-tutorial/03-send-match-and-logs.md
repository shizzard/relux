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

```text
$ relux run -f relux/tests/raw_send.relux
running 1 tests
test raw_send.relux/multiple-raw-sends: |.... ok (5.8 ms)

test result: ok. 1 passed; 0 failed; finished in 5.8 ms
```

While the test is in flight, you'll briefly see a **progress string** (`|....`) next to the test name — a compact visual trace of what is happening:

- `|` — a shell was opened
- `.` — a send or successful match operation

So `|....` means: open shell, then four operations (our two `=>`s, `>`, and `<=`). The default progress display refreshes this line live in your terminal, so by the time the test finishes the glyphs may have already scrolled out of view. The example block above is a frozen snapshot of what you'd see mid-flight; the `--progress` flag changes how progress is rendered, and a later article on the CLI walks through its modes.

### The output directory

Every test run writes detailed logs to `relux/out/`. After running the test above, the directory looks like this (the `RnwTRJ4AMY` is the run id, and it would be different in your case):

```text
relux/out/
├── latest -> run-2026-03-11-14-04-08-RnwTRJ4AMY
└── run-2026-03-11-14-04-08-RnwTRJ4AMY/
    ├── index.html
    ├── run_summary.toml
    ├── artifacts/
    └── logs/relux/tests/raw_send/multiple-raw-sends/
        ├── event.html
        ├── events.json
        └── artifacts/
```

Each run gets its own directory, named with a timestamp and a random ID. The `latest` symlink always points to the most recent run — so `relux/out/latest/index.html` is always the quickest way to the results.

Open `relux/out/latest/index.html` in a browser. The index page shows a summary table with one row per test: the test name, its result (pass/fail/skip), and the duration. For a single passing test this is underwhelming, but when you have dozens of tests and one fails, the index is where you start — scan the results, click the failing test to drill into its test log viewer.

### The test log viewer

Each test gets an `event.html` file: a self-contained page you can open from anywhere, no server required.

- **Top:** a timeline strip — a compact visual summary of all events. Click anywhere on it to jump.
- **Left:** an events list — one row per operation in the test: sends, matches, shell opens, plus richer event kinds as your tests grow.
- **Right:** a detail panel — selecting an event in the list shows the shell's output at that moment and the source line that produced it.
- **Top bar:** a few side panels you'll grow into as later articles introduce more concepts.

Next to `event.html` you'll also find `events.json` — the same data in a machine-readable form, useful when you want to feed test runs into your own tooling.

The viewer needs a fairly modern browser — Chrome / Edge 80+, Firefox 113+, or Safari 16.4+. Older browsers see a one-line fallback message instead of the report; open `events.json` directly in that case.

As you work through the rest of this tutorial, each article will introduce the test log viewer functionality relevant to its topic. For a full catalog of regions, panels, and hotkeys, see the [Test Log Viewer reference](../reference/05-test-log-viewer.html). For now, open the test log viewer for your test and click around — there's no pressure to understand everything yet.

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

```text
$ relux run -f relux/tests/hello.relux
running 1 tests
test hello.relux/echo-and-match: |... ok (9.7 ms)

test result: ok. 1 passed; 0 failed; finished in 9.7 ms
```

Wait, what? That is definitely a bug, this should not have worked — where did the second `hello-relux` come from? Let's open the test log viewer for this test (`relux/out/latest/logs/relux/tests/hello/echo-and-match/event.html`) and look at the two match rows in its events list.

The first match did not hit the output line `hello-relux` — it hit the echoed **command** `echo hello-relux`, which contains the substring `hello-relux`. The second match then found the actual output.

The shell echoes every command you send before printing its result. That echo is part of the output buffer, and `<=` matches anywhere in it. We have been matching our own commands this whole time!

This is not a bug — it is how PTY shells work. But it changes how you think about matching, and it is exactly what the next article is about.

---

Next: [The Output Buffer](04-the-output-buffer.md) — understand the buffer and cursor model that makes matching predictable
