import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { isTransparentBif } from './bif';
import { toNumber as n } from './derive';
import type { FoldedEvent } from './flatten';

export type TimeRange = { start: number; end: number; duration: number };

export type Rect = { leftPct: number; widthPct: number };

export function testTimeRange(data: StructuredLog): TimeRange {
  const spans = data.spans as unknown as Record<string, Span | undefined>;
  let testSpan: Span | null = null;
  for (const key of Object.keys(spans)) {
    const s = spans[key];
    if (s && s.kind === 'test') {
      testSpan = s;
      break;
    }
  }

  if (testSpan === null) return { start: 0, end: 0, duration: 0 };

  const start = testSpan.start_ts;
  if (testSpan.end_ts !== null) {
    const end = testSpan.end_ts;
    return { start, end, duration: end - start };
  }

  // Fallback: highest observed ts across events and span end_ts (skipping
  // spans that haven't closed yet).
  let end = start;
  for (const ev of data.events) {
    if (ev.ts > end) end = ev.ts;
  }
  for (const key of Object.keys(spans)) {
    const s = spans[key];
    if (s && s.end_ts !== null && s.end_ts > end) end = s.end_ts;
  }
  return { start, end, duration: end - start };
}

export function tsToPercent(ts: number, range: TimeRange): number {
  if (range.duration <= 0) return 0;
  const raw = ((ts - range.start) / range.duration) * 100;
  if (raw < 0) return 0;
  if (raw > 100) return 100;
  return raw;
}

function clampRect(
  startPct: number,
  endPct: number,
  minWidthPx: number,
  containerWidthPx: number,
): Rect {
  const minWidthPct =
    containerWidthPx > 0 ? (minWidthPx / containerWidthPx) * 100 : 0;
  const width = endPct - startPct;
  if (width >= minWidthPct) return { leftPct: startPct, widthPct: width };
  const mid = (startPct + endPct) / 2;
  return { leftPct: mid - minWidthPct / 2, widthPct: minWidthPct };
}

export function spanRect(
  span: Span,
  range: TimeRange,
  minWidthPx: number,
  containerWidthPx: number,
): Rect {
  const startPct = tsToPercent(span.start_ts, range);
  const endPct = tsToPercent(span.end_ts ?? range.end, range);
  return clampRect(startPct, endPct, minWidthPx, containerWidthPx);
}

export function eventRect(
  folded: FoldedEvent,
  range: TimeRange,
  minWidthPx: number,
  containerWidthPx: number,
): Rect {
  let startTs: number;
  let endTs: number;
  switch (folded.kind) {
    case 'single':
      startTs = folded.event.ts;
      endTs = folded.event.ts;
      break;
    case 'sleep':
      startTs = folded.start.ts;
      endTs = folded.done.ts;
      break;
    case 'match':
      startTs = folded.start.ts;
      endTs = folded.outcome.ts;
      break;
  }
  return clampRect(
    tsToPercent(startTs, range),
    tsToPercent(endTs, range),
    minWidthPx,
    containerWidthPx,
  );
}

export function candidateSpansAt(
  data: StructuredLog,
  cursor_ts: number,
): Span[] {
  const spansMap = data.spans as unknown as Record<string, Span | undefined>;

  // Active = non-transparent spans whose [start_ts, end_ts ?? +inf] covers cursor_ts.
  const active: Span[] = [];
  const activeIds = new Set<number>();
  for (const key of Object.keys(spansMap)) {
    const s = spansMap[key];
    if (!s) continue;
    if (isTransparentBif(s)) continue;
    const end = s.end_ts ?? Number.POSITIVE_INFINITY;
    if (s.start_ts <= cursor_ts && cursor_ts <= end) {
      active.push(s);
      activeIds.add(n(s.id));
    }
  }

  // Walk each active span's non-transparent ancestor chain; mark each
  // active ancestor as "has a deeper active descendant". A span is a
  // candidate iff it is itself active AND not marked.
  const hasDeeperActive = new Set<number>();
  for (const s of active) {
    let parentId = s.parent === null ? null : n(s.parent);
    while (parentId !== null) {
      const parent = spansMap[String(parentId)];
      if (!parent) break;
      if (!isTransparentBif(parent) && activeIds.has(parentId)) {
        hasDeeperActive.add(parentId);
      }
      parentId = parent.parent === null ? null : n(parent.parent);
    }
  }

  return active.filter((s) => !hasDeeperActive.has(n(s.id)));
}
