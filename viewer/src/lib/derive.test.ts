import { describe, expect, it } from 'vitest';
import type { BufferEvent } from '../types/BufferEvent';
import type { StructuredLog } from '../types/StructuredLog';
import {
  bootstrapForReuse,
  buildMultiMatchIndex,
  finalCleanupForDeferred,
  firstUseShellBlockForMarker,
  liveShellsAtSpan,
  multiMatchOutcomeFor,
  patternMatchedTextFor,
  replayBufferRegionsAtMarker,
  replayBufferRegionsAtPerPatternDone,
  selectionSourceRange,
} from './derive';
import type { Event } from '../types/Event';
import type { MultiMatchPattern } from '../types/MultiMatchPattern';
import type { Span } from '../types/Span';

// Minimal log builder - only `buffer_events` is consulted by
// replayBufferRegionsAtMarker, so every other field is stubbed.
function makeLog(buffer_events: BufferEvent[]): StructuredLog {
  return {
    schema_version: 2,
    info: { name: 't', path: 'p', duration_ms: 0n },
    outcome: { kind: 'pass' },
    env: { bootstrap: [] },
    shells: {},
    spans: {},
    events: [],
    buffer_events,
    sources: {},
    artifacts: [],
  };
}

// The tests below use the same shell name and marker for each event so
// the existing replay scenarios still address one logical shell. Marker
// indexing is exercised explicitly in the cross-shell test further down.
function grew(seq: number, shell: string, data: string): BufferEvent {
  return { seq: BigInt(seq), ts: 0, shell, shell_marker: shell, kind: 'grew', data };
}

// The runtime emits `before` and `after` untruncated - they are the full
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

function reset(seq: number, shell: string, consumed = ''): BufferEvent {
  return {
    seq: BigInt(seq),
    ts: 0,
    shell,
    shell_marker: shell,
    kind: 'reset',
    consumed,
  };
}

describe('replayBufferRegionsAtMarker', () => {
  it('returns empty regions when no buffer events exist', () => {
    const log = makeLog([]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: '',
      matched: null,
      tail: '',
    });
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
  });

  it('folds tail and active matched into consumed on reset', () => {
    const log = makeLog([
      grew(1, 's', 'abc'),
      matched(2, 's', '', 'a', 'bc'),
      reset(3, 's', 'bc'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'abc',
      matched: null,
      tail: '',
    });
  });

  it('folds tail into consumed on reset with no active match', () => {
    const log = makeLog([
      grew(1, 's', 'abc'),
      reset(2, 's', 'abc'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'abc',
      matched: null,
      tail: '',
    });
  });

  it('preserves earlier consumed history through a reset', () => {
    const log = makeLog([
      grew(1, 's', 'first '),
      matched(2, 's', '', 'first ', ''),
      grew(3, 's', 'second'),
      reset(4, 's', 'second'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'first second',
      matched: null,
      tail: '',
    });
  });

  it('clamps tail to empty when reset consumed exceeds reconstructed tail', () => {
    // Defensive case: emitter shipped more bytes in `consumed` than the
    // viewer accumulated in `tail`. Shouldn't happen with the aligned
    // emitter, but the trim must not throw.
    const log = makeLog([
      grew(1, 's', 'ab'),
      reset(2, 's', 'abcdef'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'abcdef',
      matched: null,
      tail: '',
    });
  });

  it('resumes growth after a reset, preserving pre-reset bytes in consumed', () => {
    const log = makeLog([
      grew(1, 's', 'pre'),
      reset(2, 's', 'pre'),
      grew(3, 's', 'post'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'pre',
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
  });

  it('uses the matched event as authoritative when its pieces do not equal the current tail', () => {
    // Inconsistent input: tail is "abcdef" but the matched event claims to
    // have operated on "QZ"+"cd"+"".  The runtime invariant says this
    // shouldn't happen, but the function still produces a well-formed
    // result by trusting the matched event's pieces.
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', 'QZ', 'cd', ''),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'QZ',
      matched: { bytes: 'cd', seq: 2 },
      tail: '',
    });
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
      reset(6, 's', ' text\n'),
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
      [6, { consumed: 'hello world\nmore text\n', matched: null, tail: '' }],
      [7, { consumed: 'hello world\nmore text\n', matched: null, tail: 'after reset' }],
    ];

    for (const [seq, want] of expected) {
      expect(replayBufferRegionsAtMarker(log, seq, 's'), `seq=${seq}`).toEqual(want);
    }
  });
});

// Helpers for the partner-lookup tests below - only `spans` is read, so
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

describe('liveShellsAtSpan', () => {
  function shellRecord(marker: string, name: string, spawn_ts: number) {
    return { marker, name, command: 'sh', spawn_ts, terminate_ts: null };
  }
  function ev(
    seq: number,
    ts: number,
    kind: Event['kind'],
    shell_marker: string,
  ): Event {
    return {
      seq: BigInt(seq),
      ts,
      span: 1n,
      shell: null,
      shell_marker,
      source: null,
      kind,
      // narrow events with extra payload are unused by liveShells replay
    } as unknown as Event;
  }
  function span(end_ts: number | null): Span {
    return {
      id: 1n,
      kind: 'fn-call',
      name: 'f',
      args: [],
      result: null,
      callee_kind: 'user',
      is_pure: false,
      parent: null,
      start_ts: 0,
      end_ts,
      location: null,
    } as unknown as Span;
  }
  function logWith(
    shells: Record<string, ReturnType<typeof shellRecord>>,
    events: Event[],
  ): StructuredLog {
    return {
      schema_version: 2,
      info: { name: 't', path: 'p', duration_ms: 0n },
      outcome: { kind: 'pass' },
      env: { bootstrap: [] },
      shells: shells as unknown as StructuredLog['shells'],
      spans: {},
      events,
      buffer_events: [],
      sources: {},
      artifacts: [],
    };
  }

  it('returns each shell as ready when no match has started by the anchor', () => {
    const log = logWith(
      {
        'a-marker': shellRecord('a-marker', 'a', 0),
        'b-marker': shellRecord('b-marker', 'b', 0),
      },
      [
        ev(1, 0.1, 'shell-spawn', 'a-marker'),
        ev(2, 0.2, 'shell-spawn', 'b-marker'),
        ev(3, 0.3, 'send', 'a-marker'),
        ev(4, 0.4, 'send', 'b-marker'),
      ],
    );
    const result = liveShellsAtSpan(log, span(1.0));
    expect(result.map((s) => `${s.name}:${s.state}`).sort()).toEqual(['a:ready', 'b:ready']);
  });

  it('marks shell busy when match-start has no match-done by the span end', () => {
    const log = logWith(
      { 'a-marker': shellRecord('a-marker', 'a', 0) },
      [
        ev(1, 0.1, 'shell-spawn', 'a-marker'),
        ev(2, 0.2, 'match-start', 'a-marker'),
      ],
    );
    const result = liveShellsAtSpan(log, span(0.5));
    expect(result).toEqual([
      { marker: 'a-marker', name: 'a', command: 'sh', state: 'busy' },
    ]);
  });

  it('walks to the latest event for an in-flight span (end_ts === null)', () => {
    const log = logWith(
      { 'a-marker': shellRecord('a-marker', 'a', 0) },
      [
        ev(1, 0.1, 'shell-spawn', 'a-marker'),
        ev(2, 0.2, 'match-start', 'a-marker'),
        ev(3, 0.3, 'match-done', 'a-marker'),
      ],
    );
    const result = liveShellsAtSpan(log, span(null));
    expect(result[0]?.state).toBe('ready');
  });

  it('returns [] when no event fires within the span lifetime', () => {
    const log = logWith({ 'a-marker': shellRecord('a-marker', 'a', 100) }, [
      ev(1, 200, 'shell-spawn', 'a-marker'),
    ]);
    // span ends before any event fires
    const result = liveShellsAtSpan(log, span(50));
    expect(result).toEqual([]);
  });

  it('marks a shell as pending when its spawn has not fired by the anchor', () => {
    // Anchor lands inside shell A's lifetime but before shell B spawns.
    // B's record exists in `data.shells` (it spawns later in the log),
    // but at the anchor moment B is "not yet started" - must not show
    // as `ready` (which the modal renders as "running").
    const log = logWith(
      {
        'a-marker': shellRecord('a-marker', 'a', 0),
        'b-marker': shellRecord('b-marker', 'b', 0),
      },
      [
        ev(1, 0.1, 'shell-spawn', 'a-marker'),
        ev(2, 0.2, 'send', 'a-marker'),
        ev(3, 1.0, 'shell-spawn', 'b-marker'),
      ],
    );
    const result = liveShellsAtSpan(log, span(0.5));
    const byName = new Map(result.map((s) => [s.name, s.state]));
    expect(byName.get('a')).toBe('ready');
    expect(byName.get('b')).toBe('pending');
  });
});

// Helpers reused by the multimatch tests below. A buffer-events-only
// helper would mask the events/spans plumbing that the index walks; we
// provide a fuller make function that accepts all three streams.
function bufferEvent(
  seq: number,
  shell: string,
  kind: 'grew' | 'matched',
  payload: Record<string, string>,
): BufferEvent {
  return {
    seq: BigInt(seq),
    ts: 0,
    shell,
    shell_marker: shell,
    kind,
    ...payload,
  } as unknown as BufferEvent;
}

function makeLogWithMultiMatch(
  buffer_events: BufferEvent[],
  events: Event[],
  spans: Record<string, Span>,
): StructuredLog {
  return {
    schema_version: 2,
    info: { name: 't', path: 'p', duration_ms: 0n },
    outcome: { kind: 'pass' },
    env: { bootstrap: [] },
    shells: {},
    spans: spans as unknown as StructuredLog['spans'],
    events,
    buffer_events,
    sources: {},
    artifacts: [],
  };
}

describe('buildMultiMatchIndex', () => {
  it('marks per-pattern Matched events inside a multi-match span as observation-only', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 12n,
      } as unknown as Event,
      {
        seq: 13n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-done',
        advance_to: 12n,
      } as unknown as Event,
    ];
    const buf: BufferEvent[] = [
      bufferEvent(12, 's', 'matched', { before: 'ab', matched: 'cd', after: 'ef' }),
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    expect(idx.observationSeqs.has(12)).toBe(true);
    expect(idx.endBySeq.get(13)).toEqual({ advanceBytes: 4, matchedSeq: 12 });
  });

  it('omits the advance entry on the timeout path', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 12n,
      } as unknown as Event,
      {
        seq: 13n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-timeout',
        unmatched: [1],
      } as unknown as Event,
    ];
    const buf: BufferEvent[] = [
      bufferEvent(12, 's', 'matched', { before: '', matched: 'cd', after: 'ef' }),
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    expect(idx.observationSeqs.has(12)).toBe(true);
    // Timeout still registers in endBySeq (as a null entry, so the
    // replay loop clears the observation highlight) but carries no
    // drain.
    expect(idx.endBySeq.get(13)).toBeNull();
  });

  it('does not flag Matched events outside any multi-match span', () => {
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 1n, shell: 's', shell_marker: 's', source: null,
        kind: 'match-done', matched: 'cd', elapsed: 5, captures: null, buffer_seq: 12n,
      } as unknown as Event,
    ];
    const buf: BufferEvent[] = [
      bufferEvent(12, 's', 'matched', { before: 'ab', matched: 'cd', after: 'ef' }),
    ];
    const shellBlockSpan: Span = {
      id: 1n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'shell-block', shell: 's',
    } as unknown as Span;
    const log = makeLogWithMultiMatch(buf, events, { '1': shellBlockSpan });
    const idx = buildMultiMatchIndex(log);
    expect(idx.observationSeqs.size).toBe(0);
    expect(idx.endBySeq.size).toBe(0);
  });
});

describe('replayBufferRegionsAtMarker with multimatch index', () => {
  it('observation-only Matched events highlight without advancing the cursor', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 2n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    expect(replayBufferRegionsAtMarker(log, 100, 's', idx)).toEqual({
      consumed: '',
      matched: { bytes: 'BB', seq: 2 },
      tail: 'AABBCC',
    });
  });

  it('multi-match-done drains the cursor by len(before)+len(matched)', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 2n,
      } as unknown as Event,
      {
        seq: 12n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-done',
        advance_to: 2n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    expect(replayBufferRegionsAtMarker(log, 100, 's', idx)).toEqual({
      consumed: 'AABB',
      matched: null,
      tail: 'CC',
    });
  });

  it('multi-match-timeout does not drain but clears the observation highlight', () => {
    // The observation matched 'BB' inside the undrained tail. On the
    // timeout path no bytes are drained from `tail`, but the highlight
    // must be cleared - the matched bytes are still in `tail`, so
    // leaving the highlight active would render them twice.
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 2n,
      } as unknown as Event,
      {
        seq: 12n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-timeout',
        unmatched: [1],
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    expect(replayBufferRegionsAtMarker(log, 100, 's', idx)).toEqual({
      consumed: '',
      matched: null,
      tail: 'AABBCC',
    });
  });

  it('keeps the observation highlight active while the block is still in flight', () => {
    // No multi-match-done or -timeout in the events: the user is
    // looking at a snapshot taken mid-block. The highlight stays so
    // the user sees what one of the patterns saw against the
    // undrained buffer.
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 2n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    expect(replayBufferRegionsAtMarker(log, 11, 's', idx)).toEqual({
      consumed: '',
      matched: { bytes: 'BB', seq: 2 },
      tail: 'AABBCC',
    });
  });

  it('handles two observation Matched events: only the second highlights, no advance', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
      matched(3, 's', '', 'AA', 'BBCC'),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 2n,
      } as unknown as Event,
      {
        seq: 12n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 1, elapsed: 6, buffer_seq: 3n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    expect(replayBufferRegionsAtMarker(log, 100, 's', idx)).toEqual({
      consumed: '',
      matched: { bytes: 'AA', seq: 3 },
      tail: 'AABBCC',
    });
  });

  it('falls back to the existing behaviour when no multimatch index is passed', () => {
    const log = makeLog([
      grew(1, 's', 'abcdef'),
      matched(2, 's', 'ab', 'cd', 'ef'),
    ]);
    expect(replayBufferRegionsAtMarker(log, 100, 's')).toEqual({
      consumed: 'ab',
      matched: { bytes: 'cd', seq: 2 },
      tail: 'ef',
    });
  });
});

describe('patternMatchedTextFor', () => {
  it('returns the matched substring on the referenced Matched buffer event', () => {
    const buf: BufferEvent[] = [
      bufferEvent(5, 's', 'matched', { before: 'a', matched: 'b', after: 'c' }),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 1n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 1, buffer_seq: 5n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, {});
    expect(patternMatchedTextFor(log, events[0]!)).toBe('b');
  });

  it('returns null when the referenced buffer event is missing', () => {
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 1n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 1, buffer_seq: 5n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch([], events, {});
    expect(patternMatchedTextFor(log, events[0]!)).toBeNull();
  });
});

describe('multiMatchOutcomeFor', () => {
  function mmSpan(): Span {
    return {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
  }
  const patterns: MultiMatchPattern[] = [
    { pattern: 'job-a: done', is_regex: false },
    { pattern: 'job-b: done', is_regex: false },
  ];

  it('reports matched for every pattern with a pattern-done event, on the done terminal', () => {
    const buf: BufferEvent[] = [
      bufferEvent(20, 's', 'matched', { before: '', matched: 'job-a: done', after: '' }),
      bufferEvent(21, 's', 'matched', { before: 'job-a: done\n', matched: 'job-b: done', after: '' }),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
        patterns,
        effective: { type: 'tolerance', duration: '5s', multiplier: '1.0', total_duration: '5s', source: null },
      } as unknown as Event,
      {
        seq: 22n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 50, buffer_seq: 20n,
      } as unknown as Event,
      {
        seq: 23n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 1, elapsed: 60, buffer_seq: 21n,
      } as unknown as Event,
      {
        seq: 24n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-done',
        advance_to: 21n,
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan() });
    expect(multiMatchOutcomeFor(log, 7)).toEqual({
      patterns,
      terminal: 'done',
      rows: [
        { index: 0, status: 'matched', matched: 'job-a: done', elapsed: 50 },
        { index: 1, status: 'matched', matched: 'job-b: done', elapsed: 60 },
      ],
    });
  });

  it('reports not-seen for unmatched patterns on the timeout terminal', () => {
    const buf: BufferEvent[] = [
      bufferEvent(20, 's', 'matched', { before: '', matched: 'job-a: done', after: '' }),
    ];
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
        patterns,
        effective: { type: 'tolerance', duration: '5s', multiplier: '1.0', total_duration: '5s', source: null },
      } as unknown as Event,
      {
        seq: 22n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 50, buffer_seq: 20n,
      } as unknown as Event,
      {
        seq: 23n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-timeout',
        unmatched: [1],
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan() });
    expect(multiMatchOutcomeFor(log, 7)).toEqual({
      patterns,
      terminal: 'timeout',
      rows: [
        { index: 0, status: 'matched', matched: 'job-a: done', elapsed: 50 },
        { index: 1, status: 'not-seen', matched: null, elapsed: null },
      ],
    });
  });

  it('reports pending terminal for an in-flight block (no done, no timeout)', () => {
    const events: Event[] = [
      {
        seq: 10n, ts: 0, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
        patterns,
        effective: { type: 'tolerance', duration: '5s', multiplier: '1.0', total_duration: '5s', source: null },
      } as unknown as Event,
    ];
    const log = makeLogWithMultiMatch([], events, { '7': mmSpan() });
    expect(multiMatchOutcomeFor(log, 7)).toEqual({
      patterns,
      terminal: 'pending',
      rows: [
        { index: 0, status: 'not-seen', matched: null, elapsed: null },
        { index: 1, status: 'not-seen', matched: null, elapsed: null },
      ],
    });
  });

  it('returns null when the span id is not a multi-match span', () => {
    const log = makeLogWithMultiMatch([], [], {});
    expect(multiMatchOutcomeFor(log, 999)).toBeNull();
  });
});

describe('replayBufferRegionsAtPerPatternDone', () => {
  it('splits the tail around the observed match for the event own shell', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'AABBCC'),
      matched(2, 's', 'AA', 'BB', 'CC'),
    ];
    const patternDone: Event = {
      seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 0, elapsed: 5, buffer_seq: 2n,
    } as unknown as Event;
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      patternDone,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    expect(replayBufferRegionsAtPerPatternDone(log, patternDone, 's', idx)).toEqual({
      consumed: '',
      matched: { bytes: 'BB', seq: 2 },
      tail: 'AABBCC',
      tailSplit: { tailBefore: 'AA', tailAfter: 'CC' },
    });
  });

  it('folds a stale highlight from before the block into consumed before splitting', () => {
    // A regular match-done fires before the block; its highlight is
    // the active matched at block entry. The per-pattern view should
    // show only the observation's match, so the stale highlight is
    // folded into consumed.
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'prefix:'),
      matched(2, 's', '', 'prefix:', ''),
      grew(3, 's', 'AABBCC'),
      matched(4, 's', 'AA', 'BB', 'CC'),
    ];
    const patternDone: Event = {
      seq: 21n, ts: 4, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 0, elapsed: 5, buffer_seq: 4n,
    } as unknown as Event;
    const events: Event[] = [
      {
        seq: 20n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      patternDone,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    expect(replayBufferRegionsAtPerPatternDone(log, patternDone, 's', idx)).toEqual({
      consumed: 'prefix:',
      matched: { bytes: 'BB', seq: 4 },
      tail: 'AABBCC',
      tailSplit: { tailBefore: 'AA', tailAfter: 'CC' },
    });
  });

  it('preserves a pre-block regular match in consumed when selecting an observation inside the block', () => {
    // The bug: a regular match-done before the multi-match block
    // drained its `matched` bytes out of `tail`. When the first
    // observation inside the block fires, the prev highlight (the
    // regular match) must fold into `consumed` - otherwise its bytes
    // vanish from the rendered history. This regression test
    // mirrors the e2e fixture where 'relux> ' was lost on the
    // second-observation click.
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'pre\nrelux> '),
      // Regular (non-observation) match: drains 'relux> ' out of tail.
      matched(2, 's', 'pre\n', 'relux> ', ''),
      // More bytes accumulate.
      grew(3, 's', 'A\nB\n'),
      // First observation inside the block.
      matched(4, 's', '', 'A', '\nB\n'),
      // Second observation inside the block.
      matched(5, 's', 'A\n', 'B', '\n'),
    ];
    const secondPatternDone: Event = {
      seq: 12n, ts: 5, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 1, elapsed: 6, buffer_seq: 5n,
    } as unknown as Event;
    const events: Event[] = [
      {
        seq: 10n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      {
        seq: 11n, ts: 4, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-pattern-done',
        index: 0, elapsed: 5, buffer_seq: 4n,
      } as unknown as Event,
      secondPatternDone,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    expect(replayBufferRegionsAtPerPatternDone(log, secondPatternDone, 's', idx)).toEqual({
      consumed: 'pre\nrelux> ',
      matched: { bytes: 'B', seq: 5 },
      tail: 'A\nB\n',
      tailSplit: { tailBefore: 'A\n', tailAfter: '\n' },
    });
  });

  it('does not double-count when an earlier per-pattern observation is still active', () => {
    // Two observations in the same multi-match span. The user clicks
    // the second one. The first observation's match is still in
    // `base.matched` when we replay up to (buffer_seq - 1), but its
    // bytes are still inside `tail` too - folding into `consumed`
    // would render those bytes twice.
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 's', 'job-a: done\r\njob-b: done\r\n'),
      // First observation matches 'job-a: done'.
      matched(2, 's', '', 'job-a: done', '\r\njob-b: done\r\n'),
      // Second observation matches 'job-b: done' within the same tail.
      matched(3, 's', 'job-a: done\r\n', 'job-b: done', '\r\n'),
    ];
    const firstPatternDone: Event = {
      seq: 11n, ts: 2, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 0, elapsed: 5, buffer_seq: 2n,
    } as unknown as Event;
    const secondPatternDone: Event = {
      seq: 12n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 1, elapsed: 6, buffer_seq: 3n,
    } as unknown as Event;
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      firstPatternDone,
      secondPatternDone,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);

    // Selecting the second pattern-done: consumed must NOT include
    // 'job-a: done' even though the first observation matched it -
    // those bytes are still in the tail, surrounded by tailBefore and
    // tailAfter of the second observation.
    expect(replayBufferRegionsAtPerPatternDone(log, secondPatternDone, 's', idx)).toEqual({
      consumed: '',
      matched: { bytes: 'job-b: done', seq: 3 },
      tail: 'job-a: done\r\njob-b: done\r\n',
      tailSplit: { tailBefore: 'job-a: done\r\n', tailAfter: '\r\n' },
    });
  });

  it('falls back to the regular replay for other shells', () => {
    const mmSpan: Span = {
      id: 7n, parent: null, start_ts: 0, end_ts: 100, location: null,
      kind: 'multi-match', shell: 's',
    } as unknown as Span;
    const buf: BufferEvent[] = [
      grew(1, 'other', 'XYZ'),
      grew(2, 's', 'AABBCC'),
      matched(3, 's', 'AA', 'BB', 'CC'),
    ];
    const patternDone: Event = {
      seq: 11n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 0, elapsed: 5, buffer_seq: 3n,
    } as unknown as Event;
    const events: Event[] = [
      {
        seq: 10n, ts: 1, span: 7n, shell: 's', shell_marker: 's', source: null,
        kind: 'multi-match-start',
      } as unknown as Event,
      patternDone,
    ];
    const log = makeLogWithMultiMatch(buf, events, { '7': mmSpan });
    const idx = buildMultiMatchIndex(log);
    // For shell 'other', no per-pattern view applies.
    const other = replayBufferRegionsAtPerPatternDone(log, patternDone, 'other', idx);
    expect(other.tailSplit).toBeUndefined();
    expect(other.tail).toBe('XYZ');
  });

  it('falls back to the regular replay when the referenced buffer event is missing', () => {
    const patternDone: Event = {
      seq: 11n, ts: 3, span: 7n, shell: 's', shell_marker: 's', source: null,
      kind: 'multi-match-pattern-done',
      index: 0, elapsed: 5, buffer_seq: 999n,
    } as unknown as Event;
    const log = makeLogWithMultiMatch([], [patternDone], {});
    const result = replayBufferRegionsAtPerPatternDone(log, patternDone, 's');
    expect(result.tailSplit).toBeUndefined();
  });
});
