import { describe, expect, it } from 'vitest';
import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import type { FoldedEvent } from './flatten';
import {
  candidateSpansAt,
  eventRect,
  spanRect,
  testTimeRange,
  tsToPercent,
} from './timeline';

function makeLog(spans: Span[], eventTs: number[]): StructuredLog {
  const spansMap: Record<string, Span> = {};
  for (const s of spans) spansMap[String(s.id)] = s;
  const events = eventTs.map(
    (ts, i) =>
      ({
        seq: BigInt(i + 1),
        ts,
        span: spans[0]?.id ?? BigInt(0),
        shell: null,
        shell_marker: null,
        source: null,
        kind: 'send',
        data: '',
      }) as unknown as StructuredLog['events'][number],
  );
  return {
    info: { name: 't', path: 'p', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: spansMap as unknown as StructuredLog['spans'],
    events,
    buffer_events: [],
    outcome: { kind: 'pass' },
    artifacts: [],
    sources: {},
  } as unknown as StructuredLog;
}

function testSpan(id: number, start_ts: number, end_ts: number | null): Span {
  return {
    id: BigInt(id),
    parent: null,
    start_ts,
    end_ts,
    location: null,
    kind: 'test',
    name: 't',
  } as Span;
}

describe('testTimeRange', () => {
  it('uses test span start_ts and end_ts when both present', () => {
    const log = makeLog([testSpan(1, 100, 1000)], []);
    expect(testTimeRange(log)).toEqual({ start: 100, end: 1000, duration: 900 });
  });

  it('falls back to max event ts when test span end_ts is null', () => {
    const log = makeLog([testSpan(1, 100, null)], [200, 800, 500]);
    expect(testTimeRange(log)).toEqual({ start: 100, end: 800, duration: 700 });
  });

  it('returns zero-duration when no test span and no events', () => {
    const log = makeLog([], []);
    expect(testTimeRange(log)).toEqual({ start: 0, end: 0, duration: 0 });
  });
});

describe('tsToPercent', () => {
  const range = { start: 100, end: 1100, duration: 1000 };

  it('returns 0 at start', () => {
    expect(tsToPercent(100, range)).toBe(0);
  });

  it('returns 100 at end', () => {
    expect(tsToPercent(1100, range)).toBe(100);
  });

  it('returns 50 at midpoint', () => {
    expect(tsToPercent(600, range)).toBe(50);
  });

  it('clamps below start to 0', () => {
    expect(tsToPercent(50, range)).toBe(0);
  });

  it('clamps above end to 100', () => {
    expect(tsToPercent(2000, range)).toBe(100);
  });

  it('returns 0 when duration is zero', () => {
    expect(tsToPercent(123, { start: 0, end: 0, duration: 0 })).toBe(0);
  });
});

function shellBlockSpan(id: number, start_ts: number, end_ts: number | null): Span {
  return {
    id: BigInt(id),
    parent: BigInt(1),
    start_ts,
    end_ts,
    location: null,
    kind: 'shell-block',
    shell: 's',
  } as Span;
}

function sendEvent(seq: number, ts: number, span = 1): Event {
  return {
    seq: BigInt(seq),
    ts,
    span: BigInt(span),
    shell: null,
    shell_marker: null,
    source: null,
    kind: 'send',
    data: '',
  } as unknown as Event;
}

function matchStart(seq: number, ts: number, span = 1): Event {
  return {
    seq: BigInt(seq),
    ts,
    span: BigInt(span),
    shell: 's',
    shell_marker: null,
    source: null,
    kind: 'match-start',
    pattern: 'p',
    is_regex: false,
    effective: { type: 'assertion', duration: '1s', source: null },
  } as unknown as Event;
}

function matchDone(seq: number, ts: number, span = 1): Event {
  return {
    seq: BigInt(seq),
    ts,
    span: BigInt(span),
    shell: 's',
    shell_marker: null,
    source: null,
    kind: 'match-done',
    matched: 'p',
    elapsed: 1,
    captures: null,
    buffer_seq: BigInt(seq),
  } as unknown as Event;
}

describe('spanRect', () => {
  const range = { start: 0, end: 1000, duration: 1000 };

  it('renders a span occupying the full range', () => {
    const span = shellBlockSpan(2, 0, 1000);
    const rect = spanRect(span, range, 3, 1000);
    expect(rect.leftPct).toBeCloseTo(0);
    expect(rect.widthPct).toBeCloseTo(100);
  });

  it('renders a span occupying the middle 20%', () => {
    const span = shellBlockSpan(2, 400, 600);
    const rect = spanRect(span, range, 3, 1000);
    expect(rect.leftPct).toBeCloseTo(40);
    expect(rect.widthPct).toBeCloseTo(20);
  });

  it('clamps a 1ms span to the 3px minimum width, centered', () => {
    const span = shellBlockSpan(2, 500, 501);
    const rect = spanRect(span, range, 3, 1000);
    expect(rect.widthPct).toBeCloseTo(0.3);
    expect(rect.leftPct).toBeCloseTo(50.05 - 0.15);
  });

  it('treats an unclosed span (end_ts null) as ending at range.end', () => {
    const span = shellBlockSpan(2, 250, null);
    const rect = spanRect(span, range, 3, 1000);
    expect(rect.leftPct).toBeCloseTo(25);
    expect(rect.widthPct).toBeCloseTo(75);
  });
});

describe('eventRect', () => {
  const range = { start: 0, end: 1000, duration: 1000 };

  it('renders a folded match as a slice from start.ts to outcome.ts', () => {
    const folded: FoldedEvent = {
      kind: 'match',
      start: matchStart(1, 200),
      outcome: matchDone(2, 700),
    };
    const rect = eventRect(folded, range, 3, 1000);
    expect(rect.leftPct).toBeCloseTo(20);
    expect(rect.widthPct).toBeCloseTo(50);
  });

  it('renders a single event as a 3px minimum box centered on ts', () => {
    const folded: FoldedEvent = { kind: 'single', event: sendEvent(1, 500) };
    const rect = eventRect(folded, range, 3, 1000);
    expect(rect.widthPct).toBeCloseTo(0.3);
    expect(rect.leftPct).toBeCloseTo(50 - 0.15);
  });
});

function bifFnCallSpan(
  id: number,
  parent: number,
  start_ts: number,
  end_ts: number | null,
  name = 'trim',
  is_pure = true,
): Span {
  return {
    id: BigInt(id),
    parent: BigInt(parent),
    start_ts,
    end_ts,
    location: null,
    kind: 'fn-call',
    name,
    args: [],
    result: null,
    callee_kind: 'bif',
    is_pure,
  } as Span;
}

function userFnCallSpan(
  id: number,
  parent: number,
  start_ts: number,
  end_ts: number | null,
): Span {
  return {
    id: BigInt(id),
    parent: BigInt(parent),
    start_ts,
    end_ts,
    location: null,
    kind: 'fn-call',
    name: 'f',
    args: [],
    result: null,
    callee_kind: 'user',
    is_pure: false,
  } as Span;
}

function effectCleanupSpan(
  id: number,
  parent: number,
  start_ts: number,
  end_ts: number | null,
): Span {
  return {
    id: BigInt(id),
    parent: BigInt(parent),
    start_ts,
    end_ts,
    location: null,
    kind: 'effect-cleanup',
    effect: 'e',
    alias: null,
    setup_span: BigInt(0),
    marker: 'm',
    is_deferred: false,
  } as Span;
}

function logWith(spans: Span[]): StructuredLog {
  const map: Record<string, Span> = {};
  for (const s of spans) map[String(s.id)] = s;
  return {
    info: { name: 't', path: 'p', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: map as unknown as StructuredLog['spans'],
    events: [],
    buffer_events: [],
    outcome: { kind: 'pass' },
    artifacts: [],
    sources: {},
  } as unknown as StructuredLog;
}

describe('candidateSpansAt', () => {
  it('returns the deepest non-transparent span in a serial chain', () => {
    const log = logWith([
      testSpan(1, 0, 1000),
      shellBlockSpan(2, 100, 900),
      userFnCallSpan(3, 2, 100, 500),
    ]);
    const result = candidateSpansAt(log, 200);
    expect(result.map((s) => Number(s.id))).toEqual([3]);
  });

  it('skips transparent BIFs and returns the next non-transparent ancestor as the leaf', () => {
    const log = logWith([
      testSpan(1, 0, 1000),
      shellBlockSpan(2, 100, 900),
      bifFnCallSpan(3, 2, 200, 400, 'trim', true),
    ]);
    const result = candidateSpansAt(log, 300);
    // BIF at id=3 is transparent. shell-block at id=2 has no other
    // non-transparent descendants at ts=300, so it becomes the leaf.
    expect(result.map((s) => Number(s.id))).toEqual([2]);
  });

  it('returns both leaves when sibling spans run concurrently', () => {
    const log = logWith([
      testSpan(1, 0, 1000),
      effectCleanupSpan(2, 1, 700, 900),
      effectCleanupSpan(3, 1, 720, 880),
    ]);
    const result = candidateSpansAt(log, 800);
    expect(result.map((s) => Number(s.id)).sort()).toEqual([2, 3]);
  });

  it('returns the parent when cursor falls between sibling spans', () => {
    const log = logWith([
      testSpan(1, 0, 1000),
      shellBlockSpan(2, 100, 200),
      shellBlockSpan(3, 400, 500),
    ]);
    const result = candidateSpansAt(log, 300);
    expect(result.map((s) => Number(s.id))).toEqual([1]);
  });

  it('returns empty when cursor is outside the test span', () => {
    const log = logWith([testSpan(1, 100, 900)]);
    expect(candidateSpansAt(log, 50)).toEqual([]);
    expect(candidateSpansAt(log, 950)).toEqual([]);
  });
});
