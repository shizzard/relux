# Introduction

## What is Relux?

Relux is an integration test framework for interactive shell programs. It takes a different approach from most end-to-end test frameworks: instead of writing imperative scripts that call APIs and poll for state changes, you describe the expected conversation between you and the system under test — what to send and what to expect back.

This style of testing is called **Expect-style testing**, named after the original [Expect](https://en.wikipedia.org/wiki/Expect) tool from 1990. The core idea is simple:

1. **Send** input to a running program
2. **Expect** (match) specific output
3. **React** to what you matched — send more input, capture values, branch

Relux is inspired by [hawk/lux](https://github.com/hawk/lux) (LUcid eXpect scripting) — an Erlang-based framework that showed Expect-style testing could be principled and composable, not just fragile scripts. Relux builds on that foundation with a block-structured DSL, an effect system for declarative dependency management, and a single standalone Rust binary with no runtime dependencies.

## Who this tutorial is for

Anybody who found himself testing a real-world system might be a Relux user.

This tutorial assumes you are comfortable with:

- **Shell basics** — what a shell is, how commands produce output, the read-eval-print loop
- **Regular expressions** — character classes, quantifiers, anchors, capture groups
- **General testing concepts** — what a test is, pass/fail, setup/teardown

No prior experience with Expect, lux, or Relux is assumed. The tutorial introduces every concept from scratch, one article at a time.

## When to use Relux

Relux is designed for integration testing of systems with real dependencies. In most cases, you have a single system under test — a CLI, a service, a REPL — and its dependencies: databases, queue servers, other backend services. Instead of mocking those dependencies away, Relux lets you start them for real, so errors in both the system under test and its dependencies are not hidden behind mocks.

Most end-to-end test frameworks approach this with imperative code: start a process, sleep or poll until it's ready, make HTTP calls, parse responses, tear down in a `finally` block. The test logic gets buried under orchestration boilerplate — process management, retry loops, health check polling, cleanup handlers.

Relux takes a declarative approach. You describe the startup order and dependencies between services as **effects**, and Relux resolves them into a dependency graph, starts them in the right order, and tears them down in reverse — even when things fail.

Relux is not, however, a general-purpose test framework. It does not replace unit test frameworks. It complements them by covering the system-level integration layer — the part where real processes start, interact, and produce observable output. It can also serve as an API testing tool (sending requests and matching responses through shell commands), though that is not its primary purpose.

Relux is not a screen scraper or a GUI testing tool either. It works at the level of text I/O through a PTY — it has no knowledge of screen layout, windows, or graphical elements.

## The mental model

Imagine you are developing and testing a service. You open a few terminal windows: in one you start the database, in another you launch a message queue, in a third you run your service. Then you open yet another terminal and fire off a few requests — HTTP, gRPC, or just raw packets. After each request you glance at the logs: any errors? You check whether your service called its dependency correctly. You switch to the dependency terminals to make sure they didn't error out either.

Relux does exactly what you do — but automated. Instead of doing it by hand, you write it down once, and Relux handles all the heavy lifting: starting processes, waiting for readiness, switching between shells, checking output. You are left with the thing that matters: the actual testing.

A Relux test reads like a transcript of that interaction:

```relux
test "echo and match" {
    shell s {
        > echo hello-relux
        <= hello-relux

        > echo "value=42"
        <= value=42
    }
}
```

The structure mirrors a conversation with a shell: send a command (`>`), match the response (`<=`), repeat. Every match operation has a **timeout** — if the expected output does not appear in time, the test fails. This is how Relux detects hangs, unexpected prompts, and wrong output.

Don't worry about the syntax details yet — the following articles will introduce every element step by step.

## The DSL at a glance

The example above uses just two operators (`>` and `<=`), but the Relux DSL has more to offer. Here is a glimpse of what you will learn in the following articles:

- **Regex matching** (`<?`) — match output with regular expressions and capture groups
- **Variables** (`let`, `${var}`) — capture and reuse values
- **Timeouts** (`~5s`, `<~2s?`) — control how long to wait for output
- **Negative matching** (`<!?`, `<!=`) — assert that output does *not* appear
- **Fail patterns** (`!?`, `!=`) — continuous background monitoring for errors
- **Functions** (`fn`) — extract reusable test logic
- **Effects** (`effect`, `need`) — shared setup/teardown infrastructure
- **Multiple shells** — test client/server interactions, concurrent processes
- **Modules and imports** — organize tests across files
- **Condition markers** (`[skip]`, `[run if ...]`) — conditional test execution

Each article in this series introduces one concept, building on everything before it.

---

Next: [Installation](01-installation.md) — get Relux built and ready to use
