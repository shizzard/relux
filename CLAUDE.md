# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Relux is a Rust reimplementation of [hawk/lux](https://github.com/hawk/lux) — an Expect-style integration test framework for interactive shell programs. It sends input to PTY shells and matches output against regex/literal patterns with timeouts.

Each test run produces a **structured event log** (canonical artifact, `events.json`) and a **self-contained HTML viewer** (`event.html`) per test. The viewer is the primary post-mortem debugger: a Svelte SPA that replays the timeline, scopes variables, inspects shell buffers, and highlights source — opened directly via `file://`, no server required.

## Commands

### Build

```bash
just build                      # Build binary in debug mode
just release                    # Build binary in release mode
just intellij                   # Build IntelliJ plugin
just vscode                     # Build VS Code extension (.vsix)
just viewer-build               # Rebuild vendored Svelte viewer bundle (vendor/relux-viewer.js.gz)
just books                      # Build tutorial/reference mdbooks
just install-hooks              # Point git at .githooks/ (runs check/clippy/fmt/books on commit)
```

### Test

```bash
just test                       # Run all tests (unit + e2e)
just unit                       # Run all Rust unit tests
just unit lexer                 # Run Rust tests matching "lexer"
just viewer-test                # Run viewer Vitest suite (in docker)
just viewer-check               # Run svelte-check on viewer (in docker)
just run <args>                 # Run relux with arguments
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

Cargo workspace with 8 crates under `crates/`, plus a sibling Svelte project under `viewer/` and vendored bundles under `vendor/`. Classic compiler pipeline feeding a per-test structured log: **Lexer → Parser → Resolver → Runtime (records `StructuredLog`) → Reporter (emits `events.json` + `event.html`)**

### `relux-core` (`crates/relux-core/`)

Foundation types shared across all crates.

- **lib.rs**: `Span` and `Spanned<T>` — byte-offset source spans. Span arithmetic is encapsulated with private fields.
- **config.rs**: Parses `Relux.toml` at project root. Shell command/prompt, timeout defaults (match/test/suite), flaky retry config. Constants for directory layout (`relux/`, `tests/`, `lib/`, `out/`).
- **error.rs**: `DiagnosticReport`, `DiagnosticReports` — Ariadne-powered error rendering.
- **table.rs**: `SharedTable<K,V>` — thread-safe write-once table backed by `elsa::FrozenMap`. `FileId`, `SourceFile`, `SourceTable`. `SourceFile::line_at()` resolves byte offsets to (line, col) via cached line-offset tables.
- **diagnostics.rs**: `IrSpan`, `ModulePath`, `EffectName`, `CauseTable`, `CauseId`, `WarningId`, `DefinitionRef`, `FnId`, `EffectId` — typed diagnostic infrastructure for cross-file error reporting.
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
- **evaluator.rs**: `eval_pure_expr()` — infallible pure expression evaluator (all failure modes caught at lowering time). Emits structured events into a `PureEvalSink` so the runtime can record interpolations and pure-fn calls.
- **pure_sink.rs**: `PureEvalSink` trait + `MatchKind` — the interface the IR evaluator uses to record pure-evaluation events. The runtime implements this against the `StructuredLogBuilder`; a null impl is used when no log is being captured.
- **shallow_env.rs**: `ShallowLayeredEnv` — name-only layered env for resolve-time `expect` satisfiability checks.
- **marker.rs**: Marker/annotation evaluation (`@skip`, `@flaky`, conditional markers). Returns recordings that the runtime replays under the synthetic `markers` span.
- **regex_validate.rs**: Compile-time regex validation for match patterns.

### `relux-resolver` (`crates/relux-resolver/`)

Resolver orchestration: module discovery, source loading, and the `resolve()` entry point.

- **lib.rs**: `resolve()` public API, `SourceLoader` trait, `FsSourceLoader`.
- **discover.rs**: `discover_test_modules()` — converts discovered `.relux` files to `ModulePath`s.
- **loader.rs**: `load_modules()` — BFS worklist that loads, parses, and enqueues transitive imports. `InMemoryLoader` for tests.
- **lower.rs**: Re-exports `LoweringContext`/`LoweringScope` from `relux-ir`. Contains shared test helpers.

### `relux-runtime` (`crates/relux-runtime/`)

- **lib.rs**: Suite executor. `execute()` runs tests with N workers (tokio tasks pulling from shared queue), cancellation support (fail-fast, suite timeout), flaky retry loop with timeout multiplier. `RunContext` holds per-run config. `EffectManager` handles effect lifecycle (instantiate → cleanup).
- **runtime_context.rs**: `RuntimeContext` — per-test context with structured log builder, shell config, log dir, tables, env, cancellation token, flaky timeout multiplier.
- **cancel.rs**: `CancelToken` + `CancelReason` — wraps `tokio_util::sync::CancellationToken` with a typed reason (`TestTimeout`, `SuiteTimeout`, `FailFast`, `Sigint`) so cancelled tests can report *why* they were stopped without a side channel. Child tokens inherit the parent's reason.
- **marker_walk.rs**: `collect_marker_recordings()` — pre-order traversal of a test's IR that collects every reachable fn/effect's marker recordings into a single deterministic list, replayed under a synthetic `markers` root span before test execution begins.
- **scan.rs**: `scan_artifacts()` — best-effort recursive walk of a test's artifact directory, producing the `Vec<ArtifactEntry>` stored on the structured log. Symlinks skipped, per-entry I/O errors silently dropped.
- **viewer.rs**: Compile-time-embedded gzip blobs (`bundle_gz`, `hljs_gz`, `hljs_grammar`) baked into the binary from `vendor/`. The pre-commit hook + CI verify these stay in sync with the `viewer/` sources.
- **vm/**:
  - **mod.rs**: Per-shell virtual machine. PTY child process, send/match operations with timeouts, fail pattern checking, statement execution, function calls. Send events emit *before* the PTY write awaits so the reader's `Grew` events cannot get a lower seq than the `Send` that triggered them.
  - **bifs.rs**: Impure built-in functions (`Bif` trait): `sleep`, `annotate`, `log`, `match_prompt`, `match_exit_code`, `match_ok`, `match_not_ok`, `ctrl_c`, `ctrl_d`, `ctrl_z`, `ctrl_l`, `ctrl_backslash`.
  - **buffer.rs**: Output buffer with cursor for matching operations. `BufferInner` is a `std::sync::Mutex` (not tokio) — no `.await` ever happens under the inner guard; sync mutex pairs cleanly with the builder's sync mutex for atomic mutate+emit.
  - **context.rs**: `ExecutionContext` — per-shell state: `Scope` (test/effect/function), `ShellState`, variable frames.
  - **pty.rs**: PTY process management.
- **effect/**:
  - **mod.rs**: `EffectManager` — instantiates effects, manages cleanup (reverse order under uncancellable tokens), warning collection. Cleanup spans always anchor under the test span, never under a parent setup span.
  - **registry.rs**: `EffectRegistry` — deduplicates effect instances by `(name, args, overlay)` key. `EffectGuard` is a refcount handle; `release_and_teardown` runs cleanup exactly once when the last guard releases.
- **observe/**:
  - **structured/**: The canonical log schema — `StructuredLogBuilder` (accumulator), `Event`, `Span`, `BufferEvent`, `ShellRecord`, `ArtifactEntry`, `SkipRecord`, `FailureRecord`, `CancellationRecord`. Builder is split by concern under `builder/` (`diagnostics.rs`, `io.rs`, `lifecycle.rs`, `matching.rs`, `values.rs`). `log_sink.rs` adapts the builder to the `PureEvalSink` trait. `utf8_stream.rs` incrementally decodes PTY bytes so matching always sees valid UTF-8. All types derive `serde` + `ts-rs` and are exported to `viewer/src/types/` under the `ts-export` feature.
  - **progress.rs**: Progress channel for live updates.
  - **tui.rs**: Terminal UI renderer (live progress with cursor control, auto-detects TTY).
- **report/**:
  - **result.rs**: `TestResult`, `Outcome` (Pass/Fail/Cancelled/Skipped/Invalid), `Failure` and `Cancellation` (carry a `FailureContext` with call stack, buffer tail, vars in scope), `ExecError` (internal VM error — `Failure | Cancellation`). All error enums use `thiserror::Error`.
  - **event_html.rs**: Per-test `event.html` emitter. Inlines four gzip+base64 payloads (StructuredLog JSON, hljs core, Relux hljs grammar, Svelte viewer bundle) into `<script type="application/octet-stream">` tags; a small bootstrap decompresses them via the browser-native `DecompressionStream` so the file opens directly under `file://` with no server.
  - **run_index.rs**: Per-run `index.html` — one row per test (outcome, duration, progress string, link to per-test artifact dir). CSS/JS sit in sibling `run_index.css` / `run_index.js` and are `include_str!`-inlined.
  - **console.rs**: Rich console error renderer — formats call stacks, buffer tails, and vars-in-scope on failure.
  - **hljs_init.rs**: Inline script that wires the Relux hljs grammar onto the bundled highlight.js.
  - **highlight-relux.js**: Canonical Relux hljs grammar, shared between the runtime report and the mdbooks (via `just _sync-book-assets`).
  - **junit.rs**: JUnit XML report output.
  - **tap.rs**: TAP (Test Anything Protocol) output.
  - **run_summary.rs**: `RunSummary` — serializable run results for history analysis.

### `relux` (CLI, `crates/relux-cli/`)

Published crate (`cargo install relux`). CLI subcommands and the `relux` binary.

- **lib.rs**: `cli()` command definition, shared helpers (`resolve_project`, `resolve_test_paths`, `read_file`, `build_source_loader`, `ModuleKind`).
- **run.rs**, **check.rs**, **new.rs**, **dump.rs**: One subcommand handler each.
- **completions.rs**: Shell completion installer (`--shell`, `--install`, `--path`).
- **completer.rs**: `ArgValueCompleter` functions for `.relux` files, manifests, timeouts, shells.
- **history/**: Analyzes test run history across multiple runs.
  - **mod.rs**: Subcommand dispatcher.
  - **loader.rs**: Reads `run_summary.toml` files from each run directory.
  - **analysis.rs**: Aggregates flaky detection, failure modes, first-fail identification, duration trends.
  - **format.rs**: Renders results as human-readable tables (via `tabled`) or TOML.
- **bin/relux.rs**: Thin dispatch layer — delegates to the library.

### Viewer (`viewer/`)

Svelte 5 + TypeScript SPA bundled by Vite into a single IIFE that the runtime inlines into each `event.html`.

- **`viewer/src/types/`**: Auto-generated TypeScript declarations exported by `ts-rs` from `relux-runtime`'s `observe::structured` types. Regenerate via `just viewer-build` (which first runs `cargo test -p relux-runtime --features ts-export 'export_bindings_'`).
- **`viewer/src/lib/`**: Logic modules — `state.svelte.ts` (root reactive state), `derive.ts` (selection-derived projections), `flatten.ts` (timeline flattening), `scope.ts` (variable scope resolution), `timeline.ts` (timeline layout), `format.ts` (display formatting), `source_highlight.ts` (hljs integration), `bif.ts`, `artifacts.ts`, `theme.ts`, `clipboard.ts`, `actions.ts`. Each has a colocated `*.test.ts` (Vitest).
- **`viewer/src/components/`**: Svelte 5 components — `EventsList`, `DetailPanel`, `TimelineBar`, `LogBar`, modals (`ShellsModal`, `EnvModal`, `ArtifactsModal`), rows (`EventRow`, `BifRow`, `SpanEntryRow`, `GapRow`), atoms (`Chip`, `Panel`, `ValueCell`, `MarkerPill`), plus `sections/` for the main detail tabs.
- **`viewer/src/styles/`**: CSS tokens shared with the docs theme (`docs/_theme/relux.css`) for product cohesion.
- **`viewer/scripts/gzip-bundle.mjs`**: Post-build step that gzips the IIFE; the output drops into `viewer/dist/relux-viewer.js.gz` and `just viewer-build` copies it to `vendor/relux-viewer.js.gz`.

### Vendored bundles (`vendor/`)

Pre-built gzip blobs baked into the runtime binary by `relux-runtime::viewer`:
- **`relux-viewer.js.gz`**: The Svelte IIFE bundle. Rebuilt by `just viewer-build`.
- **`highlight-11.11.1.min.js.gz`**: Vendored highlight.js v11 (shared between the viewer and the mdbooks).

The pre-commit hook (`.githooks/pre-commit`) and CI verify these files stay in sync with `viewer/` sources and the canonical hljs grammar at `crates/relux-runtime/src/report/highlight-relux.js`.

### Editor Support (`editors/`)

- **IntelliJ** (`editors/intellij/`): Syntax highlighting plugin for `.relux` files. Build with `just intellij`.
- **VS Code** (`editors/vscode/`): VS Code extension for `.relux` files. Build with `just vscode`.

## Key Design Decisions

- **All values are strings** — no type system beyond that
- **Pure vs impure split** — pure functions (string ops, random) evaluate at compile time in `let` bindings; impure functions (shell interaction) require a VM context
- **Effects are CamelCase, functions are snake_case** — enforced at parse level, used to disambiguate in imports
- **Effect identity** = `(name, expects)` — same tuple means deduplicated/reused instance; `EffectGuard` refcount ensures exactly-once teardown
- **Functions execute in caller's shell context** — no own shell, side effects (timeout/fail-pattern changes) persist
- **Cleanup blocks** run in fresh implicit shells with uncancellable tokens; any statement valid in a shell block is valid in a cleanup block
- **Module imports** resolve from project root, never relative to importing file
- **LayeredEnv** — environment variables use Arc-shared layered chain (no cloning of base env)
- **Concurrent test execution** — N worker tasks pull from a shared queue; results sorted back to original order
- **Structured event log is the canonical artifact** — every send/match/var-bind/marker-eval/pure-eval/span enter+exit/buffer growth/shell-io/effect-cleanup is recorded into a `StructuredLog` (`events.json`). The HTML viewer is built against this schema; do not add side-channel logs.
- **Send events emit before the PTY await** — the structured event for a `send` is recorded *before* `send_bytes().await` so the PTY reader's `Grew` event cannot get a lower seq than the `Send` that caused it. Maintain this invariant in any new send-path code.
- **Cancellation carries a typed reason** — `CancelToken` exposes `CancelReason` (`TestTimeout` / `SuiteTimeout` / `FailFast` / `Sigint`); observers can answer "why was I cancelled?" without a side channel. Always cancel through `cancel_with(reason)`, never bare `cancel()` outside tests.
- **Marker recordings are deterministic** — collected pre-order from the test's reachable fn/effect IR before execution begins, replayed under a single synthetic `markers` root span.

## Conventions

- Rust 2024 edition idioms; all error types use `#[derive(Debug, thiserror::Error)]` — no manual `Display`/`Error` impls
- Unit tests are colocated in each module via `#[cfg(test)] mod tests`; IR lowering tests are integration tests in `crates/relux-resolver/tests/`
- Viewer code is tested with Vitest colocated as `*.test.ts` beside the module under test
- Documentation as mdbooks in `docs/` — `reference/` (semantics, syntax, BIFs, CI integration, test-log viewer, events.json schema), `dsl-tutorial/`, `suite-tutorial/`
- **Every code change must be accompanied by updates to the relevant documentation** — review `docs/reference/` (semantics, syntax, BIFs, CI, test-log viewer, events.json schema), `docs/dsl-tutorial/`, and `docs/suite-tutorial/` and update any articles affected by the change
- **Changes to user-facing DSL syntax** (new keywords, operators, interpolation forms, etc.) **must be reflected in the editor plugins** — update `editors/vscode/syntaxes/relux.tmLanguage.json` and `editors/intellij/src/main/java/eu/spawnlink/relux/ReluxLexer.flex` (plus related token/highlighter files) — and in the canonical hljs grammar at `crates/relux-runtime/src/report/highlight-relux.js` (shared by the viewer and the mdbooks)
- **Changes to structured-log types** (`crates/relux-runtime/src/observe/structured/`) require regenerating viewer TypeScript bindings via `just viewer-build`; the vendored `vendor/relux-viewer.js.gz` must be committed in the same change. The pre-commit hook + CI verify the vendored bytes stay in sync.
- **PRs are squash-merged** — the final squash commit message must be a single conventional commit (type, optional scope, description, and body)

## RFCs

- RFCs for new features go in `rfc/` as `RXXX-short-name.md`
- `rfc/README.md` is the index — it must stay consistent with the actual RFC files. RFC status (proposed/accepted/implemented/superseded) is updated on `master` only.
- Each RFC is committed and submitted as a PR:
  - PR title: `RFC R009: Variable Match Operator` (format: `RFC RXXX: Capitalized RFC Name`)
  - PR body: the full RFC contents
