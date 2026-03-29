# Relux — Rust Integration Test Framework

## What is this?

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) (LUcid eXpect scripting) — an Expect-style test automation framework. It controls interactive shell programs by sending input and matching output against expected patterns (regex or literal). The goal is a fast, standalone binary with no Erlang dependency.

## Project Status

Working prototype — tests can be authored and executed end-to-end.

## Relux DSL (`.relux` files)

See [Semantics](docs/semantics.md) and [Syntax](docs/syntax.md)

### DSL Tutorial

A hands-on guide to every Relux language feature, from first test to condition markers.

0. [Introduction](docs/dsl-tutorial/00-introduction.md) — what Relux is, the problem it solves, and the core mental model of Expect-style testing
1. [Installation](docs/dsl-tutorial/01-installation.md) — building Relux from source
2. [Getting Started](docs/dsl-tutorial/02-getting-started.md) — scaffolding a project, configuring the shell, and running the first test
3. [Send, Match, and Logs](docs/dsl-tutorial/03-send-match-and-logs.md) — the fundamental operators for sending input, matching output, and debugging failures
4. [The Output Buffer](docs/dsl-tutorial/04-the-output-buffer.md) — how shell output accumulates and the cursor model behind matching
5. [Built-in Functions](docs/dsl-tutorial/05-built-in-functions.md) — the echo problem, `match_prompt()`, and the full built-in function toolkit
6. [Variables](docs/dsl-tutorial/06-variables.md) — declaring variables, string interpolation, reassignment, and scoping
7. [Regex Matching](docs/dsl-tutorial/07-regex-matching.md) — matching output with regex patterns and extracting capture groups
8. [Functions](docs/dsl-tutorial/08-functions.md) — defining reusable test logic with parameters and arity-based dispatch
9. [Timeouts](docs/dsl-tutorial/09-timeouts.md) — controlling timing at every level with tolerance and assertion timeouts
10. [Fail Patterns](docs/dsl-tutorial/10-fail-patterns.md) — continuous error monitoring that catches problems anywhere in the test
11. [Effects and Dependencies](docs/dsl-tutorial/11-effects-and-dependencies.md) — reusable test infrastructure with dependency graphs and overlay variables
12. [Pure Functions](docs/dsl-tutorial/12-pure-functions.md) — functions that compute values without touching a shell
13. [Cleanup](docs/dsl-tutorial/13-cleanup.md) — teardown blocks for removing files, collecting artifacts, and undoing side effects
14. [Modules and Imports](docs/dsl-tutorial/14-modules-and-imports.md) — organizing a multi-file test suite with shared effects and functions
15. [Condition Markers](docs/dsl-tutorial/15-condition-markers.md) — conditionally skipping or running tests based on environment
16. [The CLI](docs/dsl-tutorial/16-the-cli.md) — complete coverage of `relux new`, `check`, `run`, and `history`

A1. [Best Practices](docs/dsl-tutorial/A1-best-practices.md) — all best-practices guidelines from the series in one place

### Suite Tutorial

Building a real integration test suite for a three-service stack. Applies DSL features together to solve practical problems: duplication, shared infrastructure, and parallel execution. Includes a [complete working project](docs/suite-tutorial/project/).

0. [Project Setup](docs/suite-tutorial/00-project-setup.md) — scaffolding the project and understanding the services
1. [Testing the Database](docs/suite-tutorial/01-testing-the-database.md) — first tests, isolation, fail patterns, and helper functions
2. [Extracting a Library](docs/suite-tutorial/02-extracting-a-library.md) — moving shared code to `lib/`, imports, and pure functions
3. [Effects and Dependencies](docs/suite-tutorial/03-effects-and-dependencies.md) — declarative infrastructure with effect chains and seeded data
4. [Shared Dependencies](docs/suite-tutorial/04-shared-dependencies.md) — the task service, diamond dependencies, and effect deduplication
5. [Parallel Execution](docs/suite-tutorial/05-parallel-execution.md) — dynamic ports, overlays, and CI readiness

## Planned Features

- Per-shell command override: per-shell executable override via shell block attributes (global shell command now configurable in `Relux.toml`)
- Custom scaffold templates: user-defined templates for `relux new --test` and `relux new --effect` via `Relux.toml`, replacing the built-in defaults
- Effect-level timeouts: allow effects to declare a default timeout (similar to test-level `~timeout`) that applies to all match operations within the effect's shell blocks

## Planned RFCs

- Interactive debugger: step through test scripts interactively with breakpoints
- Multiple marker semantics: define AND/OR combination semantics when multiple condition markers are stacked on a single test or effect

## Known Bugs

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
