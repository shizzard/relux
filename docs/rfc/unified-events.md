# RFC: Unified Event System

Refactor the runtime's dual `emit_event()` + `emit_progress()` pattern
into a single event type with a unified `EventSink` that fans out to all
consumers. Simultaneously consolidate the 10+ parameters threaded through
runtime functions into a `RuntimeContext` struct.

## Motivation

- Every emit site calls both `emit_event(LogEventKind::...)` and
  `emit_progress(ProgressEvent::...)` — noisy and error-prone.
- `ProgressEvent` is a strict lossy projection of `LogEventKind` (minus 3
  progress-only events that lack log counterparts).
- Functions like `Vm::new()`, `EffectManager::new()`, and `run_test_body()`
  take 10+ parameters, all annotated `#[allow(clippy::too_many_arguments)]`.

## Design

### Sub-structs

```rust
/// Observability handle. All emit methods live here.
/// Cheap to clone — Arc + channel sender + Copy.
/// All methods are sync (std::sync::Mutex, never held across await points).
#[derive(Clone)]
pub struct EventSink {
    events: Arc<std::sync::Mutex<Vec<LogEvent>>>,
    progress_tx: ProgressTx,       // mpsc::UnboundedSender
    test_start: Instant,           // Copy
}

/// Immutable shell spawn configuration.
#[derive(Clone)]
pub struct ShellConfig {
    pub command: Arc<str>,
    pub prompt: Arc<str>,
    pub default_timeout: IrTimeout,
}
```

`EventSink` owns `test_start` and computes the relative timestamp before
pushing each event. The inner `Vec<LogEvent>` is a plain storage buffer
with no timestamp logic of its own (the current `EventCollector` wrapper
with its own `test_start` is removed).

### RuntimeContext

```rust
#[derive(Clone)]
pub struct RuntimeContext {
    pub events: EventSink,
    pub shell: ShellConfig,
    pub log_dir: Arc<Path>,
    pub tables: Tables,            // contains Arcs internally
    pub env: Arc<Env>,
    pub cancel: CancellationToken,
}
```

All fields are cheap to clone (Arc, channel sender, Copy).
Created once per test in `run_test()`.

### Emit methods

Named sync methods on `EventSink`. Each constructs the `LogEventKind`,
computes a relative timestamp from `test_start`, pushes it to the events
vec via `std::sync::Mutex`, and (where applicable) sends the corresponding
progress event via the channel. Methods with no progress representation
only push to the events vec.

The `std::sync::Mutex` lock is held only for a `Vec::push` — never across
an await point. This is an invariant that must be preserved.

All string parameters use `impl AsRef<str>` for ergonomic call sites
(accepts `&str`, `String`, `Arc<str>` without conversion).

```rust
impl EventSink {
    // --- Shell Lifecycle ---
    fn emit_shell_spawn(&self, shell: S, command: S);              // progress: 's'
    fn emit_shell_ready(&self, shell: S);                          // progress: —
    fn emit_shell_switch(&self, shell: S);                         // progress: '|'
    fn emit_shell_terminate(&self, shell: S);                      // progress: —
    fn emit_shell_alias(&self, shell: S, source: S);              // progress: —

    // --- I/O ---
    fn emit_send(&self, shell: S, data: S);                       // progress: '.'
    fn emit_recv(&self, shell: S, data: S);                       // progress: —

    // --- Pattern Matching ---
    fn emit_match_start(&self, shell: S,
        pattern: S, is_regex: bool);                               // progress: starts '~' timer
    fn emit_match_done(&self, shell: S,
        matched: S, elapsed: Duration,
        buffer: BufferSnapshot,
        captures: Option<HashMap<String, String>>);                // progress: '.'
    fn emit_timeout(&self, shell: S,
        pattern: S, buffer: BufferSnapshot);                       // progress: 'T'
    fn emit_buffer_reset(&self, shell: S,
        buffer: BufferSnapshot);                                   // progress: —

    // --- Fail Patterns ---
    fn emit_fail_pattern_set(&self, shell: S,
        pattern: S);                                               // progress: —
    fn emit_fail_pattern_cleared(&self, shell: S);                // progress: —
    fn emit_fail_pattern_triggered(&self, shell: S,
        pattern: S, matched_line: S,
        buffer: BufferSnapshot);                                   // progress: '!'

    // --- Effects ---
    fn emit_effect_setup(&self, shell: S, effect: S);             // progress: '+'
    fn emit_effect_teardown(&self, shell: S, effect: S);          // progress: '-'
    fn emit_cleanup(&self, shell: S);                              // progress: 'c'

    // --- Control Flow ---
    fn emit_sleep_start(&self, shell: S, duration: Duration);     // progress: starts 'z' timer
    fn emit_sleep_done(&self, shell: S);                           // progress: stops timer
    fn emit_fn_enter(&self, shell: S,
        name: S, args: &[(String, String)]);                       // progress: '{'
    fn emit_fn_exit(&self, shell: S,
        name: S, return_value: S,
        restored_timeout: Option<S>,
        restored_fail_pattern: Option<S>);                         // progress: '}'

    // --- Variables & Evaluation ---
    fn emit_var_let(&self, shell: S,
        name: S, value: S);                                        // progress: —
    fn emit_var_assign(&self, shell: S,
        name: S, value: S);                                        // progress: —
    fn emit_timeout_set(&self, shell: S,
        timeout: S, previous: S);                                  // progress: —
    fn emit_string_eval(&self, shell: S, result: S);              // progress: —
    fn emit_interpolation(&self, shell: S,
        template: S, result: S,
        bindings: &[(String, String)]);                            // progress: —

    // --- Diagnostics ---
    fn emit_annotate(&self, shell: S, text: S);                   // progress: '(text)'
    fn emit_log(&self, shell: S, message: S);                     // progress: —
    fn emit_warning(&self, shell: S, message: S);                 // progress: 'W'
    fn emit_error(&self, shell: S, message: S);                   // progress: 'E'
    fn emit_failure(&self, shell: S);                              // progress: 'F'

    // --- Extraction ---
    /// Consumes the collected events at test end for HTML report generation.
    /// Uses Arc::try_unwrap when possible, falls back to lock + drain.
    fn take(self) -> Vec<LogEvent>;
}
// where S: impl AsRef<str> on each method
```

### Who stores what

| Component | Stores | Reason |
|---|---|---|
| `EffectManager` | `RuntimeContext` | Constructs new Vms, needs everything |
| `Vm` | `EventSink` (pub) + `cancel` + `shell_prompt` | Only needs emit + cancellation + prompt matching at runtime |
| `PtyShell` | `ShellLogger` | Owns raw I/O capture, transfers with PTY |

`EventSink` is a public field on `Vm`. BIFs receive `&mut Vm` (not
`&mut dyn VmContext`) and access `self.events.emit_*()` directly.
The `VmContext` trait is removed — BIF dispatch changes to use concrete
`Vm` references.

### Cleanup VM behavior

Currently cleanup VMs are constructed with `None` for `progress_tx`,
suppressing progress output. This changes: cleanup VMs now receive the
same `EventSink` and emit progress events like any other shell. The
`c` token already marks cleanup start, so progress during cleanup
(sends, matches, etc.) will appear naturally in the progress string.

### What stays separate

- **ExecutionContext** — per-VM mutable state (vars, captures, call stack,
  fail patterns). Different lifecycle and mutability. Lives on `Vm.ctx`.
- **ShellLogger** — raw PTY I/O capture. Lives on `PtyShell`, transfers
  with it. When a PTY moves from an effect Vm to a test Vm, the logger
  moves too — they are an inseparable unit. `RuntimeContext` provides a
  factory method (`create_shell_logger(name)`) using its `log_dir` and
  `test_start`, but the resulting logger is owned by the `PtyShell`.

### Parameter reduction

| Function | Before | After |
|---|---|---|
| `run_test_body()` | 10 params | ~4 (meta, test, manager, rt_ctx) |
| `Vm::new()` | 10 params | ~3 (name, exec_ctx, rt_ctx) |
| `EffectManager::new()` | 10 params | ~2 (registry, rt_ctx) |

`run_test_body()` is also a direct emit site (shell switches, cleanup
start/warnings) — it uses `rt_ctx.events` directly.

Removes all `#[allow(clippy::too_many_arguments)]` annotations.

---

## Unified Event List

Single `LogEventKind` enum — the source of truth. Progress and HTML log
consumers derive their output from this one type.

Changes from current `LogEventKind`:
- **Added**: `Failure`, `Error { message }`, `Warning { message }` (were progress-only)
- **Split**: `Sleep { duration }` into `SleepStart { duration }` + `SleepDone`
- **New progress token**: `s` for `ShellSpawn`

### Progress Token Reference

Only events with a progress token emit a character. All other events
are silent in progress output (logged to HTML only).

```
s       shell spawned
.       send / match done
|       shell switch
~       waiting for match (per second)
z       sleeping (per second)
{       function enter
}       function exit
+       effect setup
-       effect teardown
c       cleanup
!       fail pattern triggered
T       timeout
F       failure
E       error
W       warning
(text)  annotation
```

### Shell Lifecycle

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `ShellSpawn` | `name, command` | `s` | yes |
| `ShellReady` | `name` | — | yes |
| `ShellSwitch` | `name` | `\|` | yes |
| `ShellTerminate` | `name` | — | yes |
| `ShellAlias` | `name, source` | — | yes |

### I/O

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `Send` | `data` | `.` | yes |
| `Recv` | `data` | — | yes |

### Pattern Matching

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `MatchStart` | `pattern, is_regex` | (starts `~` timer) | yes |
| `MatchDone` | `matched, elapsed, buffer, captures` | `.` | yes |
| `Timeout` | `pattern, buffer` | `T` | yes |
| `BufferReset` | `buffer` | — | yes |

### Fail Patterns

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `FailPatternSet` | `pattern` | — | yes |
| `FailPatternCleared` | | — | yes |
| `FailPatternTriggered` | `pattern, matched_line, buffer` | `!` | yes |

### Effects

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `EffectSetup` | `effect` | `+` | yes |
| `EffectTeardown` | `effect` | `-` | yes |
| `Cleanup` | `shell` | `c` | yes |

### Control Flow

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `SleepStart` | `duration` | (starts `z` timer) | yes |
| `SleepDone` | | (stops timer) | yes |
| `FnEnter` | `name, args` | `{` | yes |
| `FnExit` | `name, return_value, restored_timeout, restored_fail_pattern` | `}` | yes |

### Variables & Evaluation

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `VarLet` | `name, value` | — | yes |
| `VarAssign` | `name, value` | — | yes |
| `TimeoutSet` | `timeout, previous` | — | yes |
| `StringEval` | `result` | — | yes |
| `Interpolation` | `template, result, bindings` | — | yes |

### Diagnostics

| Event | Fields | Progress | HTML |
|---|---|---|---|
| `Annotate` | `text` | `(text)` | yes |
| `Log` | `message` | — | yes |
| `Warning` | `message` | `W` | yes |
| `Error` | `message` | `E` | yes |
| `Failure` | | `F` | yes |
