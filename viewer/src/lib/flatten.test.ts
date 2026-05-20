import { describe, expect, it } from 'vitest';
import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { flattenRows, type FoldedEvent, type Row } from './flatten';

// ─── Span builders ──────────────────────────────────────────────────

type SpanInput = { id: number; parent: number | null; start_ts?: number; end_ts?: number | null } & (
  | { kind: 'test'; name?: string }
  | {
      kind: 'effect-setup';
      effect: string;
      alias?: string | null;
      marker?: string;
      is_reuse?: boolean;
    }
  | {
      kind: 'effect-cleanup';
      effect: string;
      alias?: string | null;
      setup_span?: number;
      marker?: string;
      is_deferred?: boolean;
    }
  | { kind: 'cleanup-block' }
  | { kind: 'shell-block'; shell: string }
  | {
      kind: 'fn-call';
      name?: string;
      args?: Array<[string, string]>;
      result?: string | null;
      callee_kind?: 'user' | 'bif';
      is_pure?: boolean;
    }
);

function buildSpan(input: SpanInput): Span {
  const base = {
    id: BigInt(input.id),
    parent: input.parent === null ? null : BigInt(input.parent),
    start_ts: input.start_ts ?? 0,
    end_ts: input.end_ts ?? null,
    location: null,
  } as const;
  switch (input.kind) {
    case 'test':
      return { ...base, kind: 'test', name: input.name ?? 't' };
    case 'effect-setup':
      return {
        ...base,
        kind: 'effect-setup',
        effect: input.effect,
        overlay: [],
        alias: input.alias ?? null,
        marker: input.marker ?? 'test-marker-0000',
        is_reuse: input.is_reuse ?? false,
      };
    case 'effect-cleanup':
      return {
        ...base,
        kind: 'effect-cleanup',
        effect: input.effect,
        alias: input.alias ?? null,
        setup_span: BigInt(input.setup_span ?? 0),
        marker: input.marker ?? 'test-marker-0000',
        is_deferred: input.is_deferred ?? false,
      };
    case 'cleanup-block':
      return { ...base, kind: 'cleanup-block' };
    case 'shell-block':
      return { ...base, kind: 'shell-block', shell: input.shell };
    case 'fn-call':
      return {
        ...base,
        kind: 'fn-call',
        name: input.name ?? 'f',
        args: input.args ?? [],
        result: input.result ?? null,
        callee_kind: input.callee_kind ?? 'user',
        is_pure: input.is_pure ?? false,
      };
  }
}

function spansToMap(spans: Span[]): Record<string, Span> {
  const out: Record<string, Span> = {};
  for (const s of spans) out[String(s.id)] = s;
  return out;
}

// ─── Event builders ─────────────────────────────────────────────────

function send(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    kind: 'send',
    data: 'x',
  } as Event;
}
function matchStart(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    kind: 'match-start',
    pattern: 'p',
    is_regex: false,
    effective: { type: 'assertion', duration: '1s', source: null },
  } as Event;
}
function matchDone(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    kind: 'match-done',
    matched: 'p',
    elapsed: 1,
    captures: null,
    buffer_seq: BigInt(seq),
  } as Event;
}
function shellSpawn(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    kind: 'shell-spawn',
    name: shell,
    command: '/bin/sh',
  } as Event;
}
function shellTerminate(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    kind: 'shell-terminate',
    name: shell,
  } as Event;
}
function logEv(seq: number, span: number, msg = 'log'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell: null,
    kind: 'log',
    message: msg,
  } as Event;
}
function annotateEv(seq: number, span: number, text = 'note'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell: null,
    kind: 'annotate',
    text,
  } as Event;
}
function sleepStart(seq: number, span: number, ms = 100): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell: null,
    kind: 'sleep-start',
    duration: ms,
  } as Event;
}
function sleepDone(seq: number, span: number): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell: null,
    kind: 'sleep-done',
  } as Event;
}
function warningEv(seq: number, span: number, msg = 'warn'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell: null,
    kind: 'warning',
    message: msg,
  } as Event;
}

function logWith(spans: Span[], events: Event[]): StructuredLog {
  return {
    test: { name: 't', path: 'p', outcome: 'pass', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: spansToMap(spans),
    events,
    buffer_events: [],
    failure: null,
    sources: {},
  } as unknown as StructuredLog;
}

// ─── Helpers for assertions ────────────────────────────────────────

type RowSummary =
  | { kind: 'span'; id: number; depth: number }
  | { kind: 'event'; eventSeq: number; depth: number }
  | { kind: 'log-bar'; level: string; eventSeq: number; depth: number }
  | { kind: 'bif-row'; id: number; depth: number }
  | { kind: 'gap'; ms: number };

function summarize(rows: Row[]): RowSummary[] {
  return rows.map((r): RowSummary => {
    switch (r.kind) {
      case 'span-entry':
        return { kind: 'span', id: Number(r.span.id), depth: r.depth };
      case 'event':
        return { kind: 'event', eventSeq: leadSeq(r.folded), depth: r.depth };
      case 'log-bar':
        return { kind: 'log-bar', level: r.level, eventSeq: Number(r.event.seq), depth: r.depth };
      case 'bif-row':
        return { kind: 'bif-row', id: Number(r.span.id), depth: r.depth };
      case 'gap':
        return { kind: 'gap', ms: r.ms };
    }
  });
}

function leadSeq(f: FoldedEvent): number {
  if (f.kind === 'single') return Number(f.event.seq);
  if (f.kind === 'sleep') return Number(f.start.seq);
  return Number(f.start.seq);
}

// ─── Tests ─────────────────────────────────────────────────────────

describe('flattenRows', () => {
  it('returns empty rows when log has no test span', () => {
    expect(flattenRows(logWith([], []), new Set())).toEqual([]);
  });

  it('returns empty rows when test span has no children or events', () => {
    const log = logWith([buildSpan({ id: 1, parent: null, kind: 'test' })], []);
    expect(flattenRows(log, new Set())).toEqual([]);
  });

  it('emits a direct event of the test span at depth 0', () => {
    const log = logWith(
      [buildSpan({ id: 1, parent: null, kind: 'test' })],
      [send(5, 1)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'event', eventSeq: 5, depth: 0 },
    ]);
  });

  it('emits a child span at depth 0 even when collapsed', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [send(5, 2)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
    ]);
  });

  it('emits a child span and its events when the child is expanded', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [send(5, 2)],
    );
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'event', eventSeq: 5, depth: 1 },
    ]);
  });

  it('orders sibling child spans by start_ts, not by id', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 3, parent: 1, kind: 'shell-block', shell: 'a', start_ts: 1 }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 'b', start_ts: 5 }),
      ],
      [],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 3, depth: 0 },
      { kind: 'span', id: 2, depth: 0 },
    ]);
  });

  it('interleaves direct events with child spans by ts', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 3 }),
      ],
      [send(1, 1), send(5, 1)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'event', eventSeq: 1, depth: 0 },
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'event', eventSeq: 5, depth: 0 },
    ]);
  });

  it('orders a span and its same-ts event with the span first', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 5 }),
      ],
      [send(5, 1)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'event', eventSeq: 5, depth: 0 },
    ]);
  });

  it('hides events of a collapsed child span but still emits the span row', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [send(5, 2), send(6, 2)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
    ]);
  });

  it('filters hidden event kinds (shell-spawn) from the row stream', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [shellSpawn(2, 2), send(3, 2)],
    );
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'event', eventSeq: 3, depth: 1 },
    ]);
  });

  it('folds match-start + match-done into a single row at the start seq', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [matchStart(3, 2), matchDone(4, 2)],
    );
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'event', eventSeq: 3, depth: 1 },
    ]);
  });

  it('renders log/warning events as log-bar rows', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
      ],
      [logEv(3, 2), warningEv(4, 2)],
    );
    const rows = summarize(flattenRows(log, new Set([2])));
    expect(rows).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'log-bar', level: 'log', eventSeq: 3, depth: 1 },
      { kind: 'log-bar', level: 'warning', eventSeq: 4, depth: 1 },
    ]);
  });

  it('inserts a gap row when ts delta between adjacent rows exceeds the threshold', () => {
    const log = logWith(
      [buildSpan({ id: 1, parent: null, kind: 'test' })],
      [send(0, 1), send(1000, 1)],
    );
    const rows = summarize(flattenRows(log, new Set()));
    expect(rows).toEqual([
      { kind: 'event', eventSeq: 0, depth: 0 },
      { kind: 'gap', ms: 1000 },
      { kind: 'event', eventSeq: 1000, depth: 0 },
    ]);
  });

  it('does not insert a gap when ts delta is at or under the threshold', () => {
    const log = logWith(
      [buildSpan({ id: 1, parent: null, kind: 'test' })],
      [send(0, 1), send(500, 1)],
    );
    const rows = summarize(flattenRows(log, new Set()));
    expect(rows).toEqual([
      { kind: 'event', eventSeq: 0, depth: 0 },
      { kind: 'event', eventSeq: 500, depth: 0 },
    ]);
  });

  it('reattaches shell-terminate to the effect-cleanup owner', () => {
    // shell-terminate fires from inside a cleanup-block; the reattach rule
    // moves it up to the enclosing effect-cleanup so the event renders
    // under that cleanup span's depth.
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'effect-cleanup', effect: 'E', start_ts: 1 }),
        buildSpan({ id: 3, parent: 2, kind: 'cleanup-block', start_ts: 2 }),
      ],
      [shellTerminate(5, 3)],
    );
    // The terminate becomes a direct event of span 2 (effect-cleanup),
    // so it shows at depth 1 once span 2 is expanded. Without expansion
    // only the cleanup-row appears.
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'span', id: 3, depth: 1 },
      { kind: 'event', eventSeq: 5, depth: 1 },
    ]);
  });

  it('emits zero-duration is_reuse setup as a child of its parent', () => {
    // E2.setup contains a single dedup'd acquire of E0 — no events of
    // its own. It must still appear as a child row.
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'effect-setup', effect: 'E2', start_ts: 1 }),
        buildSpan({
          id: 3,
          parent: 2,
          kind: 'effect-setup',
          effect: 'E0',
          is_reuse: true,
          start_ts: 2,
          end_ts: 2,
        }),
      ],
      [],
    );
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'span', id: 3, depth: 1 },
    ]);
  });

  it('emits zero-duration is_deferred cleanup as a child of its parent', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'effect-cleanup', effect: 'E1', start_ts: 1 }),
        buildSpan({
          id: 3,
          parent: 2,
          kind: 'effect-cleanup',
          effect: 'E0',
          is_deferred: true,
          start_ts: 2,
          end_ts: 2,
        }),
      ],
      [],
    );
    expect(summarize(flattenRows(log, new Set([2])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'span', id: 3, depth: 1 },
    ]);
  });

  it('nests each diamond cleanup branch under its own parent', () => {
    // Diamond cleanup: test -> {E1.cleanup, E2.cleanup}; E1.cleanup
    // contains the deferred E0; E2.cleanup contains the final E0.
    // Events fire concurrently, but the tree-driven flatten must keep
    // each branch in its own subtree.
    //
    //   id   parent  kind                      effect  flag
    //   1    null    test
    //   10   1       effect-cleanup            E1
    //   11   10      cleanup-block
    //   12   1       effect-cleanup            E2
    //   13   12      cleanup-block
    //   14   10      effect-cleanup (deferred) E0
    //   15   12      effect-cleanup            E0
    //   16   15      cleanup-block
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 10, parent: 1, kind: 'effect-cleanup', effect: 'E1', start_ts: 1 }),
        buildSpan({ id: 11, parent: 10, kind: 'cleanup-block', start_ts: 1 }),
        buildSpan({ id: 12, parent: 1, kind: 'effect-cleanup', effect: 'E2', start_ts: 2 }),
        buildSpan({ id: 13, parent: 12, kind: 'cleanup-block', start_ts: 2 }),
        buildSpan({
          id: 14,
          parent: 10,
          kind: 'effect-cleanup',
          effect: 'E0',
          is_deferred: true,
          start_ts: 5,
          end_ts: 5,
        }),
        buildSpan({ id: 15, parent: 12, kind: 'effect-cleanup', effect: 'E0', start_ts: 6 }),
        buildSpan({ id: 16, parent: 15, kind: 'cleanup-block', start_ts: 6 }),
      ],
      [
        // Interleave events between E1's cleanup-block and E2's
        // cleanup-block, the way concurrent join_all produces them.
        send(3, 11), // E1 cleanup-block
        send(4, 13), // E2 cleanup-block
        send(7, 16), // E0 cleanup-block
      ],
    );
    // Default: only test expanded — top-level cleanup rows only.
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 10, depth: 0 },
      { kind: 'span', id: 12, depth: 0 },
    ]);
    // Expand only E1.cleanup — its cleanup-block and the deferred E0
    // render under it; E2's tree stays collapsed; no E0 rows leak under
    // E2.
    expect(summarize(flattenRows(log, new Set([10])))).toEqual([
      { kind: 'span', id: 10, depth: 0 },
      { kind: 'span', id: 11, depth: 1 },
      { kind: 'span', id: 14, depth: 1 },
      { kind: 'span', id: 12, depth: 0 },
    ]);
    // Expand only E2.cleanup — its cleanup-block and the final E0 render
    // under it; E1's tree stays collapsed; no deferred row appears under
    // E2.
    expect(summarize(flattenRows(log, new Set([12])))).toEqual([
      { kind: 'span', id: 10, depth: 0 },
      { kind: 'span', id: 12, depth: 0 },
      { kind: 'span', id: 13, depth: 1 },
      { kind: 'span', id: 15, depth: 1 },
    ]);
    // Expand both top-level cleanups. Each effect's children stay in
    // their own branch.
    expect(summarize(flattenRows(log, new Set([10, 12])))).toEqual([
      { kind: 'span', id: 10, depth: 0 },
      { kind: 'span', id: 11, depth: 1 },
      { kind: 'span', id: 14, depth: 1 },
      { kind: 'span', id: 12, depth: 0 },
      { kind: 'span', id: 13, depth: 1 },
      { kind: 'span', id: 15, depth: 1 },
    ]);
  });

  it('folds match-start/done across same-span lookahead, not strict adjacency', () => {
    // Diamond cleanup: two cleanup-block shells run concurrently and
    // their events interleave in the global array. Each cleanup wraps a
    // match_ok fn-call. A match-start in span A may be followed by
    // unrelated events from span B before its own match-done arrives.
    // The fold must use same-span lookahead so the pair still collapses
    // into a single row.
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 10, parent: 1, kind: 'effect-cleanup', effect: 'A', start_ts: 1 }),
        buildSpan({ id: 11, parent: 10, kind: 'cleanup-block', start_ts: 1 }),
        buildSpan({
          id: 12,
          parent: 11,
          kind: 'fn-call',
          name: 'match_ok',
          start_ts: 2,
          callee_kind: 'bif',
          is_pure: false,
        }),
        buildSpan({ id: 20, parent: 1, kind: 'effect-cleanup', effect: 'B', start_ts: 1 }),
        buildSpan({ id: 21, parent: 20, kind: 'cleanup-block', start_ts: 1 }),
        buildSpan({
          id: 22,
          parent: 21,
          kind: 'fn-call',
          name: 'match_ok',
          start_ts: 2,
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [
        // A opens its match…
        matchStart(3, 12, 'a'),
        // …B does an entire send+match in between…
        send(4, 21, 'b'),
        matchStart(5, 22, 'b'),
        matchDone(6, 22, 'b'),
        // …and only now does A's close arrive.
        matchDone(7, 12, 'a'),
      ],
    );
    // Both branches expanded; each match collapses to one event row
    // under its fn-call span. Before the fix, A's match-start emitted as
    // a single row and A's match-done as a separate row.
    expect(summarize(flattenRows(log, new Set([10, 11, 12, 20, 21, 22])))).toEqual([
      { kind: 'span', id: 10, depth: 0 },
      { kind: 'span', id: 11, depth: 1 },
      { kind: 'span', id: 12, depth: 2 },
      { kind: 'event', eventSeq: 3, depth: 3 },
      { kind: 'span', id: 20, depth: 0 },
      { kind: 'span', id: 21, depth: 1 },
      // span 22 (start_ts=2) emits before the cleanup-block's send
      // (ts=4) in the chronological merge of children + direct events.
      { kind: 'span', id: 22, depth: 2 },
      { kind: 'event', eventSeq: 5, depth: 3 },
      { kind: 'event', eventSeq: 4, depth: 2 },
    ]);
  });

  it('recurses into deeply nested expanded children', () => {
    // test -> shell -> fn-call -> match-start/done
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
        buildSpan({ id: 3, parent: 2, kind: 'fn-call', name: 'match_ok', start_ts: 2 }),
      ],
      [matchStart(3, 3), matchDone(4, 3)],
    );
    expect(summarize(flattenRows(log, new Set([2, 3])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
      { kind: 'span', id: 3, depth: 1 },
      { kind: 'event', eventSeq: 3, depth: 2 },
    ]);
  });

  it('stops descending when an ancestor is collapsed', () => {
    // With shell-block collapsed, fn-call and its events must NOT render.
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({ id: 2, parent: 1, kind: 'shell-block', shell: 's', start_ts: 1 }),
        buildSpan({ id: 3, parent: 2, kind: 'fn-call', name: 'match_ok', start_ts: 2 }),
      ],
      [matchStart(3, 3), matchDone(4, 3)],
    );
    expect(summarize(flattenRows(log, new Set([3])))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
    ]);
  });
});

describe('flattenRows — transparent BIFs', () => {
  it('hides pure-BIF FnCall span and emits a bif-row', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({
          id: 2,
          parent: 1,
          kind: 'fn-call',
          name: 'trim',
          start_ts: 1,
          callee_kind: 'bif',
          is_pure: true,
          args: [['$0', 'hi']],
          result: 'hi',
        }),
      ],
      [],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'bif-row', id: 2, depth: 0 },
    ]);
  });

  it('hides sleep FnCall span; folded sleep row appears at parent depth', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({
          id: 2,
          parent: 1,
          kind: 'fn-call',
          name: 'sleep',
          start_ts: 1,
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [sleepStart(1, 2), sleepDone(2, 2)],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'event', eventSeq: 1, depth: 0 },
    ]);
  });

  it('hides log FnCall span; LogBar appears at parent depth', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({
          id: 2,
          parent: 1,
          kind: 'fn-call',
          name: 'log',
          start_ts: 1,
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [logEv(1, 2, 'hi')],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'log-bar', level: 'log', eventSeq: 1, depth: 0 },
    ]);
  });

  it('hides annotate FnCall span; annotate event is also filtered', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({
          id: 2,
          parent: 1,
          kind: 'fn-call',
          name: 'annotate',
          start_ts: 1,
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [annotateEv(1, 2, 'note')],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([]);
  });

  it('keeps match_ok FnCall span as a span-entry row', () => {
    const log = logWith(
      [
        buildSpan({ id: 1, parent: null, kind: 'test' }),
        buildSpan({
          id: 2,
          parent: 1,
          kind: 'fn-call',
          name: 'match_ok',
          start_ts: 1,
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [],
    );
    expect(summarize(flattenRows(log, new Set()))).toEqual([
      { kind: 'span', id: 2, depth: 0 },
    ]);
  });
});
