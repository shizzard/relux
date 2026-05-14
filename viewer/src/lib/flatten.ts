import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { ancestors, spanById, toNumber as n, type SpanId } from './derive';

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

export function flattenRows(data: StructuredLog, expandedSpans: Set<SpanId>): Row[] {
  // Filter hidden event kinds out before folding so absorbed lifecycle
  // events (shell-spawn/ready/switch) and noise (recv/string-eval/annotate)
  // never participate in the row stream. log/warning/error pass through to
  // become log-bar rows below.
  const visibleEvents = data.events.filter((ev) => !HIDDEN_EVENT_KINDS.has(ev.kind));
  const folded = foldEvents(visibleEvents);
  const rows: Row[] = [];
  const enteredSpans = new Set<SpanId>();
  let lastTs: number | null = null;

  for (const fe of folded) {
    const lead = leadEvent(fe);

    const effectiveSpanId = reattachSpanId(data, lead);
    const chain = ancestors(data, effectiveSpanId);
    if (chain.length === 0) continue;

    // Walk the chain top-down, emitting span-entry rows lazily and
    // breaking as soon as we hit a collapsed ancestor. The test span at
    // chain[0] is the page-level root (its identity is in the header
    // bar); it is never rendered as a row and is treated as implicitly
    // always-expanded.
    //
    // The event renders only if every span up to and including its own
    // span is reachable. Stopping one short would let events fire even
    // when their own shell-block / fn-call is collapsed.
    let allVisible = true;
    for (let i = 0; i < chain.length; i++) {
      const span = chain[i]!;
      const id = n(span.id);
      const isTest = span.kind === 'test';

      if (!enteredSpans.has(id)) {
        enteredSpans.add(id);
        if (isTest) {
          if (lastTs === null) lastTs = span.start_ts;
        } else {
          const ts = span.start_ts;
          if (lastTs !== null && ts - lastTs > GAP_THRESHOLD_MS) {
            rows.push({ kind: 'gap', from: lastTs, to: ts, ms: ts - lastTs });
          }
          rows.push({ kind: 'span-entry', span, depth: Math.max(0, i - 1) });
          lastTs = ts;
        }
      }

      if (!isTest && !expandedSpans.has(id)) {
        allVisible = false;
        break;
      }
    }
    if (!allVisible) continue;

    if (lastTs !== null && lead.ts - lastTs > GAP_THRESHOLD_MS) {
      rows.push({ kind: 'gap', from: lastTs, to: lead.ts, ms: lead.ts - lastTs });
    }
    // Events sit one indent deeper than their containing span (the span
    // header is at chain.length - 2 after the test offset; the event sits
    // visually inside that span at chain.length - 1).
    const depth = Math.max(0, chain.length - 1);
    const level = fe.kind === 'single' ? LOG_LEVELS[lead.kind] : undefined;
    if (level !== undefined && fe.kind === 'single') {
      rows.push({ kind: 'log-bar', level, event: fe.event, depth });
    } else {
      rows.push({ kind: 'event', folded: fe, depth });
    }
    lastTs = lead.ts;
  }

  return rows;
}
