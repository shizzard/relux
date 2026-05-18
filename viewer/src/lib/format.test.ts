import { describe, expect, it } from 'vitest';
import type { CancelReasonRecord } from '../types/CancelReasonRecord';
import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { TimeoutValue } from '../types/TimeoutValue';
import type { FoldedEvent } from './flatten';
import {
  cancelReasonSummary,
  displayMarkerDecision,
  displayMarkerKind,
  displayMarkerModifier,
  displaySpanCallKind,
  displaySpanKind,
  escapeBufferBytes,
  escapeBytes,
  eventSummary,
  foldedFamily,
  foldedGlyph,
  foldedKindLabel,
  foldedSummary,
  formatBytes,
  formatDuration,
  formatTimeout,
  formatTimeoutLine,
  formatTimestamp,
  kindFamily,
  kindGlyph,
  spanTitle,
  truncate,
} from './format';

// ─── Fixtures ───────────────────────────────────────────────────────

function span(id: number, parent: number | null, k: Span['kind'] | Span): Span {
  if (typeof k !== 'string') return k;
  // Stub used only for kinds that take no payload.
  return {
    id: BigInt(id),
    parent: parent === null ? null : BigInt(parent),
    start_ts: 0,
    end_ts: null,
    location: null,
    kind: k as 'cleanup-block' | 'markers',
  } as Span;
}

function testSpan(name = 't'): Span {
  return { id: 1n, parent: null, start_ts: 0, end_ts: null, location: null, kind: 'test', name } as Span;
}

function fnCallSpan(opts: {
  name?: string;
  argc?: number;
  result?: string | null;
  callee_kind?: 'user' | 'bif';
} = {}): Span {
  return {
    id: 2n,
    parent: 1n,
    start_ts: 0,
    end_ts: null,
    location: null,
    kind: 'fn-call',
    name: opts.name ?? 'f',
    args: Array.from({ length: opts.argc ?? 0 }, (_, i) => [`a${i}`, `v${i}`] as [string, string]),
    result: opts.result ?? null,
    callee_kind: opts.callee_kind ?? 'user',
    is_pure: false,
  } as Span;
}

function ev<K extends Event['kind']>(kind: K, extra: Record<string, unknown> = {}): Event {
  return {
    seq: 1n,
    ts: 0,
    span: 1n,
    shell: null,
    shell_marker: null,
    source: null,
    kind,
    ...extra,
  } as unknown as Event;
}

// ─── formatTimestamp / formatDuration ───────────────────────────────

describe('formatTimestamp', () => {
  it('renders sub-1s in milliseconds (integer)', () => {
    expect(formatTimestamp(0)).toBe('0ms');
    expect(formatTimestamp(999)).toBe('999ms');
    expect(formatTimestamp(42.7)).toBe('43ms');
  });

  it('renders 1s–60s with two fractional digits', () => {
    expect(formatTimestamp(1000)).toBe('1.00s');
    expect(formatTimestamp(1234)).toBe('1.23s');
    expect(formatTimestamp(59_999)).toBe('60.00s');
  });

  it('renders ≥60s in `Xm Ys` form', () => {
    expect(formatTimestamp(60_000)).toBe('1m 0s');
    expect(formatTimestamp(125_000)).toBe('2m 5s');
  });

  it('formatDuration is a synonym', () => {
    expect(formatDuration(500)).toBe(formatTimestamp(500));
    expect(formatDuration(70_000)).toBe(formatTimestamp(70_000));
  });
});

// ─── escapeBytes / escapeBufferBytes ────────────────────────────────

describe('escapeBytes', () => {
  it('passes printable ASCII through unchanged', () => {
    expect(escapeBytes('hello world')).toBe('hello world');
  });

  it('escapes CR, TAB, and other control bytes; LF becomes `\\n\\n`', () => {
    // LF is `\n\n` (literal backslash-n followed by a real newline) so the
    // viewer can render the escape *and* break the line in `<pre>`.
    expect(escapeBytes('a\nb')).toBe('a\\n\nb');
    expect(escapeBytes('a\rb')).toBe('a\\rb');
    expect(escapeBytes('a\tb')).toBe('a\\tb');
    expect(escapeBytes('\x00\x01\x7f')).toBe('\\x00\\x01\\x7f');
  });

  it('preserves non-ASCII printables (no escaping outside the C0/DEL range)', () => {
    expect(escapeBytes('café — \u{1F600}')).toBe('café — \u{1F600}');
  });
});

describe('escapeBufferBytes', () => {
  it('strips CR, lets LF and TAB pass through, escapes other control bytes', () => {
    expect(escapeBufferBytes('a\r\nb\tc')).toBe('a\nb\tc');
    expect(escapeBufferBytes('\x00\x01\x7f')).toBe('\\x00\\x01\\x7f');
  });

  it('passes printable input through unchanged', () => {
    expect(escapeBufferBytes('hello')).toBe('hello');
  });
});

// ─── kindGlyph / kindFamily ─────────────────────────────────────────

describe('kindGlyph', () => {
  it('returns the configured glyph for a known kind', () => {
    expect(kindGlyph('send')).toBe('\u{2192}'); // →
    expect(kindGlyph('recv')).toBe('\u{2190}'); // ←
    expect(kindGlyph('error')).toBe('\u{2717}'); // ✗
  });

  it('returns the bullet fallback for unknown kinds', () => {
    // Cast through `as` so the test compiles even if the kind union changes.
    expect(kindGlyph('not-a-real-kind' as Event['kind'])).toBe('\u{2022}');
  });
});

describe('kindFamily', () => {
  it('returns the configured family for an annotated kind', () => {
    expect(kindFamily('send')).toBe('ok');
    expect(kindFamily('timeout')).toBe('danger');
    expect(kindFamily('log')).toBe('info');
  });

  it('returns `event` for unannotated kinds', () => {
    expect(kindFamily('var-let')).toBe('event');
    expect(kindFamily('match-start')).toBe('event');
  });
});

// ─── formatBytes ────────────────────────────────────────────────────

describe('formatBytes', () => {
  it('uses plain `B` below 1024', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
  });

  it('keeps a fractional digit for values under 10 in the next unit', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
  });

  it('drops the fractional digit once the value is >= 10', () => {
    expect(formatBytes(10 * 1024)).toBe('10 KB');
    expect(formatBytes(123 * 1024)).toBe('123 KB');
  });

  it('promotes through MB and GB units', () => {
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(5 * 1024 * 1024 * 1024)).toBe('5.0 GB');
  });
});

// ─── formatTimeout / formatTimeoutLine ──────────────────────────────

function tolerance(opts: {
  duration?: string;
  multiplier?: string;
  total?: string;
  src?: TimeoutValue['source'];
} = {}): TimeoutValue {
  return {
    type: 'tolerance',
    duration: opts.duration ?? '5s',
    multiplier: opts.multiplier ?? '1.0',
    total_duration: opts.total ?? opts.duration ?? '5s',
    source: opts.src ?? null,
  };
}

function assertion(opts: { duration?: string; src?: TimeoutValue['source'] } = {}): TimeoutValue {
  return {
    type: 'assertion',
    duration: opts.duration ?? '5s',
    source: opts.src ?? null,
  };
}

describe('formatTimeout', () => {
  it('emits the bare duration when tolerance multiplier is 1.0', () => {
    expect(formatTimeout(tolerance({ duration: '5s' }))).toBe('5s');
  });

  it('appends multiplier when not 1.0', () => {
    expect(formatTimeout(tolerance({ duration: '5s', multiplier: '1.5' }))).toBe('5s \u{00D7} 1.5');
  });

  it('marks assertions as `exact`', () => {
    expect(formatTimeout(assertion({ duration: '30s' }))).toBe('30s exact');
  });
});

describe('formatTimeoutLine', () => {
  it('shows the source location when present, `default` when not', () => {
    expect(formatTimeoutLine(tolerance({ src: { file: 'foo.relux', line: 12, start: 0, end: 0 } }))).toBe(
      '5s (foo.relux:12)',
    );
    expect(formatTimeoutLine(tolerance())).toBe('5s (default)');
  });

  it('expands tolerance×multiplier to the computed total', () => {
    const v = tolerance({
      duration: '5s',
      multiplier: '1.5',
      total: '7.5s',
      src: { file: 'foo.relux', line: 12, start: 0, end: 0 },
    });
    expect(formatTimeoutLine(v)).toBe('5s \u{00D7} 1.5 = 7.5s (foo.relux:12)');
  });

  it('treats assertions the same as 1.0-multiplier tolerances (no expansion)', () => {
    expect(formatTimeoutLine(assertion())).toBe('5s (default)');
  });
});

// ─── truncate ──────────────────────────────────────────────────────

describe('truncate', () => {
  it('returns the input unchanged when within budget', () => {
    expect(truncate('hello', 5)).toBe('hello');
    expect(truncate('hi', 10)).toBe('hi');
  });

  it('replaces the overflow tail with a single-character ellipsis', () => {
    expect(truncate('abcdef', 4)).toBe('abc\u{2026}');
    expect(truncate('abcdef', 6)).toBe('abcdef'); // exactly at budget
  });
});

// ─── eventSummary ──────────────────────────────────────────────────

describe('eventSummary', () => {
  it('summarises send/recv via escapeBytes', () => {
    expect(eventSummary(ev('send', { data: 'hi\n' }))).toBe('hi\\n\n');
    expect(eventSummary(ev('recv', { data: 'ok' }))).toBe('ok');
  });

  it('summarises match-start with regex/literal label and timeout', () => {
    const e = ev('match-start', {
      pattern: 'ready',
      is_regex: true,
      effective: tolerance({ duration: '5s' }),
    });
    expect(eventSummary(e)).toBe('regex ready (\u{2264} 5s)');
  });

  it('summarises match-done with elapsed time and matched bytes', () => {
    const e = ev('match-done', { matched: 'hi', elapsed: 42, captures: null, buffer_seq: 1n });
    expect(eventSummary(e)).toBe('42ms hi');
  });

  it('summarises timeout with pattern and effective timeout', () => {
    const e = ev('timeout', {
      pattern: 'ready',
      buffer_seq: null,
      effective: assertion({ duration: '30s' }),
    });
    expect(eventSummary(e)).toBe('ready after 30s exact');
  });

  it('summarises fail-pattern-cleared as the empty string', () => {
    expect(eventSummary(ev('fail-pattern-cleared'))).toBe('');
  });

  it('summarises var-let / var-assign / var-read as `name = value`', () => {
    expect(eventSummary(ev('var-let', { name: 'x', value: 'v' }))).toBe('x = v');
    expect(eventSummary(ev('var-assign', { name: 'x', value: 'v', previous: 'p' }))).toBe('x = v');
    expect(eventSummary(ev('var-read', { name: 'x', value: 'v' }))).toBe('x = v');
  });

  it('summarises pure-match with arrow and `no match` when result is empty', () => {
    const a = ev('pure-match', { match_kind: 'regex', value: 'abc', pattern: '.', result: '', captures: {} });
    expect(eventSummary(a)).toBe('. \u{2192} (no match)');
    const b = ev('pure-match', { match_kind: 'regex', value: 'abc', pattern: '.', result: 'a', captures: {} });
    expect(eventSummary(b)).toBe('. \u{2192} a');
  });

  it('summarises bool-check across all four shapes', () => {
    expect(eventSummary(ev('bool-check', { evaluation: { shape: 'unconditional' } }))).toBe(
      'unconditional',
    );
    expect(
      eventSummary(ev('bool-check', { evaluation: { shape: 'bare', value: 'v', met: true } })),
    ).toBe('"v" \u{2192} true');
    expect(
      eventSummary(ev('bool-check', { evaluation: { shape: 'eq', lhs: 'L', rhs: 'R', met: false } })),
    ).toBe('"L" = "R" \u{2192} false');
    expect(
      eventSummary(
        ev('bool-check', { evaluation: { shape: 'regex', value: 'abc', pattern: '.', met: true } }),
      ),
    ).toBe('"abc" ? . \u{2192} true');
  });

  it('summarises log/warning/error/annotate as their message text', () => {
    expect(eventSummary(ev('log', { message: 'm' }))).toBe('m');
    expect(eventSummary(ev('warning', { message: 'w' }))).toBe('w');
    expect(eventSummary(ev('error', { message: 'e' }))).toBe('e');
    expect(eventSummary(ev('annotate', { text: 'a' }))).toBe('a');
  });

  it('summarises shell-spawn as `name: command`, other shell ops as the name', () => {
    expect(eventSummary(ev('shell-spawn', { name: 's', command: '/bin/sh' }))).toBe('s: /bin/sh');
    expect(eventSummary(ev('shell-ready', { name: 's' }))).toBe('s');
    expect(eventSummary(ev('shell-switch', { name: 's' }))).toBe('s');
    expect(eventSummary(ev('shell-terminate', { name: 's' }))).toBe('s');
  });

  it('summarises effect-expose-shell with qualifier when re-exposing', () => {
    expect(
      eventSummary(ev('effect-expose-shell', { name: 'inner', target: 'inner', qualifier: null })),
    ).toBe('inner');
    expect(
      eventSummary(ev('effect-expose-shell', { name: 'inner', target: 'inner', qualifier: 'Db' })),
    ).toBe('inner \u{2190} Db.inner');
  });

  it('summarises cancelled via cancelReasonSummary', () => {
    expect(eventSummary(ev('cancelled', { reason: { type: 'sigint' } }))).toBe('sigint');
  });
});

// ─── cancelReasonSummary ────────────────────────────────────────────

describe('cancelReasonSummary', () => {
  it('formats all four variants', () => {
    const variants: [CancelReasonRecord, string][] = [
      [{ type: 'test-timeout', duration_ms: 5000n } as CancelReasonRecord, 'test-timeout (duration 5000ms)'],
      [
        { type: 'suite-timeout', duration_ms: 30000n } as CancelReasonRecord,
        'suite-timeout (duration 30000ms)',
      ],
      [
        { type: 'fail-fast', trigger_test: 'foo' } as CancelReasonRecord,
        'fail-fast (triggered by foo)',
      ],
      [{ type: 'sigint' } as CancelReasonRecord, 'sigint'],
    ];
    for (const [reason, expected] of variants) {
      expect(cancelReasonSummary(reason)).toBe(expected);
    }
  });
});

// ─── folded helpers ─────────────────────────────────────────────────

describe('folded helpers', () => {
  const single: FoldedEvent = { kind: 'single', event: ev('send', { data: 'x' }) };
  const sleep: FoldedEvent = {
    kind: 'sleep',
    start: ev('sleep-start', { duration: 100 }),
    done: ev('sleep-done'),
  };
  const matchOk: FoldedEvent = {
    kind: 'match',
    start: ev('match-start', { pattern: 'p', is_regex: false, effective: tolerance() }),
    outcome: ev('match-done', { matched: 'm', elapsed: 5, captures: null, buffer_seq: 1n }),
  };
  const matchTimeout: FoldedEvent = {
    kind: 'match',
    start: ev('match-start', { pattern: 'p', is_regex: false, effective: tolerance() }),
    outcome: ev('timeout', { pattern: 'p', buffer_seq: null, effective: tolerance({ duration: '5s' }) }),
  };

  it('foldedGlyph delegates to the lead kind for singles, fixed for sleep, outcome kind for match', () => {
    expect(foldedGlyph(single)).toBe(kindGlyph('send'));
    expect(foldedGlyph(sleep)).toBe(kindGlyph('sleep-start'));
    expect(foldedGlyph(matchOk)).toBe(kindGlyph('match-done'));
    expect(foldedGlyph(matchTimeout)).toBe(kindGlyph('timeout'));
  });

  it('foldedFamily folds sleep to info and uses the outcome family for match', () => {
    expect(foldedFamily(single)).toBe('ok');
    expect(foldedFamily(sleep)).toBe('info');
    expect(foldedFamily(matchOk)).toBe('ok');
    expect(foldedFamily(matchTimeout)).toBe('danger');
  });

  it('foldedKindLabel collapses pairs to a single label', () => {
    expect(foldedKindLabel(single)).toBe('send');
    expect(foldedKindLabel(sleep)).toBe('sleep');
    expect(foldedKindLabel(matchOk)).toBe('match');
    expect(foldedKindLabel(matchTimeout)).toBe('match');
  });

  it('foldedSummary stitches the pair into a single line', () => {
    expect(foldedSummary(single)).toBe(eventSummary(single.event));
    expect(foldedSummary(sleep)).toBe('100ms');
    expect(foldedSummary(matchOk)).toBe('p \u{2192} m (5ms)');
    expect(foldedSummary(matchTimeout)).toBe('p timed out after 5s');
  });
});

// ─── span display ──────────────────────────────────────────────────

describe('displaySpanKind / displaySpanCallKind', () => {
  it('maps schema strings to DSL-aligned labels', () => {
    expect(displaySpanKind('effect-setup')).toBe('setup');
    expect(displaySpanKind('effect-cleanup')).toBe('cleanup');
    expect(displaySpanKind('shell-block')).toBe('shell');
    expect(displaySpanKind('fn-call')).toBe('call');
    expect(displaySpanKind('markers')).toBe('MARKERS');
    expect(displaySpanKind('marker-eval')).toBe('marker');
  });

  it('passes unmapped kinds through unchanged', () => {
    expect(displaySpanKind('test')).toBe('test');
    expect(displaySpanKind('cleanup-block')).toBe('cleanup-block');
  });

  it('displaySpanCallKind labels BIF fn-calls as `BIF`, others fall back to displaySpanKind', () => {
    expect(displaySpanCallKind(fnCallSpan({ callee_kind: 'bif' }))).toBe('BIF');
    expect(displaySpanCallKind(fnCallSpan({ callee_kind: 'user' }))).toBe('call');
    expect(displaySpanCallKind(testSpan('t'))).toBe('test');
  });
});

describe('spanTitle', () => {
  it('uses the test name for `test`', () => {
    expect(spanTitle(testSpan('login-flow'))).toBe('login-flow');
  });

  it('includes the alias for effect-setup when present', () => {
    const noAlias: Span = {
      id: 1n,
      parent: null,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'effect-setup',
      effect: 'Db',
      overlay: [],
      alias: null,
      marker: 'M',
      is_reuse: false,
    } as Span;
    const withAlias: Span = { ...noAlias, alias: 'd' } as Span;
    expect(spanTitle(noAlias)).toBe('Db');
    expect(spanTitle(withAlias)).toBe('Db as d');
  });

  it('renders effect-cleanup as the effect name', () => {
    const s: Span = {
      id: 1n,
      parent: null,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'effect-cleanup',
      effect: 'Db',
      alias: null,
      setup_span: 1n,
      marker: 'M',
      is_deferred: false,
    } as Span;
    expect(spanTitle(s)).toBe('Db');
  });

  it('renders shell-block as the shell name and cleanup-block as the literal `cleanup`', () => {
    const sb: Span = {
      id: 1n,
      parent: null,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'shell-block',
      shell: 's',
    } as Span;
    expect(spanTitle(sb)).toBe('s');
    expect(spanTitle(span(1, null, 'cleanup-block'))).toBe('cleanup');
  });

  it('renders fn-call with arity, and appends the result arrow when present', () => {
    expect(spanTitle(fnCallSpan({ name: 'f', argc: 2, result: null }))).toBe('f/2');
    expect(spanTitle(fnCallSpan({ name: 'f', argc: 0, result: 'ok' }))).toBe('f/0 \u{2192} "ok"');
    // Result is byte-escaped:
    expect(spanTitle(fnCallSpan({ name: 'f', argc: 0, result: 'hi\n' }))).toBe('f/0 \u{2192} "hi\\n\n"');
  });

  it('renders markers as the empty string', () => {
    expect(spanTitle(span(1, null, 'markers'))).toBe('');
  });

  it('renders marker-eval as `#kind modifier → decision`', () => {
    const me: Span = {
      id: 1n,
      parent: null,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'marker-eval',
      marker_kind: 'skip',
      modifier: 'if',
      decision: 'mark',
    } as Span;
    expect(spanTitle(me)).toBe('#skip if \u{2192} mark');
  });
});

// ─── marker display helpers ────────────────────────────────────────

describe('marker display helpers', () => {
  it('displayMarkerKind prefixes with `#`', () => {
    expect(displayMarkerKind('skip')).toBe('#skip');
    expect(displayMarkerKind('run')).toBe('#run');
    expect(displayMarkerKind('flaky')).toBe('#flaky');
  });

  it('displayMarkerModifier and displayMarkerDecision are identity', () => {
    expect(displayMarkerModifier('if')).toBe('if');
    expect(displayMarkerModifier('unless')).toBe('unless');
    expect(displayMarkerDecision('pass')).toBe('pass');
    expect(displayMarkerDecision('mark')).toBe('mark');
  });
});
