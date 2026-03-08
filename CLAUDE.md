# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) — an Expect-style integration test framework for interactive shell programs. It sends input to PTY shells and matches output against regex/literal patterns with timeouts.

## Build & Test Commands

```bash
just build                      # Build in debug mode
just release                    # Build in release mode
just test                       # Run all tests (unit + e2e)
just unit                       # Run all ~187 unit tests
just unit lexer                 # Run tests matching "lexer"
just unit parser                # Run tests matching "parser"
just unit resolver              # Run tests matching "resolver"
just e2e                        # Run e2e tests (check then run)
just e2e <path from tests root> # Run e2e particular test (check then run)
just check                      # Run cargo check
just intellij                   # Build IntelliJ plugin
just clean                      # Remove build artifacts
just clean-logs                 # Remove e2e test output logs
```

## Architecture

Classic compiler pipeline: **Lexer → Parser → Resolver → Runtime → Reporter**

### DSL Pipeline (`src/dsl/`)

- **Lexer** (`dsl/lexer/`): Logos-based multi-mode tokenizer. `tokens.rs` defines `Token` and `Fragment` types. `mod.rs` handles submorphing for variable interpolation within operator payloads.
- **Parser** (`dsl/parser/`): Chumsky combinator parser. `ast.rs` defines the AST (`Module`, `Item`, `FnDef`, `EffectDef`, `TestDef`, `Stmt`, `Expr`). `mod.rs` is the grammar.
- **Resolver** (`dsl/resolver/`): Converts AST → IR. Handles module loading (`FsSourceLoader`), name resolution, import validation, circular dependency detection, effect graph construction via `daggy` DAG. `ir.rs` defines `Plan`, `EffectGraph`, `SourceMap`.
- **Report** (`dsl/report.rs`): Ariadne-powered diagnostic output with source annotations.

### Runtime (`src/runtime/`)

- **mod.rs**: Main executor. `Runtime::execute(plans) -> Vec<TestResult>`. `CodeServer` indexes functions by `(name, arity)`. `RunContext` manages per-test state, effect graph execution (topological order), cleanup (reverse order).
- **vm.rs**: Per-shell virtual machine. Manages PTY child process, output buffer with cursor, send/match operations with timeouts, fail pattern checking.
- **bifs.rs**: 24 built-in functions (`Bif` trait). String ops, control chars (`ctrl_c`, `ctrl_d`), `match_prompt()`, `sleep()`, `log()`, `uuid()`, etc.
- **vars.rs**: Variable scoping via `ScopeStack`. All values are strings. `interpolate()` handles `${var}` and `${1}` (regex captures).
- **html.rs**: Rich HTML test report generation.
- **result.rs**: Test result types (`TestResult`, outcome enums).
- **event_log.rs**: Structured event logging for test execution.
- **progress.rs**: Progress reporting during test runs.
- **shell_log.rs**: Shell I/O logging for debugging.

### Binary (`bin/relux.rs`)

Unified CLI with subcommands: `new`, `run`, `check`, `dump`. Uses clap for arg parsing.

### Configuration (`src/config.rs`)

Parses `Relux.toml` at project root: shell command/prompt, timeout defaults (match/case/suite).

### Editor Support (`editors/`)

- **IntelliJ** (`editors/intellij/`): Syntax highlighting plugin for `.relux` files. Build with `just intellij`.

## Key Design Decisions

- **All values are strings** — no type system beyond that
- **Effects are CamelCase, functions are snake_case** — enforced at parse level, used to disambiguate in imports
- **Effect identity** = `(name, args, overlay)` — same tuple means deduplicated/reused instance
- **Functions execute in caller's shell context** — no own shell, side effects (timeout/fail-pattern changes) persist
- **Cleanup blocks** run in fresh implicit shells, only send/let allowed (no match operators)
- **Module imports** resolve from project root, never relative to importing file

## Conventions

- Rust 2024 edition idioms
- Unit tests are colocated in each module via `#[cfg(test)] mod tests`
- Language semantics documented in `docs/semantics.md`, syntax in `docs/syntax.md`
- RFCs for new features go in `docs/rfc/`
