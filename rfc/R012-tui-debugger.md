# R012: Interactive Debugger

- **Status**: draft
- **Created**: 2026-04-23

## Motivation

When a relux test fails, the HTML report shows what happened — buffer state, matched patterns, timeouts. But it shows the *result* of the failure, not the state leading up to it. Debugging often requires re-running the test with added `log()` calls or adjusted timeouts, iterating until the root cause surfaces.

An interactive debugger lets the user pause execution *before* a failure, inspect live shell buffers, step through DSL instructions, and observe the test state as it evolves. The goal is pre-failure inspection, not post-mortem analysis — the HTML reports already cover post-mortem.

## Design

### Architecture

The debugger is embedded in the relux binary. `relux run --debug` starts the test runtime, an embedded HTTP/WebSocket server, and opens the browser. A single process — no IPC, no separate binary. The server serves a static frontend and communicates with the browser via JSON over WebSocket.

### Prerequisites (relux changes)

- **Single test selection**: `relux run module.relux --test "test name"` (filter by name or line number)
- **Debug mode**: `relux run --debug module.relux --test "test name"` — starts paused, serves debugger UI on a local port, prints the URL
- **Timeout multiplier**: debug mode starts with a generous default multiplier, user-overridable. A special "freeze" mode sets an effectively infinite multiplier and hides the timeout countdown, so the user isn't racing the clock while inspecting state. The shells remain live — freeze affects DSL-level timeouts only.

### Debug Protocol

JSON over WebSocket. Commands flow from browser to server, events stream from server to browser. The WebSocket naturally models the bidirectional debugger interaction.

#### Why not DAP?

DAP (Debug Adapter Protocol) was evaluated as the primary option since it's the de facto standard with support in VS Code, Neovim (nvim-dap), Emacs (dap-mode), and JetBrains. DAP concepts map well to relux:

| DAP concept   | Relux concept                             |
|---------------|-------------------------------------------|
| Thread        | Shell / Effect                            |
| Stack Frame   | Current position in test/function/effect  |
| Source        | `.relux` test files, module files         |
| Breakpoint    | Line in DSL source                        |
| Variable      | Relux variables, capture groups, env vars |
| Step In       | Enter function/effect body                |
| Step Over     | Next DSL instruction                      |
| Step Out      | Return to caller                          |
| Output Event  | Shell stdout/stderr                       |
| Stopped Event | Breakpoint hit, pattern match, timeout    |

However, DAP has no concept of "waiting for a condition with a deadline." Timeouts could be approximated via DAP's `progressStart`/`progressUpdate`/`progressEnd` events or custom `relux/*` events, but the live buffer streaming with pattern overlay is the killer feature of this debugger, and DAP clients would show a degraded experience (a progress bar instead of the actual buffer). Without the live buffer view, IDE integration adds little value over the existing HTML reports.

DAP adapter as a future option remains viable.

#### Key protocol capabilities

- Start/pause/resume execution
- Step over (next actionable line), step in (enter function/effect body), step out (return to caller)
- Set/remove breakpoints by file and line (multiple breakpoints across files supported)
- Query callstack, variables (local + global scopes), effect statuses
- Stream shell output buffers in real time
- Report current operation, pattern wait state, and timeout countdown
- Stream evaluation trees — structured trace of each statement's evaluation (variable resolutions, function calls, interpolations)
- Toggle freeze mode (infinite timeout multiplier, no countdown display)

### UI Phases

The debugger has two phases: **pre-run** (source browsing, breakpoint setup) and **execution** (live debugging). Both are full-page browser views.

#### 1. Pre-Run Phase

Source viewer for setting breakpoints and navigating code before execution starts.

- **Source listing**: syntax-highlighted source with line numbers. The target test body is the initial view, with the cursor on the first actionable line.
- **Line navigation**: click or keyboard (up/down) moves between actionable lines only — lines that produce runtime effects. Non-actionable lines are visible but skipped during navigation.
- **Breakpoint toggling**: click line number or press a key to toggle. Breakpoints can only be set on actionable lines. Breakpoints persist across file navigation — the user can set breakpoints in multiple files (test source, library functions, effect definitions) before launching.
- **Jump to definition**: click or press a key on a line to navigate into function/effect definitions. Navigation works as a stack — jump in pushes, back pops.
  - Single jumpable reference on the line: navigates directly
  - Multiple references (e.g. `let x = foo("bar", bar(baz(), 1122))`): dropdown picker listing all jumpable targets on that line
- **Breakpoint list**: sidebar or panel showing all set breakpoints across files
- **Run button**: launches execution and transitions to execution phase

**Actionable lines** — the exhaustive set of line types that accept breakpoints and participate in navigation:

| Element                | Example                     |
|------------------------|-----------------------------|
| Send                   | `> echo hello`              |
| Raw send               | `=> partial`                |
| Regex match            | `<? ^hello$`                |
| Literal match          | `<= exact text`             |
| Negative match         | `<!? ERROR`, `<!= FATAL`    |
| Inline timeout match   | `<~30s? ^done$`, `<@2s= OK` |
| Buffer reset           | `<?`, `<=` (bare)           |
| Fail pattern set/clear | `!? panic`, `!=`, `!?`      |
| Timeout set            | `~10s`, `@2s`               |
| Variable declaration   | `let x = "value"`           |
| Variable assignment    | `x = "new"`                 |
| Let-from-match         | `let val = <? pattern`      |
| Function call (impure) | `match_ok()`, `sleep(1)`    |
| Function call (pure)   | `trim(x)`, `len(s)`         |
| Start effect           | `start MyEffect`            |
| Need                   | `need MyEffect`             |
| Condition marker       | `[skip unless CI]`          |

**Non-actionable lines** — skipped during navigation, cannot have breakpoints:

| Element              | Example                               |
|----------------------|---------------------------------------|
| Comments             | `# this is a comment`                 |
| Blank lines          |                                       |
| Block open/close     | `shell s {`, `}`                      |
| Test declaration     | `test "name" {`                       |
| Function declaration | `fn name(args) {`                     |
| Effect declaration   | `effect Name -> shell s {`            |
| Import               | `import lib/helpers { check_status }` |
| Docstring            | `"""..."""`                           |

**Jumpable points** — targets for jump-to-definition:

- `start Effect...` — jump to the effect definition
- User-defined function calls — jump to the function definition (not BIFs)

**Root file scoping**: when viewing the root file (bottom of the navigation stack), only lines inside the target test body are navigable and breakpointable. Lines outside the test are visible but inert — they never execute for this test. When jumping into a referenced file, the entire definition body becomes navigable.

#### 2. Execution Phase

Main debugger view with four panels:

##### Source listing

Current module source with line cursor indicating execution position. Breakpoint markers in line number gutter. Breakpoints remain togglable during execution.

##### Active shell buffer

Live-streaming PTY output from the currently active shell. When awaiting a pattern match, displays the pattern being waited on, timeout countdown (hidden in freeze mode), and the buffer tail so the user can see why the pattern isn't matching yet. Active fail patterns shown per shell. The debugger switches to the active shell automatically when stepping. Other shells accessible via shell switcher.

##### Callstack and variables

Function call stack, local variables for current scope, global variable scope (for effects and test shells) when it exists, and capture group bindings (`$1`, `$2`, etc.).

##### Evaluation log

Each DSL statement produces an evaluation tree that captures every intermediate operation: variable resolutions, interpolations, function calls with resolved arguments and return values. The VM context holds the current evaluation tree as a structured field — each new statement starts a fresh tree root, and operations append subtrees as they execute. The tree is streamed to the debugger via the protocol and accumulates as an ever-growing log across the test run.

The panel shows the most recent evaluation inline. An expand action opens a full scrollable view of the entire log history.

Example evaluation trees for complex expressions:

```
#7  foo(bar("something", ${1}), baz(x, "interpolated ${y}"))
      ├─ ${1} = "captured"
      ├─ ${y} = "world"
      ├─ bar("something", "captured") = "bar_result"
      ├─ baz("x_value", "interpolated world") = "baz_result"
      └─ foo("bar_result", "baz_result") = "final"

#12 > "${cmd} --flag ${val}"
      ├─ ${cmd} = "deploy"
      ├─ ${val} = "prod"
      └─ → deploy --flag prod

#15 [skip unless ARCH ? ^(x86_64|aarch64)$]
      ├─ ARCH = "arm64"
      ├─ ^(x86_64|aarch64)$ ~ "arm64" = no match
      └─ → skip
```

#### On-Demand Views

- **Effects status**: list of effects with their state (starting/started/stopped) and evaluated ENV variables
- **Function jump picker**: when a line has multiple function references
- **Shell switcher**: navigate to non-active shells to inspect their buffers. Shells have distinct labels. Unaliased effect shells (inaccessible from test code but running in the background) get generated aliases.

### Design Decisions

#### Browser over TUI

A browser UI avoids terminal rendering complexity — no manual layout, no cell-by-cell rendering, no scroll viewport math. CSS handles layout, resizable panels, and text styling natively. WebSocket is simpler than gRPC. The single-process embedded server eliminates IPC overhead.

#### Cross-file navigation: popup menu over cursor-level selection

When a line references multiple functions, a popup picker lists all references rather than requiring horizontal cursor navigation within the line. This keeps the line-based mental model intact.

#### Pattern match visualization: side-by-side over partial match highlighting

Showing how far a regex matched before failing was considered and rejected — regex engines don't expose backtracking state, and reimplementing matching with custom visualization is a rabbit hole. Instead, the debugger shows the pattern and the buffer tail side by side. This covers the vast majority of debugging value.

#### Effects display: on-demand over always-visible

Effects status is a popup rather than a persistent panel. Effects are primarily relevant during startup and teardown, not during main test execution. Effect shells are otherwise no different from test shells — breakpoints work in effect code the same way.

#### Timeout handling in debug mode

The shells are live regardless of debugger state — pausing timeouts entirely is not meaningful since the programs under test continue running. Instead, freeze mode sets an effectively infinite timeout multiplier, giving the user as much time as needed without the fiction of stopped time.

## Future Considerations

- Conditional breakpoints (e.g. break when `$1` matches a value)
- DAP adapter layer for IDE integration (with degraded buffer experience)
- Post-mortem mode (or continue relying on existing HTML reports)
