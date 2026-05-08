import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { ancestors, toNumber as n, type SpanId } from './derive';

export type Row =
  | { kind: 'span-entry'; span: Span; depth: number }
  | { kind: 'event'; event: Event; depth: number }
  | { kind: 'gap'; from: number; to: number; ms: number };

const GAP_THRESHOLD_MS = 500;

export function flattenRows(data: StructuredLog, expandedSpans: Set<SpanId>): Row[] {
  const rows: Row[] = [];
  const enteredSpans = new Set<SpanId>();
  let lastTs: number | null = null;

  for (const event of data.events) {
    const chain = ancestors(data, n(event.span));
    if (chain.length === 0) continue;

    // The event renders only when every ancestor (and the event's own span)
    // is reachable through `expandedSpans`. The root is always reachable.
    let allVisible = true;
    for (let i = 0; i < chain.length - 1; i++) {
      const parent = chain[i]!;
      if (!expandedSpans.has(n(parent.id))) {
        allVisible = false;
        break;
      }
    }
    if (!allVisible) continue;

    // Emit any not-yet-entered span-entry rows along the visible chain.
    for (let i = 0; i < chain.length; i++) {
      const span = chain[i]!;
      const id = n(span.id);
      if (enteredSpans.has(id)) continue;
      const ts = span.start_ts;
      if (lastTs !== null && ts - lastTs > GAP_THRESHOLD_MS) {
        rows.push({ kind: 'gap', from: lastTs, to: ts, ms: ts - lastTs });
      }
      rows.push({ kind: 'span-entry', span, depth: i });
      enteredSpans.add(id);
      lastTs = ts;
    }

    if (lastTs !== null && event.ts - lastTs > GAP_THRESHOLD_MS) {
      rows.push({ kind: 'gap', from: lastTs, to: event.ts, ms: event.ts - lastTs });
    }
    rows.push({ kind: 'event', event, depth: chain.length - 1 });
    lastTs = event.ts;
  }

  return rows;
}
