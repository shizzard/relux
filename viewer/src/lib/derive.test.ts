import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BufferEvent } from '../types/BufferEvent';
import type { StructuredLog } from '../types/StructuredLog';
import {
  bootstrapForReuse,
  finalCleanupForDeferred,
  firstUseShellBlockForMarker,
  replayBufferRegionsAtMarker,
  selectionSourceRange,
} from './derive';

// Minimal log builder — only `buffer_events` is consulted by
// replayBufferRegionsAtMarker, so every other field is stubbed.
function makeLog(buffer_events: BufferEvent[]): StructuredLog {
  return {
    info: { name: 't', path: 'p', duration_ms: 0n },
    outcome: { kind: 'pass' },
    env: { bootstrap: [] },
    shells: {},
    spans: {},
    events: [],
    buffer_events,
    sources: {},
  };
}

// The tests below use the same shell name and marker for each event so
// the existing replay scenarios still address one logical shell. Marker
// indexing is exercised explicitly in the cross-shell test further down.
function grew(seq: number, shell: string, data: string): BufferEvent {
  return { seq: BigInt(seq), ts: 0, shell, shell_marker: shell, kind: 'grew', data };
}

// The runtime emits `before` and `after` untruncated — they are the full
// bytes of the buffer tail surrounding the match.  The invariant the
// viewer enforces:  before + matched + after === current tail.
function matched(
  seq: number,
  shell: string,
  before: string,
  matchedBytes: string,
  after: string,
): BufferEvent {
  return {
    seq: BigInt(seq),
    ts: 0,
    shell,
    shell_marker: shell,
    kind: 'matched',
    before,
    matched: matchedBytes,
    after,
  };
}

function reset(seq: number, shell: string, discarded = ''): BufferEvent {
  return {
    seq: BigInt(seq),
    ts: 0,
    shell,
    shell_marker: shell,
    kind: 'reset',
    discarded,
  };
}

describe('replayBufferRegionsAtMarker', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    warnSpy.mockRestore();
  });

  it('returns empty regions when no buffer events exist', () => {
    const log = makeLog([]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: '',
    });
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it('returns empty regions when no events match the shell', () => {
    const log = makeLog([grew(1, 'other', 'abc'), grew(2, 'other', 'def')]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: '',
    });
  });

  it('appends grow data to tail when no match has happened', () => {
    const log = makeLog([grew(1, 's', 'abc'), grew(2, 's', 'def')]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: 'abcdef',
    });
  });

  it('splits the unmatched tail into consumed/matched/tail on a single match', () => {
    // Tail is "abcdef"; match takes "cd" out of the middle.
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', 'ab', 'cd', 'ef'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'ab',
      matched: { bytes: 'cd', seq: 2 },
      tail: 'ef',
    });
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it('folds previous matched bytes into consumed on the next match', () => {
    // First match: tail "abcdef" -> "ab"+"cd"+"ef".  Then grow "ghi" -> tail "efghi".
    // Second match against tail "efghi": before="ef", matched="g", after="hi"
    // (the runtime emits the full surrounding context).
    //
    // After: consumed gathers everything before the new match in the
    // *full* timeline = old consumed + previous matched + new before.
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', 'ab', 'cd', 'ef'),
      grew(3, 's', 'ghi'),
      matched(4, 's', 'ef', 'g', 'hi'),
    ]);
    const out = replayBufferRegionsAtMarker(log, 100, 's');
    expect(out).toEqual({
      consumed: 'abcdef',
      matched: { bytes: 'g', seq: 4 },
      tail: 'hi',
    });
    expect(out.consumed + out.matched!.bytes + out.tail).toBe('abcdefghi');
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it('clears all regions on reset', () => {
    const log = makeLog([
      grew(1, 's', 'abc'),
      matched(2, 's', '', 'a', 'bc'),
      reset(3, 's', 'whatever'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: '',
    });
  });

  it('resumes growth after a reset using only post-reset bytes', () => {
    const log = makeLog([
      grew(1, 's', 'pre'),
      reset(2, 's'),
      grew(3, 's', 'post'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: 'post',
    });
  });

  it('stops processing at events past seq (inclusive cap)', () => {
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', '', 'ab', 'cdef'),
      grew(3, 's', 'ghi'),
      matched(4, 's', 'cd', 'ef', 'ghi'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 3, 's')).toEqual({
      consumed: '',
      matched: { bytes: 'ab', seq: 2 },
      tail: 'cdefghi',
    });
  });

  it('includes the event at seq=N when called with seq=N', () => {
    const log = makeLog([
      grew(1, 's', 'abc'),
      matched(2, 's', 'a', 'b', 'c'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 2, 's')).toEqual({
      consumed: 'a',
      matched: { bytes: 'b', seq: 2 },
      tail: 'c',
    });
  });

  it('skips events from other shells', () => {
    const log = makeLog([
      grew(1, 'other', 'XYZ'),
      grew(2, 's', 'abc'),
      grew(3, 'other', 'more'),
      matched(4, 'other', '', 'X', 'YZmore'),
      matched(5, 's', '', 'a', 'bc'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: { bytes: 'a', seq: 5 },
      tail: 'bc',
    });
  });

  it('reconstructs each shell independently in an interleaved log', () => {
    const log = makeLog([
      grew(1, 'a', 'AAA'),
      grew(2, 'b', 'BBB'),
      matched(3, 'a', '', 'A', 'AA'),
      grew(4, 'b', 'bbb'),
      matched(5, 'b', 'BB', 'B', 'bbb'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 'a')).toEqual({
      consumed: '',
      matched: { bytes: 'A', seq: 3 },
      tail: 'AA',
    });
    expect(replayBufferRegionsAtMarker(log, 100, 'b')).toEqual({
      consumed: 'BB',
      matched: { bytes: 'B', seq: 5 },
      tail: 'bbb',
    });
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it('handles a match that consumes the entire tail (empty before and after)', () => {
    const log = makeLog([
      grew(1, 's', 'exact'),
      matched(2, 's', '', 'exact', ''),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: { bytes: 'exact', seq: 2 },
      tail: '',
    });
  });

  it('preserves very long histories byte-for-byte', () => {
    const big = 'x'.repeat(9000);
    const log = makeLog([
      grew(1, 's', `${big}M`),
      matched(2, 's', big, 'M', ''),
    ]);
    const out = replayBufferRegionsAtMarker(log, 100, 's');
    expect(out.consumed).toBe(big);
    expect(out.consumed.length).toBe(9000);
    expect(out.matched).toEqual({ bytes: 'M', seq: 2 });
    expect(out.tail).toBe('');
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it('warns when before+matched+after does not equal the current tail', () => {
    // Inconsistent input: tail is "abcdef" but the match claims to have
    // operated on "QZ"+matched+"".  The function must surface the
    // mismatch via console.warn instead of silently producing garbage.
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', 'QZ', 'cd', ''),
    ]);
    replayBufferRegionsAtMarker(log, 100, 's');
    expect(warnSpy).toHaveBeenCalledTimes(1);
    expect(warnSpy.mock.calls[0]?.[0]).toContain('tail mismatch');
    expect(warnSpy.mock.calls[0]?.[0]).toContain('seq=2');
    expect(warnSpy.mock.calls[0]?.[0]).toContain('marker=s');
  });

  it('produces the right regions at every intermediate seq prefix', () => {
    // Walk a small but complete scenario step-by-step. All `before` and
    // `after` strings are full bytes, satisfying the runtime invariant.
    const events: BufferEvent[] = [
      grew(1, 's', 'hello '),
      grew(2, 's', 'world\n'),
      matched(3, 's', '', 'hello ', 'world\n'),
      grew(4, 's', 'more text\n'),
      matched(5, 's', 'world\n', 'more', ' text\n'),
      reset(6, 's'),
      grew(7, 's', 'after reset'),
    ];
    const log = makeLog(events);

    const expected: Array<[number, ReturnType<typeof replayBufferRegionsAtMarker>]> = [
      [0, { consumed: '', matched: null, tail: '' }],
      [1, { consumed: '', matched: null, tail: 'hello ' }],
      [2, { consumed: '', matched: null, tail: 'hello world\n' }],
      [3, { consumed: '', matched: { bytes: 'hello ', seq: 3 }, tail: 'world\n' }],
      [4, { consumed: '', matched: { bytes: 'hello ', seq: 3 }, tail: 'world\nmore text\n' }],
      [5, { consumed: 'hello world\n', matched: { bytes: 'more', seq: 5 }, tail: ' text\n' }],
      [6, { consumed: '', matched: null, tail: '' }],
      [7, { consumed: '', matched: null, tail: 'after reset' }],
    ];

    for (const [seq, want] of expected) {
      expect(replayBufferRegionsAtMarker(log, seq, 's'), `seq=${seq}`).toEqual(want);
    }
    expect(warnSpy).not.toHaveBeenCalled();
  });
});

// Helpers for the partner-lookup tests below — only `spans` is read, so
// everything else is stubbed.
type SpanRecord = Record<string, unknown>;
function spansLog(spans: SpanRecord[]): StructuredLog {
  const byId: Record<string, SpanRecord> = {};
  for (const span of spans) {
    byId[String(span.id)] = span;
  }
  return {
    test: { name: 't', path: 'p', outcome: 'pass', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: byId,
    events: [],
    buffer_events: [],
    failure: null,
    sources: {},
  } as unknown as StructuredLog;
}
function setupSpan(id: bigint, marker: string, is_reuse: boolean): SpanRecord {
  return {
    id,
    kind: 'effect-setup',
    effect: 'E0',
    overlay: [],
    alias: null,
    marker,
    is_reuse,
    parent: 1n,
    start_ts: 0,
    end_ts: 0,
    location: null,
  };
}
function cleanupSpan(id: bigint, marker: string, is_deferred: boolean): SpanRecord {
  return {
    id,
    kind: 'effect-cleanup',
    effect: 'E0',
    alias: null,
    setup_span: 2n,
    marker,
    is_deferred,
    parent: 1n,
    start_ts: 0,
    end_ts: 0,
    location: null,
  };
}

describe('bootstrapForReuse', () => {
  it('returns the bootstrap setup id when one with the marker exists', () => {
    const log = spansLog([
      setupSpan(2n, 'kind-cobra-0001', false),
      setupSpan(3n, 'kind-cobra-0001', true),
    ]);
    expect(bootstrapForReuse(log, 'kind-cobra-0001')).toBe(2);
  });

  it('ignores reuse spans even when their marker matches', () => {
    const log = spansLog([setupSpan(5n, 'kind-cobra-0001', true)]);
    expect(bootstrapForReuse(log, 'kind-cobra-0001')).toBeNull();
  });

  it('returns null when no bootstrap with that marker exists', () => {
    expect(bootstrapForReuse(spansLog([]), 'kind-cobra-0001')).toBeNull();
  });
});

describe('finalCleanupForDeferred', () => {
  it('returns the final cleanup id when one with the marker exists', () => {
    const log = spansLog([
      cleanupSpan(4n, 'kind-cobra-0001', false),
      cleanupSpan(5n, 'kind-cobra-0001', true),
    ]);
    expect(finalCleanupForDeferred(log, 'kind-cobra-0001')).toBe(4);
  });

  it('ignores deferred cleanups even when their marker matches', () => {
    const log = spansLog([cleanupSpan(6n, 'kind-cobra-0001', true)]);
    expect(finalCleanupForDeferred(log, 'kind-cobra-0001')).toBeNull();
  });

  it('returns null when no final cleanup with that marker exists', () => {
    expect(finalCleanupForDeferred(spansLog([]), 'kind-cobra-0001')).toBeNull();
  });
});

// firstUseShellBlockForMarker reads `spans` AND `events`, so this helper
// needs both wired up.
function shellBlock(id: bigint, shell: string): SpanRecord {
  return {
    id,
    kind: 'shell-block',
    shell,
    parent: 1n,
    start_ts: 0,
    end_ts: 0,
    location: null,
  };
}

function spawnEvent(seq: number, span: bigint, marker: string, name: string) {
  return {
    seq: BigInt(seq),
    ts: 0,
    span,
    shell: name,
    shell_marker: marker,
    kind: 'shell-spawn',
    name,
    command: '/bin/sh',
  };
}

function switchEvent(seq: number, span: bigint, marker: string, name: string) {
  return {
    seq: BigInt(seq),
    ts: 0,
    span,
    shell: name,
    shell_marker: marker,
    kind: 'shell-switch',
    name,
  };
}

function logWithSpansAndEvents(spans: SpanRecord[], events: unknown[]): StructuredLog {
  const byId: Record<string, SpanRecord> = {};
  for (const span of spans) byId[String(span.id)] = span;
  return {
    test: { name: 't', path: 'p', outcome: 'pass', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: byId,
    events,
    buffer_events: [],
    failure: null,
    sources: {},
  } as unknown as StructuredLog;
}

describe('firstUseShellBlockForMarker', () => {
  it('returns the shell-block that contains shell-spawn for the marker', () => {
    const log = logWithSpansAndEvents(
      [shellBlock(10n, 'default'), shellBlock(20n, 'default')],
      [
        spawnEvent(1, 10n, 'tiny-cat-0001', 'default'),
        switchEvent(2, 20n, 'tiny-cat-0001', 'default'),
      ],
    );
    expect(firstUseShellBlockForMarker(log, 'tiny-cat-0001')).toBe(10);
  });

  it('returns null when no shell-block first-event is shell-spawn for the marker', () => {
    const log = logWithSpansAndEvents(
      [shellBlock(20n, 'default')],
      [switchEvent(1, 20n, 'tiny-cat-0001', 'default')],
    );
    expect(firstUseShellBlockForMarker(log, 'tiny-cat-0001')).toBeNull();
  });

  it('distinguishes two markers with the same shell name', () => {
    // Two effect-cleanup `__cleanup` shells: same name, different markers.
    const log = logWithSpansAndEvents(
      [shellBlock(10n, '__cleanup'), shellBlock(20n, '__cleanup')],
      [
        spawnEvent(1, 10n, 'aaa-bbb-1111', '__cleanup'),
        spawnEvent(2, 20n, 'ccc-ddd-2222', '__cleanup'),
      ],
    );
    expect(firstUseShellBlockForMarker(log, 'aaa-bbb-1111')).toBe(10);
    expect(firstUseShellBlockForMarker(log, 'ccc-ddd-2222')).toBe(20);
  });
});

describe('selectionSourceRange', () => {
  function makeData(
    spans: Record<string, unknown> = {},
    events: unknown[] = [],
  ): StructuredLog {
    return {
      test: { name: 't', path: 'p', outcome: 'pass', duration_ms: 0n },
      env: { bootstrap: [] },
      shells: {},
      spans,
      events,
      buffer_events: [],
      failure: null,
      sources: {},
    } as unknown as StructuredLog;
  }

  it('returns the span location when a span is selected', () => {
    const data = makeData({
      '1': {
        id: 1n,
        parent: null,
        start_ts: 0,
        end_ts: 0,
        kind: 'test',
        name: 't',
        location: { file: 'a.relux', line: 1, start: 0, end: 4 },
      },
    });
    expect(selectionSourceRange(data, 1, null)).toEqual({
      file: 'a.relux',
      line: 1,
      start: 0,
      end: 4,
    });
  });

  it('returns event.source when an event is selected', () => {
    const data = makeData({}, [
      {
        seq: 7n,
        ts: 0,
        span: 1n,
        shell: null,
        shell_marker: null,
        source: { file: 'a.relux', line: 2, start: 10, end: 20 },
        kind: 'annotate',
        text: 'x',
      },
    ]);
    expect(selectionSourceRange(data, null, 7)).toEqual({
      file: 'a.relux',
      line: 2,
      start: 10,
      end: 20,
    });
  });

  it('falls back to parent-span location when event has no source', () => {
    const data = makeData(
      {
        '1': {
          id: 1n,
          parent: null,
          start_ts: 0,
          end_ts: 0,
          kind: 'test',
          name: 't',
          location: { file: 'a.relux', line: 5, start: 50, end: 60 },
        },
      },
      [
        {
          seq: 7n,
          ts: 0,
          span: 1n,
          shell: null,
          shell_marker: null,
          source: null,
          kind: 'annotate',
          text: 'x',
        },
      ],
    );
    expect(selectionSourceRange(data, null, 7)).toEqual({
      file: 'a.relux',
      line: 5,
      start: 50,
      end: 60,
    });
  });

  it('merges folded sleep halves (min start, max end)', () => {
    const data = makeData({}, [
      {
        seq: 7n,
        ts: 0,
        span: 1n,
        shell: null,
        shell_marker: null,
        source: { file: 'a.relux', line: 3, start: 100, end: 110 },
        kind: 'sleep-start',
        duration: 1,
      },
      {
        seq: 8n,
        ts: 0,
        span: 1n,
        shell: null,
        shell_marker: null,
        source: { file: 'a.relux', line: 3, start: 102, end: 112 },
        kind: 'sleep-done',
      },
    ]);
    expect(selectionSourceRange(data, null, 7)).toEqual({
      file: 'a.relux',
      line: 3,
      start: 100,
      end: 112,
    });
  });

  it('returns null when nothing is selected', () => {
    expect(selectionSourceRange(makeData(), null, null)).toBeNull();
  });
});
