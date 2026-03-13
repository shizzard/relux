# Tutorial: The Relux DSL

> **Format:** GitHub-Flavored Markdown, compatible with GitHub rendering, docs.rs, and crates.io. No HTML, no custom directives.

## Overview

This tutorial series is a comprehensive, hands-on guide to writing integration tests with the Relux DSL. Relux is an Expect-style test framework for interactive shell programs — it spawns PTY shells, sends input, and matches output against patterns with timeouts.

The series takes the reader from zero to productive: by the end, they will be able to write, organize, and run multi-shell integration test suites with shared effects, reusable functions, and CI-ready reporting.

Each article builds strictly on the previous ones. No concept is used before it is introduced.

## Prerequisites

The reader must already be comfortable with:

- **Shell basics** — what a shell is, how commands produce output, the read-eval-print loop
- **Regular expressions** — character classes, quantifiers, anchors, capture groups
- **General testing concepts** — what a test is, pass/fail, setup/teardown

No prior experience with Expect, lux, or Relux is assumed.

## Articles

### 0. Introduction
**File:** `00-introduction.md`
**Scope:** What Relux is, the problem it solves, and how Expect-style testing works. Introduces the core mental model: "send input to a shell, match output against patterns." Shows a complete minimal test without explaining every detail — the reader sees the destination before the journey.
**After reading:** The reader understands why Relux exists, what kind of tests it's for, and has a high-level picture of what a `.relux` file looks like.

### 1. Installation
**File:** `01-installation.md`
**Scope:** Building Relux from source. Short stub — will be expanded when distribution options are available.
**After reading:** The reader has a working `relux` binary.

### 2. Getting Started
**File:** `02-getting-started.md`
**Scope:** Scaffolding a project with `relux new`, understanding the project layout (`Relux.toml`, `relux/tests/`, `relux/lib/`), configuring the shell and prompt, shell blocks (single and multiple, switching between them), writing and running the first real test.
**After reading:** The reader has a working project, understands shell blocks, and can run a simple test.

### 3. Send, Match, and Logs
**File:** `03-send-match-and-logs.md`
**Scope:** The three fundamental operators: send (`>`), raw send (`=>`), and literal match (`<=`). The interaction loop of sending a command and checking the output. Raw send as a way to send input without a trailing newline. What happens when a match fails — the terminal output, rich HTML test reports, and shell logs. How to read and navigate logs to diagnose failures.
**After reading:** The reader can send commands (with and without trailing newlines), match output literally, and debug failing tests using logs and reports.

### 4. The Output Buffer
**File:** `04-the-output-buffer.md`
**Scope:** The buffer and cursor model — how shell output accumulates, how the cursor advances on each match, what it means for subsequent matches.
**After reading:** The reader understands the mental model behind matching and can predict match behavior.

### 5. Built-in Functions
**File:** `05-built-in-functions.md`
**Scope:** The echo problem (matching your own command instead of its output), `match_prompt()` and `match_ok()` as the solution. Function call syntax. Complete tour of all built-in functions organized by category: string operations, random generation, system utilities, shell interaction, control characters.
**After reading:** The reader knows function call syntax and the full BIF toolkit.

### 6. Variables
**File:** `06-variables.md`
**Scope:** Declaring variables with `let`, string interpolation with `${var}`, variable reassignment, the `$$` escape for literal dollar signs. Scoping basics: test-scoped variables shared across shell blocks.
**After reading:** The reader can store, transform, and reuse values across a test.

### 7. Regex Matching
**File:** `07-regex-matching.md`
**Scope:** Regex match operator (`<?`), capture groups (`${0}`, `${1}`, ...), capturing regex results into variables for later use.
**After reading:** The reader can match output with regex patterns and extract captured values.

### 8. Functions
**File:** `08-functions.md`
**Scope:** Defining functions with `fn`, parameters, arity-based dispatch (same name, different parameter counts). The critical mental model: functions execute in the caller's shell context. Scope behavior: parameters are local, side effects persist. Functions as the primary tool for organizing and reusing test logic.
**After reading:** The reader can extract reusable test logic into functions.

### 9. Timeouts
**File:** `09-timeouts.md`
**Scope:** Default timeout from `Relux.toml`, tolerance (`~`) vs assertion (`@`) timeouts, the `--timeout-multiplier` flag, shell-scoped `~` and `@` operators, inline `<~` and `<@` overrides for one-shot timeout on a single match, test-level timeout (`test "name" ~30s` or `test "name" @3s`), how timeout scoping works across function calls.
**After reading:** The reader can control timing at every level of granularity and choose the right timeout kind for the intent.

### 10. Fail Patterns
**File:** `10-fail-patterns.md`
**Scope:** Fail pattern operators (`!?` for regex, `!=` for literal). Fail patterns are checked inline during match operations and at statement boundaries. One active pattern per shell, persistence across function calls, clearing fail patterns. Setting a fail pattern immediately rescans the buffer.
**After reading:** The reader can set up continuous error monitoring that catches problems anywhere in the test.

### 12. Effects and Dependencies
**File:** `12-effects-and-dependencies.md`
**Scope:** Effect definitions (`effect ... -> shell`), the `need` keyword for declaring dependencies, overlay variables (`{ KEY = "value" }`), effect identity and deduplication, dependency graph and topological execution order. Cleanup blocks for both effects and tests.
**After reading:** The reader can create reusable test infrastructure with effects and proper cleanup.

### 13. Condition Markers
**File:** `13-condition-markers.md`
**Scope:** `[skip]`, `[run]`, `[flaky]` markers. Conditional forms: `if`/`unless` with truthiness checks, equality comparisons, regex matching. Multiple markers (AND semantics). Environment-only variable lookup.
**After reading:** The reader can conditionally skip or run tests based on environment.

### 14. Modules and Imports
**File:** `14-modules-and-imports.md`
**Scope:** The module system: every `.relux` file is a module, paths mirror filesystem from project root. Selective imports, wildcard imports, aliases. Resolution rules. Organizing a project with shared effects and functions.
**After reading:** The reader can organize a multi-file test suite.

### 15. The CLI
**File:** `15-the-cli.md`
**Scope:** Complete coverage of the `relux` CLI: `run` (filtering, strategies, `--rerun`, TAP/JUnit), `check`, `dump`, `new`, `history`.
**After reading:** The reader can use every CLI feature effectively.

### 16. Patterns and Recipes
**File:** `16-patterns-and-recipes.md`
**Scope:** Practical cookbook: waiting for services, testing exit codes, temporary resources, multi-shell coordination, timeout strategies, capture pipelines, environment-based configuration.
**After reading:** The reader has proven patterns to adapt to their own test suites.
