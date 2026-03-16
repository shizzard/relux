# Relux — Rust Integration Test Framework

## What is this?

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) (LUcid eXpect scripting) — an Expect-style test automation framework. It controls interactive shell programs by sending input and matching output against expected patterns (regex or literal). The goal is a fast, standalone binary with no Erlang dependency.

## Project Status

Working prototype — tests can be authored and executed end-to-end.

## Relux DSL (`.relux` files)

See [docs/semantics.md] and [docs/syntax.md]

A hands-on guide to writing integration tests with Relux, from first test to full test suite.

0. [Introduction](docs/tutorial/00-introduction.md) — what Relux is, the problem it solves, and the core mental model of Expect-style testing
1. [Installation](docs/tutorial/01-installation.md) — building Relux from source
2. [Getting Started](docs/tutorial/02-getting-started.md) — scaffolding a project, configuring the shell, and running the first test
3. [Send, Match, and Logs](docs/tutorial/03-send-match-and-logs.md) — the fundamental operators for sending input, matching output, and debugging failures
4. [The Output Buffer](docs/tutorial/04-the-output-buffer.md) — how shell output accumulates and the cursor model behind matching
5. [Built-in Functions](docs/tutorial/05-built-in-functions.md) — the echo problem, `match_prompt()`, and the full built-in function toolkit
6. [Variables](docs/tutorial/06-variables.md) — declaring variables, string interpolation, reassignment, and scoping
7. [Regex Matching](docs/tutorial/07-regex-matching.md) — matching output with regex patterns and extracting capture groups
8. [Functions](docs/tutorial/08-functions.md) — defining reusable test logic with parameters and arity-based dispatch
9. [Timeouts](docs/tutorial/09-timeouts.md) — controlling timing at every level with tolerance and assertion timeouts
10. [Fail Patterns](docs/tutorial/10-fail-patterns.md) — continuous error monitoring that catches problems anywhere in the test
11. [Effects and Dependencies](docs/tutorial/11-effects-and-dependencies.md) — reusable test infrastructure with dependency graphs and overlay variables
12. [Pure Functions](docs/tutorial/12-pure-functions.md) — functions that compute values without touching a shell
13. [Cleanup](docs/tutorial/13-cleanup.md) — teardown blocks for removing files, collecting artifacts, and undoing side effects
14. [Modules and Imports](docs/tutorial/14-modules-and-imports.md) — organizing a multi-file test suite with shared effects and functions
15. [Condition Markers](docs/tutorial/15-condition-markers.md) — conditionally skipping or running tests based on environment
16. [The CLI](docs/tutorial/16-the-cli.md) — complete coverage of `relux new`, `check`, `run`, and `history`
A1. Patterns and Recipes — practical cookbook for common testing scenarios

## Planned Features

- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults
- Make `sleep`, `log`, and `annotate` impure BIFs: these have observable side effects (time delay, output) and don't belong in pure context — move them from `lookup_pure` to `lookup` so they require a shell context

## Planned RFCs

- Interactive debugger: step through test scripts interactively with breakpoints
- Multiple marker semantics: define AND/OR combination semantics when multiple condition markers are stacked on a single test or effect

## Known Bugs

- Cleanup ordering: all test and effect shells should be terminated before any cleanup block runs. Currently cleanup runs interleaved with shell termination. The correct order is: terminate all test shells, terminate all effect shells, then run cleanup blocks (test cleanup first, then effect cleanup in reverse topological order).
- Cleanup variable scope: test-level and effect-level `let` variables should be available in their respective cleanup blocks. Currently cleanup blocks get a fresh empty scope and can only see overlay variables (for effects) and environment variables.

## Tech Stack

- **Language:** Rust (edition 2024)
- **Lexer:** `logos` (~0.16)
- **Parser:** `chumsky` (~0.12)

## Architecture

1. **Lexer** (logos): tokenizes `.relux` files
2. **Parser** (chumsky): tokens → AST
3. **Resolver**: AST → IR, module imports, effect dependency graph
4. **Runtime**: walks IR, spawns PTY shells via tokio, executes send/match with timeouts
5. **Reporter**: test results with ariadne-powered diagnostic output

## Conventions

- File extension: `.relux`
- Examples go in `examples/`
- Follow Rust 2024 edition idioms
