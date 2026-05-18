# Test Log Viewer

The **test log viewer** is the per-test HTML report that ships with every Relux run as `event.html` inside the run directory's `logs/<test>/` folder. It is a single self-contained SPA: open it directly via `file://`, no server required. See [CI Integration](04-ci-integration.md) for how the file is packaged and shipped.

This article catalogs the viewer's surface. Each region is listed with the data it shows and the keys that act on it.

## Layout

The viewer has four persistent regions, top to bottom:

1. **App bar** — test identity, outcome, modal launchers, run timing.
2. **Timeline bar** — proportional bar of the test's time range with click-to-jump slices.
3. **Events list** (left pane) — the structured event log as a foldable tree.
4. **Detail panel** (right pane, 2x2 grid) — source / shell / variables / call stack views of the current selection.

Three modal overlays — **env**, **shells**, **artifacts** — open on top of the layout.

Selection is the spine of the UI: nearly every pane reads from `selectedSpanId` or `selectedEventSeq`. Clicking a row in the events list, clicking a timeline slice, or clicking a frame in the call stack all change the selection; every other pane re-derives from there.

## App bar

The strip across the top of the viewer.

Contents, left to right:

- **Breadcrumb** — `<directory>/<file>` followed by the test name.
- **Outcome pill** — `pass`, `fail`, `cancelled`, `skip`, or `invalid`. `cancelled` indicates the test was stopped before completion (test-timeout, suite-timeout, fail-fast, or SIGINT); the in-stream `cancelled` event tells you which one.
- **Modal launcher chips** — `env`, `shells (N)`, `artifacts (N)`. The artifacts chip is disabled when the test produced no files.
- **Timing summary** — total duration, event count, span count.

| Key | Action |
|-----|--------|
| `E` | Toggle the env modal |
| `S` | Toggle the shells modal |
| `A` | Toggle the artifacts modal (no-op when empty) |

## Timeline bar

A proportional time track spanning the test's duration.

The selected event or span is rendered as a pulsing accent slice over the bar. Hovering anywhere on the track reveals one or more **preview cards** for the spans active at that timestamp: a short stack of cards anchored to the slice on the track, each summarizing one candidate span. Clicking the track selects:

- the only span there, if there is one;
- otherwise, it **pins** the preview cards so you can click the one you want.

Clicking outside a pinned card stack dismisses the pin. Clicking a preview card selects its span and reveals the matching row in the events list.

No keyboard shortcuts; the timeline bar is mouse-driven.

## Events list

The left pane. Renders the structured event log as a foldable, indented tree.

Row types:

- **Span entry** — a span's opening row. Indented by depth, foldable.
- **Event** — a non-span event (send, match, var-let, interpolation, ...). Some related events are **folded** into a single row (e.g. a `match-start` / `match-done` pair, a `sleep-start` / `sleep-done` pair).
- **Log bar** — emitted by the `log` BIF; rendered as a horizontal bar carrying the log level and message.
- **BIF row** — a transparent impure BIF call (e.g. `match_prompt`) shown as a single row instead of a foldable span.
- **Gap** — a synthetic row marking a duration with no events.

The footer below the list carries chips for filtering and bulk fold control:

- **Filter** — opens a popup with one checkbox per event type. The chip is highlighted when any types are hidden.
- **Error path** preset — hides everything except `error`, `fail-pattern-triggered`, `match-timeout`. Disabled when the test passed.
- **Send / match only** preset — hides everything except `send`, `match`, `match-timeout`.
- **Collapse all** / **Expand all** — fold or unfold every span at once.

| Key | Action |
|-----|--------|
| `Up` / `Down` | Move selection to the previous / next row |
| `Right` | Expand the selected span |
| `Left` | Collapse the selected span |
| `Enter` / `Space` | If an event is selected, deselect it; if a span is selected, toggle its fold |
| `F` | Toggle the filter popup |
| `T` | Toggle the error-path preset (no-op when the test passed) |
| `M` | Toggle the send / match preset |
| `C` | Collapse all spans |
| `X` | Expand all spans |

## Detail panel

The right pane. A 2x2 grid of panes, all driven by the current selection:

```text
+---------------------+---------------------+
|       source        |        shell        |
+---------------------+---------------------+
| variables in scope  |     call stack      |
+---------------------+---------------------+
```

### Source

Renders the `.relux` file the selection points to, with the relevant byte range outlined by a pulsing accent frame. The header hint shows `<file>:<line>`; the view auto-scrolls to vertically center the anchor line and horizontally to keep the framed range on screen. Function calls, BIF rows, and imported items resolve to the file that actually defines them, not the file that called them.

When the selection has no source location, the pane shows `no location` and a placeholder.

### Shell

The output buffer of the shell that owns the selected event, snapshotted at the moment of selection.

Renders three regions concatenated, top to bottom:

- **Consumed** — bytes already matched and advanced past. Dimmed.
- **Matched** — bytes consumed by the most recent match up to and including the selected event. Accent color, pulsing.
- **Tail** — bytes still in the buffer after the cursor. Default ink.

The header hint surfaces the shell's state at the moment of selection: timestamp, `matched ✓` if the selected event was a successful match, the active timeout, and the count of fail patterns armed in scope.

When the selection has no shell context (e.g. a pure-function span), the pane shows `this event has no shell context`.

This pane embeds the **searchable buffer** (see below) — type into the search field to find substrings inside the buffer.

### Variables in scope

A two-section key/value table:

- **Captures** (`$name`) — regex captures live in this scope, rendered with the accent color.
- **`let` variables** — variables declared at this scope or any enclosing scope.

The footer carries a chip that links to the env modal — environment variables are not shown in this pane; they live in the env modal because they are global to the test.

When the selection has no scope context, the pane shows a placeholder.

### Call stack

The stack of lexical scopes that contain the selected event, deepest frame at the top. Each frame shows its kind (e.g. `TEST`, `FN-CALL`, `EFFECT-SETUP`), name, optional alias, source location, and any bound arguments.

The topmost frame is fixed (it is the selection's own frame). Clicking a lower frame "promotes" it: the viewer selects that frame's inner neighbor, effectively walking out one level. Use this to navigate from a deeply nested BIF call back up to the test body.

The pane's footer lists **also-live shells** — shells that are running but are not the one owning the selected event. Useful for multi-shell tests where work is happening in parallel.

## Searchable buffer

Used in two places: the **shell** pane of the detail panel, and inside every card in the shells modal. A single-line search bar above a buffer view.

The query is matched against the rendered (escape-expanded) buffer text using substring search with **smart case**: case-insensitive unless the query contains an uppercase letter. The bar shows `<current> / <total>` matches; the current hit is rendered with a stronger accent than the rest.

| Key | Action |
|-----|--------|
| `Enter` | Cycle to the next hit |
| `Shift+Enter` | Cycle to the previous hit |
| `Esc` | Clear the query; blur the field if already empty |
| `Cmd+S` / `Ctrl+S` | Focus / cycle search inputs (see [Global hotkeys](#global-hotkeys)) |

## Env modal

Snapshot of the process environment captured when the test started.

The body lists every variable, grouped by origin (`relux internals`, `cargo`, `nix / toolchain`, `shell & terminal`, `large blobs`, `other`). A filter row at the top accepts a query and a scope toggle: filter by **name**, **value**, or **name · matches** (either side). The counter shows `<filtered> / <total>`.

The header carries a `copy all` action that copies the full environment as `KEY=VALUE` lines, one per row.

| Key | Action |
|-----|--------|
| `E` | Toggle the modal |
| `Esc` | Close the modal |
| `Cmd+S` / `Ctrl+S` | Focus the filter input |

## Shells modal

One card per shell spawned during the test, sorted by spawn time.

Each card has:

- **Header** — shell name, command, state dot (`running`, `awaiting input`, `ended`, `error`), and a `★ this event` badge when the card corresponds to the shell owning the currently selected event.
- **Stats line** — spawn timestamp, buffer size, events seen up to the selection, and termination timestamp (when ended before the selected event).
- **Buffer column** — a searchable buffer view for that shell's output at the moment of the selected event.

The modal subtitle reflects the current selection (`@ event #N · kind · t = ... · in <test>`) so you always know what timestamp the buffers are snapshotted at.

| Key | Action |
|-----|--------|
| `S` | Toggle the modal |
| `Esc` | Close the modal |
| `Cmd+S` / `Ctrl+S` | Cycle through the cards' search inputs |

## Artifacts modal

The list of files the test wrote under its `artifacts/` directory.

Each row is `path · size · mime`. The path is a link that opens the artifact in a new tab (resolved relative to the report directory). A filter row at the top narrows the list; the header `copy all` action copies every path as a newline-separated list.

The modal launcher chip in the app bar is disabled when the test produced no artifacts; the `A` hotkey is a no-op in that case.

| Key | Action |
|-----|--------|
| `A` | Toggle the modal (no-op when the test has no artifacts) |
| `Esc` | Close the modal |
| `Cmd+S` / `Ctrl+S` | Focus the filter input |

## Global hotkeys

A consolidated reference. Keys without a modifier are ignored while a text input or contenteditable element has focus; the search-input cycle is the one exception (it deliberately runs before the input-focus guard so it can move from one input to the next).

| Key | Scope | Action |
|-----|-------|--------|
| `E` | global | Toggle env modal |
| `S` | global | Toggle shells modal |
| `A` | global | Toggle artifacts modal |
| `T` | global | Toggle error-path preset in the events list |
| `M` | global | Toggle send / match preset in the events list |
| `F` | global | Toggle the events-list filter popup |
| `C` | global | Collapse all spans |
| `X` | global | Expand all spans |
| `Esc` | global | Close the open modal |
| `Cmd+S` / `Ctrl+S` | global | Focus / cycle search inputs in the current scope (modal if one is open, otherwise the main view) |
| `Up` / `Down` | events list | Move selection by one row |
| `Right` / `Left` | events list | Expand / collapse the selected span |
| `Enter` / `Space` | events list | Toggle the current row |
| `Enter` | searchable buffer | Cycle to the next hit |
| `Shift+Enter` | searchable buffer | Cycle to the previous hit |
| `Esc` | searchable buffer | Clear the query; blur if already empty |
