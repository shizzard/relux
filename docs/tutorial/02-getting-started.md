# Getting Started

[Previous: Installation](01-installation.md)

## A working example

Let's go from an empty directory to a passing test. Let's create a new project and add an integration test suite for it.

```bash
mkdir my-project && cd my-project
relux new
```

Now scaffold a test:

```bash
relux new --test hello
```

And run it:

```bash
relux run
```

You should see something like this:
```
test result: ok. 1 passed; 0 failed; finished in 132.7ms
```

That's all it takes: three commands and you have a working test suite. Let's unpack what happened.

## Scaffolding a project

Unlike tools such as `cargo new` or `npm init` that create a new project directory, `relux new` works **in the current directory**. This is a deliberate choice — Relux is designed to add integration tests to an existing project, not to create a project from scratch.

Running `relux new` creates exactly two things at the project root:

```
my-project/
├── Relux.toml
└── relux/
    ├── .gitignore
    ├── tests/
    └── lib/
```

- **`Relux.toml`** — the project manifest. Configures the shell, prompt, and timeouts.
- **`relux/`** — everything Relux-related lives here: tests, library modules, and test output.
  - **`tests/`** — your test files go here (`.relux` files).
  - **`lib/`** — shared modules: reusable functions and effects. Don't pay too much attention here, we'll get to this later.
  - **`.gitignore`** — ignores `out/`, the directory where Relux writes test run artifacts.

Two entities at the root — `Relux.toml` and `relux/`.

## Relux.toml

The generated `Relux.toml` looks like this:

```toml
# name = "my-test-suite"

# [shell]
# command = "/bin/sh"
# prompt = "relux> "

# [timeout]
# match = "5s"
# test = "5m"
# suite = "30m"
```

Everything is commented out. The values shown are the defaults — Relux uses them when a field is not explicitly set. Let's walk through each section.

**`name`** — an optional label for the test suite. If omitted, Relux uses the project directory name. In the example above, that would be 'my-project'.

**`[shell]`** — configures the shell that Relux spawns for each shell it starts:

- `command` — the shell binary to run. Defaults to `/bin/sh`.
- `prompt` — the prompt string Relux configures in each spawned shell. Defaults to `relux> `.

**`[timeout]`** — controls how long Relux waits before declaring failure:

- `match` — the default timeout for each match operation. If the expected output does not appear within this duration, the test fails. Defaults to `5s`.
- `test` — optional maximum duration for a single test. No limit by default.
- `suite` — optional maximum duration for the entire test run. No limit by default.

All timeout values use [humantime](https://docs.rs/humantime/latest/humantime/) format: `100ms`, `5s`, `1m30s`, `30m`, etc.

For now, you can leave everything at the defaults, or change the values and see what happens.

## Scaffolding test modules

You already saw `relux new --test hello` in the opening example. This command creates a test file at `relux/tests/hello.relux` with a starter test you can run immediately.

The path you provide maps directly to the filesystem under `relux/tests/`. You can use subdirectories to organize your tests:

```bash
relux new --test auth/login
```

This creates `relux/tests/auth/login.relux` (and the `auth/` directory if it doesn't exist).

Path rules:

- Must be **snake_case** — lowercase letters, digits, and underscores only.
- Each segment must start with a letter or underscore.
- The `.relux` extension is added automatically — you don't need to include it.

There is also `relux new --effect`, which scaffolds a module in `relux/lib/` instead. Effects are shared test infrastructure, we'll cover them in detail later.

## Writing a test

Let's look at what `relux new --test hello` generated:

```relux
test hello {
    shell myshell {
        > echo hello-relux
        <= hello-relux
    }
}
```

This test does three things:

1. **`test hello`** — declares a test with a descriptive name.
2. **`shell myshell { ... }`** — opens a shell block named `myshell`. Relux spawns a new `/bin/sh` process for this shell.
3. Inside the shell block:
   - **`> echo hello-relux`** — sends the command `echo hello-relux` to the shell, followed by a newline (just like pressing Enter).
   - **`<= hello-relux`** — matches the output literally. Relux waits (up to the match timeout) for the string `hello-relux` to appear in the shell's output. If it appears, the match succeeds and the test continues. If it doesn't appear before the timeout, the test fails.

That's the fundamental interaction loop: **send** a command, **match** the expected output.

## Shells

A test can use more than one shell. Let's add a second one:

```relux
test hello {
    shell myshell {
        > echo hello-relux
        <= hello-relux
    }

    shell anothershell {
        > echo hello-user
        <= hello-user
    }
}
```

This is like opening two terminal windows side by side. Relux enters the `myshell` window first, sends a command, and checks the output. Then it opens a new window called `anothershell` and does the same there. Each shell is an independent process — its own environment, its own working directory, its own output.

### Switching between shells

Now consider a more realistic pattern: you want to start something in one shell and verify its effect in another. To do that, you interleave shell blocks:

```relux
test hello {
    shell myshell {
        > echo hello-relux
    }

    shell anothershell {
        > echo hello-user
    }

    shell myshell {
        <= hello-relux
    }

    shell anothershell {
        <= hello-user
    }
}
```

Here, both shells appear twice. The first time, Relux opens a new "terminal window" and sends the command. The second time, Relux **switches back** to the same "terminal window" — the process is still running, the output is still there — and matches the result.

This pattern — send in one shell, do something in another, come back to check — is the foundation of multiprocess integration testing. You'll use it whenever you test interactions between a client and a server, a producer and a consumer, or any two processes that need to coordinate.

## Running tests

You have two commands for working with tests:

**`relux check`** validates your test files without executing them. It runs the lexer, parser, and resolver — catching syntax errors, unresolved names, and invalid imports — but never spawns a shell. This is fast and useful as a quick sanity check, especially before committing.

**`relux run`** actually executes the tests:

```bash
relux run
```

```
test result: ok. 1 passed; 0 failed; finished in 12.5ms
```

You can also run a specific test file:

```bash
relux run relux/tests/hello.relux
```

Or a directory of tests:

```bash
relux run relux/tests/auth/
```

## Best practices

### Keep `/bin/sh` as the default shell

You might be tempted to configure your favorite shell — `zsh`, `fish`, `bash` — as the default in `Relux.toml`. After all, you use it every day and know its features well.

Resist the temptation. A custom shell means every developer on the team and every CI machine needs that shell installed and configured. `/bin/sh` is available everywhere, and the operations you need in integration tests — running commands, checking output, setting environment variables — work the same across POSIX shells. The interactive niceties of fancier shells (tab completion, syntax highlighting, advanced globbing) don't matter when Relux is driving the terminal.

Only switch away from `/bin/sh` if your system under test genuinely requires a specific shell to function.

### Leave timeouts at their defaults

The default match timeout of 5 seconds is generous for most commands. You might think "I'll set timeout to 500 ms to speed up failure detection". Don't — not yet.

Timeout tuning is one of those things that should be driven by actual pain, not preemptive optimization. Tight timeouts cause flaky tests on slower machines or under CI load. The defaults are deliberately conservative. When you encounter a specific situation where the default is genuinely wrong — a command that reliably takes 30 seconds — that's the time to tune. Relux provides fine-grained timeout control at the operator, shell, and test level, which you'll learn about in later articles.

### The shell prompt must be static

The prompt configured in `Relux.toml` (default: `relux> `) must be a fixed, unchanging string. Do not include dynamic elements like timestamps, git branch names, hostnames, or user-specific paths.

Why this matters will become clear in later articles, but the short version is: Relux uses the prompt as a reliable marker in the shell output stream. A prompt that changes between commands — or between machines — makes that marker unpredictable, which leads to flaky or outright broken tests. The default `relux> ` is a good choice: short, distinctive, and the same everywhere.

## Try it yourself

Open `relux/tests/hello.relux` and experiment:

1. Change the `echo` command to print something different. Update the match to expect the new output. Run the test — does it pass?
2. Change only the match string so it no longer matches the output. Run the test and observe the failure — what does Relux tell you?
3. Try matching a substring. For example, if you send `echo hello-relux`, try matching just `hello`. Does that work with `<=`?
4. Add a second `>` and `<=` pair below the first. Send a different command (ping something?) and match its output.

The goal is to get comfortable with the edit-run-observe loop. Every test you'll write in the rest of this series is built from this same foundation: send, match, repeat.

---

Next: [Send, Match, and Logs](03-send-match-and-logs.md) — a deeper look at the fundamental operators and how to debug failures
