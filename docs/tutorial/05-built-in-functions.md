# Built-in Functions

[Previous: The Output Buffer](04-the-output-buffer.md)

## Functions

If you have used any programming language before, functions in Relux will feel familiar. A function is a named operation that you call by writing its name followed by parentheses. Some functions take arguments — values you pass inside the parentheses, separated by commas. Some take no arguments at all.

```relux
match_prompt()
match_exit_code(0)
```

The first line calls `match_prompt` with no arguments. The second calls `match_exit_code` with one argument: `0`.

Relux ships with a set of **built-in functions** (BIFs) — functions provided by the runtime that you can use in any test without importing or declaring anything. This article covers the ones you need most often. The remaining built-in functions — string operations, random generation, and system utilities — will be introduced in later articles alongside the language features they complement.

You can also define your own functions, which a later article will cover. For now, all the functions you will see are built-in.

## Arity

In Relux, a function is identified by its **name and its number of arguments**. The number of arguments a function accepts is called its **arity**.

This means two functions can share the same name as long as they take different numbers of arguments. Relux treats them as separate functions. You will see this with `match_not_ok` shortly: `match_not_ok()` (arity 0) and `match_not_ok(exit_code)` (arity 1) are two distinct functions that do related but different things.

When the article refers to a specific function, it uses the notation `name/arity` — for example, `match_not_ok/0` and `match_not_ok/1`. This is just a convention for documentation; you do not write it this way in your tests.

## The match functions

The previous article established that matching the prompt after each command is the most important habit for writing reliable tests. It also showed a manual way to check the exit code. The match functions automate both of these patterns.

There are five match functions. We will build them up from the simplest to the most convenient, showing what each one does in terms of the operators you already know.

**`match_prompt()`** matches the shell prompt — the string configured in `Relux.toml` (default: `relux> `). It is equivalent to:

```relux
<= relux>
```

That is all it does: a literal match for the prompt string. The advantage over writing `<= relux>` by hand is that `match_prompt()` always uses the prompt from your project configuration. If you change the prompt in `Relux.toml`, every `match_prompt()` call picks up the new value automatically. No find-and-replace across your test files.

Here is the test from the previous article, rewritten with `match_prompt()`:

```relux
test "full consumption" {
    shell s {
        > echo hello
        <= echo hello
        <= hello
        match_prompt()
    }
}
```

The behavior is identical to matching `<= relux>` — the cursor advances past the prompt, leaving the buffer clean for the next command.

**`match_exit_code(code)`** verifies the exit code of the most recently executed command. It is equivalent to:

```relux
> echo ::$?::
<= ::0::
<= relux>
```

(Where `0` is whatever value you passed as the argument.)

It sends `echo ::$?::` to the shell — `$?` is the POSIX variable that holds the exit code of the last command. The `::` delimiters are there to prevent accidental substring matches. Then it matches the expected code and the prompt.

Notice that `match_exit_code` does **not** match the prompt before sending. It assumes the buffer has already been consumed up to the prompt — either by a previous `match_prompt()` call or by a manual `<= relux>`. If you call `match_exit_code` with unconsumed output still in the buffer, the cursor will scan past all of it to find `::code::`. The function will succeed, but you will have skipped over output without examining it — the same problem as a buffer reset.

A typical usage pattern:

```relux
test "match_exit_code with zero" {
    shell s {
        > true
        match_exit_code(0)
    }
}

test "match_exit_code with 127 for missing command" {
    shell s {
        > relux_nonexistent_command_42
        match_exit_code(127)
    }
}
```

The first verifies that `true` exits with code 0. The second verifies that a nonexistent command exits with 127 — the standard "command not found" code.

Why skip `match_prompt()` before checking the exit code? Because `match_exit_code` is a building block. The higher-level functions below combine prompt matching and exit code checking into a single call.

**`match_ok()`** is the idiomatic way to assert that a command succeeded. It combines the two functions above:

```relux
match_prompt()
match_exit_code(0)
```

That is it: match the prompt (consuming the command's output and leaving the buffer clean), then verify the exit code is zero. One function call replaces two, and it reads naturally: "match that the command was OK."

Here is an example:

```relux
test "shell retains state after switching away" {
    shell a {
        > export MY_MARKER=from_a
        match_ok()
    }

    shell b {
        > echo "in shell b"
        <= in shell b
        match_ok()
    }

    shell a {
        > echo $MY_MARKER
        <= from_a
        match_ok()
    }
}
```

The `export` command produces no interesting output — you just need to know it succeeded. `match_ok()` handles that in one call: consume whatever output there was, verify exit code 0, leave the buffer clean for the next shell block. The other two shell blocks first match a specific piece of output, then use `match_ok()` to consume the rest and verify success.

**`match_not_ok()`** is the opposite of `match_ok()`: it asserts that the previous command **failed** — that its exit code is anything other than zero. Like `match_ok`, it matches the prompt first:

```relux
<= relux>
> echo ::$?::
# verify the exit code is not ::0::
<= relux>
```

Use it when you expect a command to fail but don't care about the specific exit code:

```relux
test "match_not_ok after failing command" {
    shell s {
        > false
        match_not_ok()
    }
}

test "match_not_ok after command-not-found" {
    shell s {
        > relux_nonexistent_command_42
        match_not_ok()
    }
}
```

The first test uses `false`, which always exits with code 1. The second uses a nonexistent command (exit code 127). In both cases, `match_not_ok()` passes because the exit code is not zero.

**`match_not_ok(exit_code)`** is the arity-1 variant. It asserts that the command failed with a **specific** non-zero exit code. It matches the prompt first, then verifies that the exit code equals the given value — and that the value is not zero:

```relux
<= relux>
> echo ::$?::
# verify the exit code equals the argument AND is not ::0::
<= relux>
```

This is stricter than `match_not_ok/0`. If the command exits with a different non-zero code, the test fails. If the command succeeds (exit code 0), the test also fails — even if you passed `0` as the argument.

Use it when the specific failure mode matters:

```relux
test "command not found gives 127" {
    shell s {
        > relux_nonexistent_command_42
        match_not_ok(127)
    }
}
```

Here is a summary of all five match functions:

| Function | Matches prompt first? | Then checks |
|---|---|---|
| `match_prompt()` | Yes (that's all it does) | — |
| `match_exit_code(code)` | **No** | Exit code equals `code` |
| `match_ok()` | Yes | Exit code is 0 |
| `match_not_ok()` | Yes | Exit code is not 0 |
| `match_not_ok(code)` | Yes | Exit code equals `code` and is not 0 |

## Control character functions

The match functions deal with text — matching output and checking exit codes. But sometimes you need to send a keystroke that is not a printable character — interrupting a running process with Ctrl+C, closing a pipe with Ctrl+D, or suspending a job with Ctrl+Z. The control character functions send these signals to the shell:

| Function | Key | Signal / Effect |
|---|---|---|
| `ctrl_c()` | Ctrl+C | Sends SIGINT — interrupts the running foreground process |
| `ctrl_d()` | Ctrl+D | Sends EOF — signals end of input, closing stdin |
| `ctrl_z()` | Ctrl+Z | Sends SIGTSTP — suspends the foreground process |
| `ctrl_l()` | Ctrl+L | Sends form feed — typically clears the terminal screen |
| `ctrl_backslash()` | Ctrl+\\ | Sends SIGQUIT — forcefully terminates the process |

These functions take no arguments and send a single control byte to the shell.

Here is an example that interrupts a long-running command:

```relux
test "ctrl_c interrupts a running command" {
    shell s {
        > sleep 60
        ctrl_c()
        match_prompt()
    }
}
```

The test sends `sleep 60` — a command that would run for a minute. Then `ctrl_c()` interrupts it, just like pressing Ctrl+C in a terminal. Finally, `match_prompt()` verifies the shell returned to the prompt, confirming the interrupt worked and the shell is ready for the next command.

Another common pattern is using `ctrl_d()` to close stdin on an interactive program:

```relux
test "ctrl_d sends eof to interactive program" {
    shell s {
        > cat
        > hello
        ctrl_d()
        <= hello
        match_ok()
    }
}
```

This starts `cat`, which reads from stdin and echoes back. The test sends `hello`, then closes stdin with `ctrl_d()`. The `cat` process exits, and the match picks up the echoed output. `match_ok()` verifies `cat` exited cleanly and consumes the prompt.

And `ctrl_z()` to suspend a process:

```relux
test "ctrl_z suspends a process" {
    shell s {
        > sleep 60
        ctrl_z()
        match_prompt()
        > kill %%
        match_ok()
    }
}
```

The `sleep 60` command is suspended by `ctrl_z()`, returning control to the shell. Then `kill %%` terminates the suspended job (the `%%` is shell syntax for "the most recent background job"). `match_ok()` confirms the kill succeeded.

## Logging and annotation

Beyond interacting with the shell, Relux provides two built-in functions that help you leave breadcrumbs in your test output: `log` and `annotate`. Both take a single string argument.

**`log(message)`** writes a message to the test's event log — the same log you see in the HTML report at `relux/out/latest/`. It appears as a log event row in the event timeline, timestamped alongside sends and matches. This is useful for marking phases of a complex test, recording diagnostic information, or leaving notes for whoever reads the report after a failure.

```relux
log("about to start the server")
```

**`annotate(text)`** adds a label to the progress output — the compact `|....` string you see in terminal output during a test run. Annotations appear inline as named markers, making it easier to see where a test is spending its time when watching a run in real time.

```relux
annotate("setup complete")
```

For example, a test with two annotations might produce progress output like this:

```
test my_test.relux/server-startup: |...[setup complete]....[server ready].. ok (2.1s)
```

The annotation text appears between the dots, marking the point in the test where it was called.

The distinction between the two is where the output goes: `log` writes to the persistent HTML report, `annotate` writes to the live terminal progress line.

---

Next: [Variables](06-variables.md) — store, transform, and reuse values with `let` and `${var}`
