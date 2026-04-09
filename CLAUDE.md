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
just run <args>                  # Run relux with arguments
just e2e                        # Run e2e tests (check then run)
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

Cargo workspace with 8 crates under `crates/`. Classic compiler pipeline: **Lexer → Parser → Resolver → Runtime → Reporter**

### `relux-core` (`crates/relux-core/`)

Foundation types shared across all crates.

- **lib.rs**: `Span` and `Spanned<T>` — byte-offset source spans. Span arithmetic is encapsulated with private fields.
- **config.rs**: Parses `Relux.toml` at project root. Shell command/prompt, timeout defaults (match/test/suite), flaky retry config. Constants for directory layout (`relux/`, `tests/`, `lib/`, `out/`).
- **error.rs**: `DiagnosticReport`, `DiagnosticReports` — Ariadne-powered error rendering.
- **table.rs**: `SharedTable<K,V>` — thread-safe write-once table backed by `elsa::FrozenMap`. `FileId`, `SourceFile`, `SourceTable`.
- **diagnostics.rs**: `IrSpan`, `ModulePath`, `EffectName`, `CauseTable`, `CauseId`, `WarningId` — typed diagnostic infrastructure for cross-file error reporting.
- **pure/mod.rs**: `VarScope`, `Env`, `LayeredEnv` — variable scoping with layered environment chain (own → parent → grandparent). `LayeredEnv` uses `Arc`-sharing, no cloning of base env.
- **pure/bifs.rs**: Pure built-in functions: `trim`, `upper`, `lower`, `replace`, `split`, `len`, `uuid`, `rand`, `available_port`, `which`, `default`.
- **discover.rs**: `discover_relux_files()` — recursive `.relux` file discovery, stops at nested project boundaries.

### `relux-ast` (`crates/relux-ast/`)

AST type definitions: `AstModule`, `AstItem`, `AstTestDef`, `AstEffectDef`, `AstFnDef`, `AstStmt`, `AstExpr`, etc. Depends only on `relux-core` for `Span`/`Spanned`.

### `relux-lexer` (`crates/relux-lexer/`)

Logos-based tokenizer. `Token` enum with keyword/operator/literal variants. Multi-mode lexing and `normalize()` for whitespace normalization.

### `relux-parser` (`crates/relux-parser/`)

Chumsky combinator parser. Split into focused modules: `module.rs` (top-level), `fn_def.rs`, `effect.rs`, `test_def.rs`, `stmt.rs`, `expr.rs`, `operator.rs`, `interpolation.rs`, `overlay.rs`, `block.rs`, `import.rs`, `need.rs`, `ident.rs`, `prefix.rs`, `timeout.rs`, `annotation.rs`, `punctuation.rs`, `ws.rs`, `token.rs`, `error.rs`.

### `relux-ir` (`crates/relux-ir/`)

IR types and AST→IR lowering.

- **Type definitions**: `plan.rs`, `test_def.rs`, `effect.rs`, `func.rs`, `stmt.rs`, `expr.rs`, `block.rs`, `interpolation.rs`, `ident.rs`, `timeout.rs`, `comment.rs`, `tables.rs`. Each file defines IR types and their `IrNodeLowering` impl.
- **lowering_context.rs**: `LoweringContext` — orchestrates caching, cycle detection, scope stacks, BIF registration, import resolution, and diagnostic collection during AST→IR lowering.
- **lowering_trait.rs**: `IrNodeLowering` trait — cached, cycle-detecting AST→IR conversion.
- **evaluator.rs**: `eval_pure_expr()` — infallible pure expression evaluator (all failure modes caught at lowering time).
- **shallow_env.rs**: `ShallowLayeredEnv` — name-only layered env for resolve-time `expect` satisfiability checks.
- **marker.rs**: Marker/annotation evaluation (`@skip`, `@flaky`, conditional markers).
- **regex_validate.rs**: Compile-time regex validation for match patterns.

### `relux-resolver` (`crates/relux-resolver/`)

Resolver orchestration: module discovery, source loading, and the `resolve()` entry point.

- **lib.rs**: `resolve()` public API, `SourceLoader` trait, `FsSourceLoader`.
- **discover.rs**: `discover_test_modules()` — converts discovered `.relux` files to `ModulePath`s.
- **loader.rs**: `load_modules()` — BFS worklist that loads, parses, and enqueues transitive imports. `InMemoryLoader` for tests.
- **lower.rs**: Re-exports `LoweringContext`/`LoweringScope` from `relux-ir`. Contains shared test helpers.

### `relux-runtime` (`crates/relux-runtime/`)

- **lib.rs**: Suite executor. `execute()` runs tests with N workers (tokio tasks pulling from shared queue), cancellation support (fail-fast, suite timeout), flaky retry loop with timeout multiplier. `RunContext` holds per-run config. `EffectManager` handles effect lifecycle (instantiate → cleanup).
- **runtime_context.rs**: `RuntimeContext` — per-test context with event sink, shell config, log dir, tables, env, cancellation token.
- **vm/**:
  - **mod.rs**: Per-shell virtual machine. PTY child process, send/match operations with timeouts, fail pattern checking, statement execution, function calls.
  - **bifs.rs**: Impure built-in functions (`Bif` trait): `sleep`, `annotate`, `log`, `match_prompt`, `match_exit_code`, `match_ok`, `match_not_ok`, `ctrl_c`, `ctrl_d`, `ctrl_z`, `ctrl_l`, `ctrl_backslash`.
  - **buffer.rs**: Output buffer with cursor for matching operations.
  - **context.rs**: `ExecutionContext` — per-shell state: `Scope` (test/effect/function), `ShellState`, variable frames.
  - **pty.rs**: PTY process management.
- **effect/**:
  - **mod.rs**: `EffectManager` — instantiates effects, manages cleanup (reverse order), warning collection.
  - **registry.rs**: `EffectRegistry` — deduplicates effect instances by `(name, args, overlay)` key.
- **observe/**:
  - **event_sink.rs**: `EventSink` — structured event collection for test execution.
  - **event_log.rs**: `BufferSnapshot`, event log types.
  - **progress.rs**: Progress channel for live updates.
  - **tui.rs**: Terminal UI renderer (live progress with cursor control, auto-detects TTY).
  - **shell_log.rs**: Shell I/O logging for debugging.
- **report/**:
  - **result.rs**: `TestResult`, `Outcome` (Pass/Fail/Skipped/Invalid), `Failure` variants (Runtime/Cancelled).
  - **html.rs**: Rich HTML test report generation.
  - **junit.rs**: JUnit XML report output.
  - **tap.rs**: TAP (Test Anything Protocol) output.
  - **run_summary.rs**: `RunSummary` — serializable run results for history analysis.

### `relux` (CLI, `crates/relux-cli/`)

Published crate (`cargo install relux`). CLI subcommands and the `relux` binary.

- **lib.rs**: `cli()` command definition, shared helpers (`resolve_project`, `resolve_test_paths`, `read_file`, `build_source_loader`, `ModuleKind`).
- **run.rs**, **check.rs**, **new.rs**, **dump.rs**: One subcommand handler each.
- **completions.rs**: Shell completion installer (`--shell`, `--install`, `--path`).
- **completer.rs**: `ArgValueCompleter` functions for `.relux` files, manifests, timeouts, shells.
- **history/**: Analyzes test run history across multiple runs: flaky detection, failure modes, first-fail identification, duration trends. Supports human-readable and TOML output formats.
- **bin/relux.rs**: Thin dispatch layer — delegates to the library.

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
- Unit tests are colocated in each module via `#[cfg(test)] mod tests`; IR lowering tests are integration tests in `crates/relux-resolver/tests/`
- Documentation as mdbooks in `docs/` — `reference/` (semantics, syntax, BIFs, CI), `dsl-tutorial/`, `suite-tutorial/`
- **Every code change must be accompanied by updates to the relevant documentation** — review `docs/reference/` (semantics, syntax, BIFs, CI), `docs/dsl-tutorial/`, and `docs/suite-tutorial/` and update any articles affected by the change
- **PRs are squash-merged** — the final squash commit message must be a single conventional commit (type, optional scope, description, and body)

## RFCs

- RFCs for new features go in `rfc/` as `RXXX-short-name.md`
- `rfc/README.md` is the index — it must stay consistent with the actual RFC files
- Each RFC is committed and submitted as a PR:
  - PR title: `RFC R009: Variable Match Operator` (format: `RFC RXXX: Capitalized RFC Name`)
  - PR body: the full RFC contents
