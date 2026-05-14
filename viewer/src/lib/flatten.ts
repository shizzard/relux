import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { ancestors, spanById, toNumber as n, type SpanId } from './derive';

export type Row =
  | { kind: 'span-entry'; span: Span; depth: number }
  | { kind: 'event'; event: Event; depth: number }
  | { kind: 'gap'; from: number; to: number; ms: number };

const GAP_THRESHOLD_MS = 500;

// shell-spawn / shell-ready are surfaced as properties on the owning
// shell-block span (see `shellBlockProps`); shell-switch is redundant
// with the shell-block span entry (the row title already names the shell).
// effect-expose-* events are surfaced as inline props on the owning
// effect-setup span (see `effectSetupProps`).
// None of them appear as timeline rows. The runtime continues to emit
// shell-switch because it doubles as a progress-stream signal that drives
// the live TUI's `|` indicator.
// shell-spawn / shell-ready / shell-switch / shell-terminate are surfaced as
// first-class event rows per the design. The effect-expose-* events stay
// hidden — they're rendered as inline props on the owning effect-setup
// span (see `effectSetupProps`).
const HIDDEN_EVENT_KINDS: ReadonlySet<Event['kind']> = new Set([
  'effect-expose-shell',
  'effect-expose-var',
]);

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

export function flattenRows(data: StructuredLog, expandedSpans: Set<SpanId>): Row[] {
  const rows: Row[] = [];
  const enteredSpans = new Set<SpanId>();
  let lastTs: number | null = null;

  for (const event of data.events) {
    if (HIDDEN_EVENT_KINDS.has(event.kind)) continue;
    const effectiveSpanId = reattachSpanId(data, event);
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

    if (lastTs !== null && event.ts - lastTs > GAP_THRESHOLD_MS) {
      rows.push({ kind: 'gap', from: lastTs, to: event.ts, ms: event.ts - lastTs });
    }
    // Events sit one indent deeper than their containing span (the span
    // header is at chain.length - 2 after the test offset; the event sits
    // visually inside that span at chain.length - 1).
    rows.push({ kind: 'event', event, depth: Math.max(0, chain.length - 1) });
    lastTs = event.ts;
  }

  return rows;
}
