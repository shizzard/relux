# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) — an Expect-style integration test framework for interactive shell programs. It sends input to PTY shells and matches output against regex/literal patterns with timeouts.

## Commands

### Build

```bash
just build                      # Build binary in debug mode
just release                    # Build binary in release mode
just intellij                   # Build IntelliJ plugin
just books                      # Build tutorial/reference mdbooks
```

### Test

```bash
just test                       # Run all tests (unit + e2e)
just unit                       # Run all unit tests
just unit lexer                 # Run tests matching "lexer"
just e2e                        # Run e2e tests (check then run)
just e2e <path from cwd>        # Run a particular e2e test(s)
just history                    # Analyze e2e run history
```

### Fix

```bash
just check                      # Run cargo check + clippy + fmt check
just fix                        # Fix clippy warnings and format code
```

### Clean

```bash
just clean                      # Remove build artifacts
just clean-logs                 # Remove e2e test output logs
```

## Architecture

Classic compiler pipeline: **Lexer → Parser → Resolver → Runtime → Reporter**

### Shared Types (`src/lib.rs`)

`Span` and `Spanned<T>` — byte-offset source spans used across all pipeline stages. Span arithmetic is encapsulated with private fields.

### Core (`src/core/`)

- **config.rs**: Parses `Relux.toml` at project root. Shell command/prompt, timeout defaults (match/test/suite), flaky retry config. Constants for directory layout (`relux/`, `tests/`, `lib/`, `out/`).
- **error.rs**: Shared error types.
- **table.rs**: `SharedTable<K,V>` — thread-safe write-once table backed by `elsa::FrozenMap`. `FileId` for cross-file source tracking.

### Diagnostics (`src/diagnostics/`)

`IrSpan`, `ModulePath`, `EffectName`, `CauseTable`, `CauseId`, `WarningId` — typed diagnostic infrastructure for cross-file error reporting with Ariadne-powered annotations.

### DSL Pipeline (`src/dsl/`)

- **Lexer** (`dsl/lexer/`): Logos-based tokenizer. `Token` enum with keyword/operator/literal variants. `mod.rs` handles multi-mode lexing and `normalize()` for whitespace normalization.
- **Parser** (`dsl/parser/`): Chumsky combinator parser. Split into focused modules: `ast.rs` (AST types), `module.rs` (top-level), `fn_def.rs`, `effect.rs`, `test_def.rs`, `stmt.rs`, `expr.rs`, `operator.rs`, `interpolation.rs`, `overlay.rs`, `block.rs`, `import.rs`, `need.rs`, `ident.rs`, `prefix.rs`, `timeout.rs`, `annotation.rs`, `punctuation.rs`, `ws.rs`, `token.rs`.
- **Resolver** (`dsl/resolver/`): Converts AST → IR. `discover.rs` finds test modules, `loader.rs` loads sources, `lower.rs` lowers AST to IR, `shallow_env.rs` handles environment pre-resolution. IR is defined in `resolver/ir/` with separate files for `plan.rs`, `test_def.rs`, `effect.rs`, `func.rs`, `stmt.rs`, `expr.rs`, `block.rs`, `interpolation.rs`, `ident.rs`, `timeout.rs`, `marker.rs`, `comment.rs`, `regex_validate.rs`, `tables.rs`.

### Pure (`src/pure/`)

Side-effect-free evaluation layer, shared between resolver (compile-time let evaluation) and runtime.

- **mod.rs**: `VarScope`, `Env`, `LayeredEnv` — variable scoping with layered environment chain (own → parent → grandparent). `LayeredEnv` uses `Arc`-sharing, no cloning of base env.
- **bifs.rs**: Pure built-in functions: `trim`, `upper`, `lower`, `replace`, `split`, `len`, `uuid`, `rand`, `available_port`, `which`, `default`.
- **evaluator.rs**: `eval_pure_expr()` — infallible pure expression evaluator (all failure modes caught at lowering time).

### Runtime (`src/runtime/`)

- **mod.rs**: Suite executor. `execute()` runs tests with N workers (tokio tasks pulling from shared queue), cancellation support (fail-fast, suite timeout), flaky retry loop with timeout multiplier. `RunContext` holds per-run config. `EffectManager` handles effect lifecycle (instantiate → cleanup).
- **runtime_context.rs**: `RuntimeContext` — per-test context with event sink, shell config, log dir, tables, env, cancellation token.
- **vm/** (`runtime/vm/`):
  - **mod.rs**: Per-shell virtual machine. PTY child process, send/match operations with timeouts, fail pattern checking, statement execution, function calls.
  - **bifs.rs**: Impure built-in functions (`Bif` trait): `sleep`, `annotate`, `log`, `match_prompt`, `match_exit_code`, `match_ok`, `match_not_ok`, `ctrl_c`, `ctrl_d`, `ctrl_z`, `ctrl_l`, `ctrl_backslash`.
  - **buffer.rs**: Output buffer with cursor for matching operations.
  - **context.rs**: `ExecutionContext` — per-shell state: `Scope` (test/effect/function), `ShellState`, variable frames.
  - **pty.rs**: PTY process management.
- **effect/** (`runtime/effect/`):
  - **mod.rs**: `EffectManager` — instantiates effects, manages cleanup (reverse order), warning collection.
  - **registry.rs**: `EffectRegistry` — deduplicates effect instances by `(name, args, overlay)` key.
- **observe/** (`runtime/observe/`):
  - **event_sink.rs**: `EventSink` — structured event collection for test execution.
  - **event_log.rs**: `BufferSnapshot`, event log types.
  - **progress.rs**: Progress channel for live updates.
  - **tui.rs**: Terminal UI renderer (live progress with cursor control, auto-detects TTY).
  - **shell_log.rs**: Shell I/O logging for debugging.
- **report/** (`runtime/report/`):
  - **result.rs**: `TestResult`, `Outcome` (Pass/Fail/Skipped/Invalid), `Failure` variants (Runtime/Cancelled).
  - **html.rs**: Rich HTML test report generation.
  - **junit.rs**: JUnit XML report output.
  - **tap.rs**: TAP (Test Anything Protocol) output.
  - **run_summary.rs**: `RunSummary` — serializable run results for history analysis.

### History (`src/history/`)

Analyzes test run history across multiple runs: flaky detection, failure modes, first-fail identification, duration trends. Supports human-readable and TOML output formats.

### Binary (`bin/relux.rs`)

Unified CLI with subcommands: `new`, `run`, `check`, `dump`, `history`. Uses clap for arg parsing.

### Editor Support (`editors/`)

- **IntelliJ** (`editors/intellij/`): Syntax highlighting plugin for `.relux` files. Build with `just intellij`.
- **VS Code** (`editors/vscode/`): VS Code extension for `.relux` files.

## Key Design Decisions

- **All values are strings** — no type system beyond that
- **Pure vs impure split** — pure functions (string ops, random) evaluate at compile time in `let` bindings; impure functions (shell interaction) require a VM context
- **Effects are CamelCase, functions are snake_case** — enforced at parse level, used to disambiguate in imports
- **Effect identity** = `(name, expects)` — same tuple means deduplicated/reused instance
- **Functions execute in caller's shell context** — no own shell, side effects (timeout/fail-pattern changes) persist
- **Cleanup blocks** run in fresh implicit shells with uncancellable tokens; any statement valid in a shell block is valid in a cleanup block
- **Module imports** resolve from project root, never relative to importing file
- **LayeredEnv** — environment variables use Arc-shared layered chain (no cloning of base env)
- **Concurrent test execution** — N worker tasks pull from a shared queue; results sorted back to original order

## Conventions

- Rust 2024 edition idioms
- Unit tests are colocated in each module via `#[cfg(test)] mod tests`
- Documentation as mdbooks in `docs/` — `reference/` (semantics, syntax, BIFs, CI), `dsl-tutorial/`, `suite-tutorial/`
- **Every code change must be accompanied by updates to the relevant documentation** — review `docs/reference/` (semantics, syntax, BIFs, CI), `docs/dsl-tutorial/`, and `docs/suite-tutorial/` and update any articles affected by the change

## RFCs

- RFCs for new features go in `rfc/` as `RXXX-short-name.md`
- `rfc/README.md` is the index — it must stay consistent with the actual RFC files
- Each RFC is committed and submitted as a PR:
  - PR title: `RFC R009: Variable Match Operator` (format: `RFC RXXX: Capitalized RFC Name`)
  - PR body: the full RFC contents
