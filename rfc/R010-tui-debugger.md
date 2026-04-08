# R010: TUI Debugger

- **Status**: draft
- **Created**: 2026-04-08

## Motivation

When a relux test fails, the HTML report shows what happened — buffer state, matched patterns, timeouts. But it shows the *result* of the failure, not the state leading up to it. Debugging often requires re-running the test with added `log()` calls or adjusted timeouts, iterating until the root cause surfaces.

A TUI debugger lets the user pause execution *before* a failure, inspect live shell buffers, step through DSL instructions, and observe the test state as it evolves. The goal is pre-failure inspection, not post-mortem analysis — the HTML reports already cover post-mortem.

## Design

### Architecture

The debugger is a standalone binary (`relux-dbg`) that launches relux as a child process and communicates with it via a debug protocol over TCP. The debugger selects the port and passes it to relux. This decouples the debugger UI from the test runtime, allows independent release cycles, and keeps the relux binary focused. TCP is chosen over Unix sockets for future Windows compatibility.

### Prerequisites (relux changes)

- **Single test selection**: `relux run module.relux --test "test name"` (filter by name or line number)
- **Debug mode**: `relux debug module.relux --test "test name"` — starts paused, exposes debug protocol endpoint on a TCP port
- **Timeout multiplier**: debug mode starts with a generous default multiplier, user-overridable. A special "freeze" mode sets an effectively infinite multiplier and hides the timeout countdown, so the user isn't racing the clock while inspecting state. The shells remain live — freeze affects DSL-level timeouts only.

### Debug Protocol

Custom protocol over TCP, using gRPC with a single bidirectional stream. This gives Protobuf wire format plus service definition, and the bidirectional stream naturally models the debugger interaction — commands flow in, events stream out.

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

### UI Framework

Built with Ratatui.

### UI Model: Modal Phases

The debugger uses distinct full-screen modes rather than an IDE-like persistent panel layout. The debugger is not an editor — the user's primary task changes between phases, so the full screen should reflect that.

### UI Modes

#### 1. Test Selector

File tree with `.relux` files expanded to show their test names as child nodes. The debugger parses test files using relux's decoupled lexer/parser to extract test names and line numbers. Select a test to enter pre-run mode.

```
┌─ relux-dbg ── Test Selector ─────────────────────────────────────────────────┐
│                                                                              │
│  suite/                                                                      │
│   ├─ auth.relux                                                              │
│   │   ├─ test "login with valid credentials"                                 │
│   │  ►│   ├─ test "login with expired token"                                 │
│   │   └─ test "logout clears session"                                        │
│   ├─ healthcheck.relux                                                       │
│   │   └─ test "service healthcheck"                                          │
│   └─ migration/                                                              │
│       └─ schema.relux                                                        │
│           ├─ test "migrate up"                                               │
│           └─ test "migrate down"                                             │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│ ↑↓ navigate  Enter select  q quit                                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### 2. Pre-Run Mode

Source viewer with:

- **Line navigation**: up/down moves between actionable lines only — lines that produce runtime effects. Non-actionable lines (comments, blank lines, block delimiters, structural keywords) are visible but skipped during navigation.
- **Breakpoint gutter**: toggle breakpoints with a keypress, visual marker in gutter. Breakpoints can only be set on actionable lines. Breakpoints persist across file navigation — the user can set breakpoints in multiple files (test source, library functions, effect definitions) before launching.

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

- **Jump to definition**: press jump key on a line to navigate into function/effect definitions
  - Single function reference on the line: jumps directly (no extra keypress)
  - Multiple references (e.g. `let x = foo("bar", bar(baz(), 1122))`): popup picker listing all functions on that line with their source locations
  - For effects: navigate to `start Effect` line and press jump
- **Trigger execution**: keypress to launch relux in debug mode and transition to execution mode

Non-actionable lines (test declaration, docstring, `shell s {`, `}`) are visible but have no line numbers — the cursor skips them.

```
┌─ relux-dbg ── Pre-Run ── auth.relux ─────────────────────────────────────────┐
│     test "login with expired token" {                                        │
│         """                                                                  │
│         Verify expired tokens are rejected with a clear error.               │
│         """                                                                  │
│         shell s {                                                            │
│ ●  12       !? PANIC|SEGFAULT                                                │
│    13       > ./auth-cli login                                               │
│►   14       <? ^Token:\s*$                                                   │
│    15       > expired-token-abc123                                           │
│    16       <? ^Error: token expired                                         │
│    17       match_ok()                                                       │
│    18       > ./auth-cli status                                              │
│    19       <? ^logged out$                                                  │
│    20       match_ok()                                                       │
│         }                                                                    │
│     }                                                                        │
│                                                                              │
│                                                                              │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│ ↑↓ navigate  b breakpoint  g jump-to-def  r run  q back     ● 1 breakpoint   │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### 3. Execution Mode

Main debugger view with the following panels:

Four panels: source listing (top-left), active shell buffer (top-right), callstack and variables (bottom-left), evaluation log (bottom-right).

```
┌─ relux-dbg ── Executing ── auth.relux ───────────────────────────────────────┐
│ Source                              │ [shell s] awaiting <? ^Error: token exp│
│                                     │ ────────────────────────── 3s/30s ─────│
│    12   !? PANIC|SEGFAULT           │ $ ./auth-cli login                     │
│    13   > ./auth-cli login          │ Token:                                 │
│    14   <? ^Token:\s*$              │ $ expired-token-abc123                 │
│    15   > expired-token-abc123      │ validating token...                    │
│ ►  16   <? ^Error: token expired    │ contacting auth server...              │
│    17   match_ok()                  │ █                                      │
│    18   > ./auth-cli status         │                                        │
│    19   <? ^logged out$             │                                        │
│    20   match_ok()                  │                                        │
│                                     │                                        │
├─────────────────────────────────────┼────────────────────────────────────────┤
│ Callstack          │ Variables      │ Eval: <? ^Error: token expired         │
│                    │                │   └─ → ^Error: token expired           │
│ test "login wi..." │ $0 = "Token:"  │                                        │
│  └─ shell s        │ $1 = ""        │                                        │
│                    │                │                                        │
│                    │ !? PANIC|SEGF… │                                        │
├────────────────────┴────────────────┴────────────────────────────────────────┤
│ F5 continue  F10 step  f freeze  e eval-log  s shells  x effects  q quit     │
└──────────────────────────────────────────────────────────────────────────────┘
```

##### Source listing with cursor

Top-left panel. Current module source with line cursor indicating execution position. Breakpoint markers in gutter.

##### Active shell buffer

Top-right panel. Live-streaming PTY output from the currently active shell. When awaiting a pattern match, displays the pattern being waited on, timeout countdown (hidden in freeze mode), and the buffer tail so the user can see why the pattern isn't matching yet. Active fail patterns shown per shell. The debugger switches to the active shell automatically when stepping. Other shells accessible via shell switcher.

##### Callstack and variables

Bottom-left panel. Function call stack, local variables for current scope, global variable scope (for effects and test shells) when it exists, and capture group bindings (`$1`, `$2`, etc.).

##### Evaluation log

Bottom-right panel. Each DSL statement produces an evaluation tree that captures every intermediate operation: variable resolutions, interpolations, function calls with resolved arguments and return values. The VM context holds the current evaluation tree as a structured field — each new statement starts a fresh tree root, and operations append subtrees as they execute. The tree is streamed to the debugger via the protocol and accumulates as an ever-growing log across the test run.

The panel shows the most recent evaluation inline. Pressing `e` opens a full scrollable overlay of the entire log history. Exact rendering (flat, nested, collapsed) is a debugger concern — the protocol sends the structured tree and the debugger decides presentation.

Evaluation log overlay:

```
┌─ relux-dbg ── Executing ── auth.relux ───────────────────────────────────────┐
│ Source            ┌─ Evaluation Log ──────────────────────────────────────┐  │
│                   │                                                       │  │
│    12   !? PANI   │  #1  !? PANIC|SEGFAULT                                │  │
│    13   > ./aut   │       └─ → PANIC|SEGFAULT                             │  │
│    14   <? ^Tok   │                                                       │  │
│    15   > expir   │  #2  > ./auth-cli login                               │  │
│ ►  16   <? ^Err   │       └─ → ./auth-cli login                           │  │
│    17   match_o   │                                                       │  │
│    18   > ./aut   │  #3  <? ^Token:\s*$                                   │  │
│    19   <? ^log   │       └─ → ^Token:\s*$                                │  │
│    20   match_o   │                                                       │  │
│                   │  #4  > expired-token-abc123                           │  │
├────────────────   │       └─ → expired-token-abc123                       │  │
│ Callstack         │                                                       │  │
│                   │ ►#5  <? ^Error: token expired                         │  │
│ test "login wi…   │       └─ → ^Error: token expired                      │  │
│  └─ shell s       │                                                       │  │
│                   └───────────────────────────────────── ↑↓ scroll  Esc ──┘  │
├──────────────────────────────────────────────────────────────────────────────┤
│ F5 continue  F10 step  f freeze  e eval-log  s shells  x effects  q quit     │
└──────────────────────────────────────────────────────────────────────────────┘
```

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

#### 4. On-Demand Popups

- **Effects status**: list of effects with their state (starting/started/stopped) and evaluated ENV variables
- **Function jump picker**: when a line has multiple function references
- **Shell switcher**: navigate to non-active shells to inspect their buffers. Shells have distinct labels. Unaliased effect shells (inaccessible from test code but running in the background) get generated aliases.

### Design Decisions

#### Cross-file navigation: popup menu over cursor-level selection

When a line references multiple functions, a popup picker lists all references rather than requiring horizontal cursor navigation within the line. This keeps the line-based TUI mental model intact.

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
