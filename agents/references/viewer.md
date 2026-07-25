# Test Log Viewer

`event.html` is a self-contained Svelte SPA written next to each test's `events.json`. It is a **human surface** -- the agent does not read or interact with it. The agent's job is to (a) hand the user a link when visual triage will help, and (b) ask the user for clues that only the visual surface reveals (a buffer state, a span tree shape, a timeline anomaly).

For every programmatic reconstruction question -- buffer state at a seq, scope walk, call stack, dedup audit -- use [events-recipes](events-recipes.md) against `events.json`. Do not try to substitute the viewer.

## The link to share

For a given run and test, the path is:

```
relux/out/<run>/logs/<test-path>/<test-name>/event.html
```

`relux/out/latest` always symlinks to the most recent run, so a stable share looks like:

```
relux/out/latest/logs/<test-path>/<test-name>/event.html
```

Open with `file://` -- no server, no build. Modern browser required (Chrome/Edge 80+, Firefox 113+, Safari 16.4+); older browsers see a one-line fallback message and should be pointed at `events.json` directly.

## When to hand off to the user

Reach for the link when the answer is visual or the user's eye is faster than another scripted pass:

- A failure narrowed to a buffer region that won't fit on one console screen.
- An unusual dedup or span structure that reads faster as a tree.
- "What did the run actually look like?"
- An interpretation question the user can confirm in a click.

The hand-off is a confirmation or clarification step, not a way to outsource diagnosis you could do via `events.json`.

## What the user sees there (so the agent can ask focused questions)

Knowing the surface helps the agent ask useful questions. The viewer shows:

- A timeline strip across the top, with events in `seq` order.
- A foldable event/span list on the left.
- A detail panel on the right with `event`, `shell`, `scope`, and `source` panes -- all visible at once for the selected event.
- Top-bar modals for shells, env, and artifacts.
- A synthetic `markers` root for skipped tests, with the firing marker focused on open.
- `reused` setup spans that link back to the original bootstrap setup.

**On open:** the viewer auto-selects the relevant event and expands its ancestors so the detail panel is already showing what matters:

- `fail` -> the failing event (`outcome.event_seq`).
- `cancelled` -> the event where cancellation fired.
- `skip` -> the `bool-check` under the firing marker (the synthetic `markers` root carries it).
- `pass` -> nothing pre-selected.

There is no separate "jump to failure" hotkey because the failing event is already the selection on open. If the user has navigated away and wants to return, `t` toggles the error-path preset (the list collapses to just the events leading to the failure), making the failing event one or two `Down` keystrokes away.

Useful questions to ask the user:

- "Open `<link>` -- the failing event is already selected; tell me what the `shell` pane shows for `buffer_tail`."
- "In `<link>` the firing marker is already selected under the `markers` root; tell me the marker name."
- "Click the `reused` setup span at the top -- does it link to a previous `EffectName` setup with the same overlay?"
- "Look at the timeline strip in `<link>` -- is there a long gap before the failing event?"

Keep the questions narrow. The viewer is for the user's eyes; the agent treats the answers as opaque inputs.

## Hotkeys (for guiding the user)

When the user is navigating the viewer, the agent can suggest a hotkey instead of describing a click path. All keys work when no text input is focused (text inputs swallow the key).

### Top-bar modals

| Key | Action |
|---|---|
| `e` | Toggle the env modal (every env var captured at test start, grouped by provenance -- base / `.env` / relux-internal -- and searchable). |
| `s` | Toggle the shells modal (per-shell buffer inspector). |
| `a` | Toggle the artifacts modal (disabled when the test wrote no artifacts). |
| `f` | Toggle the filter modal (custom hide/show by event type). |
| `Esc` | Close whichever modal is open. |

### Event list filtering

| Key | Action |
|---|---|
| `t` | Toggle the "error path" preset -- shows only the path of events leading to the failure. No-op on passing tests. |
| `m` | Toggle the "send/match" preset -- shows only `send` and `match-*` events; everything else hides. |

### Event tree

| Key | Action |
|---|---|
| `c` | Collapse all spans. |
| `x` | Expand all spans. |
| `Cmd+S` / `Ctrl+S` | Focus/cycle through search inputs on screen. |

### Inside the focused event list (after clicking it or tabbing to it)

| Key | Action |
|---|---|
| `Up` / `Down` | Move selection one row. |
| `Enter` / `Space` | Toggle the current span's expand state. |
| `Right` | Expand the selected span (no-op if already expanded). |
| `Left` | Collapse the selected span (no-op if already collapsed). |

When asking the user to do something, prefer a hotkey to a click: "press `t` to show the error path", "press `s` to open the shells inspector", "press `c` then `x` to collapse and re-expand everything".

## See also

- [events-schema](events-schema.md) -- read if you need the data shape behind what the user sees
- [events-recipes](events-recipes.md) -- read for the programmatic equivalent to anything the viewer surfaces
- [events-failures](events-failures.md) -- read if you need failure/cancellation/skip record shapes
