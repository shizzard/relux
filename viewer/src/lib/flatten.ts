import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { spanById, toNumber as n, type SpanId } from './derive';

// A FoldedEvent is either a single Event (the common case) or a deterministic
// pair of adjacent events whose halves carry no information the other
// didn't already imply. The runtime still emits both halves for streaming
// correctness; folding happens at the viewer layer only.
export type FoldedEvent =
  | { kind: 'single'; event: Event }
  | { kind: 'sleep'; start: Event; done: Event }
  | { kind: 'match'; start: Event; outcome: Event };

export type LogLevel = 'log' | 'warning' | 'error';

export type Row =
  | { kind: 'span-entry'; span: Span; depth: number }
  | { kind: 'event'; folded: FoldedEvent; depth: number }
  | { kind: 'log-bar'; level: LogLevel; event: Event; depth: number }
  | { kind: 'gap'; from: number; to: number; ms: number };

const GAP_THRESHOLD_MS = 500;

// Events that never reach the timeline. `effect-expose-*` surface as inline
// props on the owning `effect-setup` span (see `effectSetupProps`).
// `shell-spawn` / `shell-ready` / `shell-switch` are absorbed into the
// containing `shell-block` span card. `recv` and `string-eval` and
// `annotate` are filtered for signal-to-noise. `log` / `warning` /
// `error` produce passive `log-bar` rows instead of regular event rows.
const HIDDEN_EVENT_KINDS: ReadonlySet<Event['kind']> = new Set([
  'effect-expose-shell',
  'effect-expose-var',
  'shell-spawn',
  'shell-ready',
  'shell-switch',
  'recv',
  'string-eval',
  'annotate',
]);

const LOG_LEVELS: Partial<Record<Event['kind'], LogLevel>> = {
  log: 'log',
  warning: 'warning',
  error: 'error',
};

// Span kinds that "own" shell lifecycles for placement purposes.
// shell-terminate fires from within the last shell-block the VM was
// in, but conceptually it belongs to the test or effect that ended.
const OWNER_KINDS: ReadonlySet<Span['kind']> = new Set([
  'test',
  'effect-setup',
  'effect-cleanup',
]);

function reattachSpanId(data: StructuredLog, event: Event): SpanId {
  const original = n(event.span);
  if (event.kind !== 'shell-terminate') return original;
  let cursor: Span | null = spanById(data, original);
  while (cursor) {
    if (OWNER_KINDS.has(cursor.kind)) return n(cursor.id);
    cursor = cursor.parent === null ? null : spanById(data, n(cursor.parent));
  }
  return original;
}

function sameSpan(a: Event, b: Event): boolean {
  return n(a.span) === n(b.span);
}

function sameShell(a: Event, b: Event): boolean {
  return a.shell === b.shell;
}

export function leadEvent(f: FoldedEvent): Event {
  switch (f.kind) {
    case 'single':
      return f.event;
    case 'sleep':
      return f.start;
    case 'match':
      return f.start;
  }
}

export function foldedSeqs(f: FoldedEvent): number[] {
  switch (f.kind) {
    case 'single':
      return [n(f.event.seq)];
    case 'sleep':
      return [n(f.start.seq), n(f.done.seq)];
    case 'match':
      return [n(f.start.seq), n(f.outcome.seq)];
  }
}

// Given an index into `events`, returns the index of the close half of the
// fold that starts there (sleep-done, match-done/timeout, shell-ready).
// Unlike `foldEvents`, which requires strict adjacency to merge rows
// visually, this helper scans forward by *kind* within the same span
// (and shell, where the event kind carries one) so it stays correct if
// future code paths inject other events (log, fail-pattern-triggered,
// recv, ...) between the open and the close. Returns `startIdx` when the
// event there does not open a fold, or when no matching close is found.
export function foldCloseIndex(events: readonly Event[], startIdx: number): number {
  const e = events[startIdx];
  if (!e) return startIdx;
  switch (e.kind) {
    case 'sleep-start':
      for (let i = startIdx + 1; i < events.length; i++) {
        const c = events[i]!;
        if (c.kind === 'sleep-done' && sameSpan(e, c)) return i;
      }
      return startIdx;
    case 'match-start':
      for (let i = startIdx + 1; i < events.length; i++) {
        const c = events[i]!;
        if (
          (c.kind === 'match-done' || c.kind === 'timeout') &&
          sameSpan(e, c) &&
          sameShell(e, c)
        ) {
          return i;
        }
      }
      return startIdx;
    default:
      return startIdx;
  }
}

export function foldEvents(events: readonly Event[]): FoldedEvent[] {
  const out: FoldedEvent[] = [];
  for (let i = 0; i < events.length; i++) {
    const ev = events[i]!;
    if (ev.kind === 'sleep-start') {
      const next = events[i + 1];
      if (next && next.kind === 'sleep-done' && sameSpan(ev, next)) {
        out.push({ kind: 'sleep', start: ev, done: next });
        i++;
        continue;
      }
    } else if (ev.kind === 'match-start') {
      const next = events[i + 1];
      if (
        next &&
        (next.kind === 'match-done' || next.kind === 'timeout') &&
        sameSpan(ev, next) &&
        sameShell(ev, next)
      ) {
        out.push({ kind: 'match', start: ev, outcome: next });
        i++;
        continue;
      }
    }
    out.push({ kind: 'single', event: ev });
  }
  return out;
}

// flattenRows: tree-driven flattening.
//
// Walks the span tree in DFS order rooted at the test span, sorting
// children by `start_ts`. At each span, direct events (those whose
// post-reattach span IS this span) and child spans are merged by
// timestamp and emitted in chronological order. Events are emitted as
// `event` or `log-bar` rows; child spans are emitted as `span-entry`
// rows followed (if expanded) by their own contents at one deeper
// indent.
//
// This replaces an earlier event-seq-driven pass that lazily emitted
// span-entry rows on first event visit. The old pass produced correct
// nesting whenever event order happened to follow tree order (the case
// for sequential effect setup) but broke for concurrently-running
// children (the diamond-cleanup case where E1 and E2 cleanup events
// interleave in seq and E0's cleanup runs after both). The tree-driven
// pass uses parent links directly, so nesting is correct regardless of
// event interleaving.
//
// Zero-duration "marker-only" spans (`effect-setup { is_reuse: true }`
// and `effect-cleanup { is_deferred: true }`) need no special handling
// — they appear in the tree as ordinary children and slot into the
// merged ts ordering naturally.
export function flattenRows(data: StructuredLog, expandedSpans: Set<SpanId>): Row[] {
  const visibleEvents = data.events.filter((ev) => !HIDDEN_EVENT_KINDS.has(ev.kind));
  const folded = foldEvents(visibleEvents);

  // Index folded events by their effective (post-reattach) span.
  const eventsBySpan = new Map<SpanId, FoldedEvent[]>();
  for (const fe of folded) {
    const lead = leadEvent(fe);
    const sid = reattachSpanId(data, lead);
    let bucket = eventsBySpan.get(sid);
    if (!bucket) {
      bucket = [];
      eventsBySpan.set(sid, bucket);
    }
    bucket.push(fe);
  }
  // Within each bucket events are already in seq order (folded preserves
  // it), which is also non-decreasing by ts; no further sort needed.

  // Index children by parent, sorted by start_ts.
  const childrenByParent = new Map<SpanId, Span[]>();
  const spanMap = data.spans as unknown as Record<string, Span | undefined>;
  let testSpan: Span | null = null;
  for (const key of Object.keys(spanMap)) {
    const span = spanMap[key];
    if (!span) continue;
    if (span.kind === 'test' && testSpan === null) testSpan = span;
    if (span.parent === null) continue;
    const pid = n(span.parent);
    let bucket = childrenByParent.get(pid);
    if (!bucket) {
      bucket = [];
      childrenByParent.set(pid, bucket);
    }
    bucket.push(span);
  }
  for (const bucket of childrenByParent.values()) {
    bucket.sort((a, b) => a.start_ts - b.start_ts);
  }
  if (testSpan === null) return [];

  const rows: Row[] = [];
  let lastTs: number | null = testSpan.start_ts;

  function maybeGap(ts: number): void {
    if (lastTs !== null && ts - lastTs > GAP_THRESHOLD_MS) {
      rows.push({ kind: 'gap', from: lastTs, to: ts, ms: ts - lastTs });
    }
  }

  function emitEvent(fe: FoldedEvent, depth: number): void {
    const lead = leadEvent(fe);
    maybeGap(lead.ts);
    const level = fe.kind === 'single' ? LOG_LEVELS[lead.kind] : undefined;
    if (level !== undefined && fe.kind === 'single') {
      rows.push({ kind: 'log-bar', level, event: fe.event, depth });
    } else {
      rows.push({ kind: 'event', folded: fe, depth });
    }
    lastTs = lead.ts;
  }

  function emitSpanContents(span: Span, depth: number): void {
    const events = eventsBySpan.get(n(span.id)) ?? [];
    const children = childrenByParent.get(n(span.id)) ?? [];
    let ei = 0;
    let ci = 0;
    while (ei < events.length || ci < children.length) {
      const eTs = ei < events.length ? leadEvent(events[ei]!).ts : Infinity;
      const cTs = ci < children.length ? children[ci]!.start_ts : Infinity;
      // Ties: child span before its same-ts events (spans open at the
      // instant before any of their inner activity).
      if (cTs <= eTs) {
        emitChildSpan(children[ci]!, depth);
        ci++;
      } else {
        emitEvent(events[ei]!, depth);
        ei++;
      }
    }
  }

  function emitChildSpan(span: Span, depth: number): void {
    maybeGap(span.start_ts);
    rows.push({ kind: 'span-entry', span, depth });
    lastTs = span.start_ts;
    if (expandedSpans.has(n(span.id))) {
      emitSpanContents(span, depth + 1);
    }
  }

  // Test span is the page-level root: never rendered as a row,
  // implicitly always-expanded. Its direct children are at depth 0.
  emitSpanContents(testSpan, 0);

  return rows;
}
