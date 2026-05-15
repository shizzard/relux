import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { buildCallStack } from './derive';
import {
  capturesAtSeq,
  capturesAtSpan,
  scopeContext,
  varsAtSeq,
  varsAtSpan,
} from './scope';

// ─── Span builders ──────────────────────────────────────────────────

type SpanInput = { id: number; parent: number | null } & (
  | { kind: 'test'; name?: string }
  | { kind: 'effect-setup'; effect: string; alias: string | null }
  | { kind: 'effect-cleanup'; effect: string; alias: string | null; setup_span: number }
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
    start_ts: 0,
    end_ts: null,
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
        alias: input.alias,
        marker: 'test-marker-0000',
        is_reuse: false,
      };
    case 'effect-cleanup':
      return {
        ...base,
        kind: 'effect-cleanup',
        effect: input.effect,
        alias: input.alias,
        setup_span: BigInt(input.setup_span),
        marker: 'test-marker-0000',
        is_deferred: false,
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

function ev(
  seq: number,
  span: number,
  shell: string | null,
  rest: Omit<Event, 'seq' | 'ts' | 'span' | 'shell'>,
): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    ...rest,
  } as Event;
}

function varLet(seq: number, span: number, shell: string | null, name: string, value: string): Event {
  return ev(seq, span, shell, { kind: 'var-let', name, value } as never);
}

function varAssign(
  seq: number,
  span: number,
  shell: string | null,
  name: string,
  value: string,
  previous = '',
): Event {
  return ev(seq, span, shell, {
    kind: 'var-assign',
    name,
    value,
    previous,
  } as never);
}

function exposeVar(
  seq: number,
  span: number,
  name: string,
  value: string,
  target: string = name,
  qualifier: string | null = null,
): Event {
  return ev(seq, span, null, {
    kind: 'effect-expose-var',
    name,
    target,
    qualifier,
    value,
  } as never);
}

function matchDone(
  seq: number,
  span: number,
  shell: string,
  captures: Record<string, string>,
): Event {
  return ev(seq, span, shell, {
    kind: 'match-done',
    matched: '',
    elapsed: 0,
    captures,
    buffer_seq: BigInt(seq),
  } as never);
}

// A "marker" event used purely as the selection target. Pick something
// innocuous — `log` works and doesn't interact with scope replay.
function marker(seq: number, span: number, shell: string | null): Event {
  return ev(seq, span, shell, { kind: 'log', message: 'mark' } as never);
}

// ─── Log builder ────────────────────────────────────────────────────

function makeLog(spans: Span[], events: Event[]): StructuredLog {
  return {
    test: { name: 't', path: 'p', outcome: 'pass', duration_ms: 0n },
    env: { bootstrap: [] },
    shells: {},
    spans: spansToMap(spans),
    events,
    buffer_events: [],
    failure: null,
    sources: {},
  };
}

// ─── scopeContext ───────────────────────────────────────────────────

describe('scopeContext', () => {
  it('returns the test span itself when called on a test span', () => {
    const log = makeLog([buildSpan({ kind: 'test', id: 1, parent: null })], []);
    expect(scopeContext(log, 1)).toEqual({ ambientScope: 1, innermostFn: null });
  });

  it('returns the test as ambient when called on a shell-block under test', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      ],
      [],
    );
    expect(scopeContext(log, 2)).toEqual({ ambientScope: 1, innermostFn: null });
  });

  it('reports the innermost fn-call ancestor and the test as ambient', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
        buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
      ],
      [],
    );
    expect(scopeContext(log, 3)).toEqual({ ambientScope: 1, innermostFn: 3 });
  });

  it('returns the innermost fn-call when fn-calls are nested', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'fn-call', id: 2, parent: 1 }),
        buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
      ],
      [],
    );
    expect(scopeContext(log, 3)).toEqual({ ambientScope: 1, innermostFn: 3 });
  });

  it('returns the effect-setup as ambient when inside one', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
        buildSpan({ kind: 'shell-block', id: 3, parent: 2, shell: 'eshell' }),
      ],
      [],
    );
    expect(scopeContext(log, 3)).toEqual({ ambientScope: 2, innermostFn: null });
  });

  it('returns nulls when the span id is unknown', () => {
    const log = makeLog([], []);
    expect(scopeContext(log, 99)).toEqual({ ambientScope: null, innermostFn: null });
  });

  it('hops through effect-cleanup to its linked setup_span', () => {
    // Cleanup is now parented under test (sibling of effect-setup), but
    // its scope still belongs to the originating effect.
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
        buildSpan({ kind: 'effect-cleanup', id: 3, parent: 1, effect: 'Eff', alias: 'E', setup_span: 2 }),
        buildSpan({ kind: 'cleanup-block', id: 4, parent: 3 }),
      ],
      [],
    );
    expect(scopeContext(log, 4)).toEqual({ ambientScope: 2, innermostFn: null });
  });

  it('reports innermostFn for fn-call inside cleanup-block, scope still hops to setup', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
        buildSpan({ kind: 'effect-cleanup', id: 3, parent: 1, effect: 'Eff', alias: 'E', setup_span: 2 }),
        buildSpan({ kind: 'cleanup-block', id: 4, parent: 3 }),
        buildSpan({ kind: 'fn-call', id: 5, parent: 4 }),
      ],
      [],
    );
    expect(scopeContext(log, 5)).toEqual({ ambientScope: 2, innermostFn: 5 });
  });
});

// ─── varsAtSeq — shell mode ─────────────────────────────────────────

describe('varsAtSeq (shell mode)', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    warnSpy.mockRestore();
  });

  it('returns an empty map for an empty log', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const sel = marker(1, 2, 'a');
    expect(varsAtSeq(makeLog(spans, [sel]), sel)).toEqual(new Map());
  });

  it('seeds the panel with a top-level test let (shell:null, span:test)', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', '42'),
      marker(2, 2, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(
      new Map([['X', '42']]),
    );
  });

  it('shows a shell-local let only in that shell', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'b' }),
    ];
    const letA = varLet(1, 2, 'a', 'shellA', 'va');
    const inA = marker(2, 2, 'a');
    const inB = marker(3, 3, 'b');
    const log = makeLog(spans, [letA, inA, inB]);
    expect(varsAtSeq(log, inA)).toEqual(new Map([['shellA', 'va']]));
    expect(varsAtSeq(log, inB)).toEqual(new Map());
  });

  it('cascades var-assign from one shell into test scope, visible in other shells', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'b' }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', 'orig'),
      varAssign(2, 2, 'a', 'X', 'updated', 'orig'),
      marker(3, 3, 'b'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[2]!)).toEqual(
      new Map([['X', 'updated']]),
    );
  });

  it('shell-local let shadows test-scope value; subsequent assign updates the shadow only', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'b' }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', 'test-orig'),
      varLet(2, 3, 'b', 'X', 'b-local'),
      varAssign(3, 3, 'b', 'X', 'b-updated', 'b-local'),
      marker(4, 3, 'b'),
      marker(5, 2, 'a'),
    ];
    const log = makeLog(spans, events);
    expect(varsAtSeq(log, events[3]!)).toEqual(new Map([['X', 'b-updated']]));
    expect(varsAtSeq(log, events[4]!)).toEqual(new Map([['X', 'test-orig']]));
  });

  it('warns and skips when var-assign cannot find a binding', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      varAssign(1, 2, 'a', 'mystery', 'x'),
      marker(2, 2, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(new Map());
    expect(warnSpy).toHaveBeenCalledOnce();
  });

  it('exposes effect-expose-var into the parent scope as `<alias>.<name>`', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Db', alias: 'Dep' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      exposeVar(1, 2, 'port', '5432'),
      marker(2, 3, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(
      new Map([['Dep.port', '5432']]),
    );
  });

  it('ignores effect-expose-var when the emitting effect has no alias', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Db', alias: null }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      exposeVar(1, 2, 'port', '5432'),
      marker(2, 3, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(new Map());
  });

  it('nested expose: re-exposed dep var lands in the outermost scope under the outer alias', () => {
    // Test starts E1 as E1; E1 starts E2 as E2.
    // E2 exposes foo. E1 re-exposes E2.foo as bar.
    // The test shell sees `E1.bar` with E2's original value; the
    // intermediate `E2.foo` lives in E1's scope and is invisible from
    // the test shell.
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'E1', alias: 'E1' }),
      buildSpan({ kind: 'effect-setup', id: 3, parent: 2, effect: 'E2', alias: 'E2' }),
      buildSpan({ kind: 'shell-block', id: 4, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      exposeVar(1, 3, 'foo', 'X'),
      ev(2, 2, null, {
        kind: 'effect-expose-var',
        name: 'bar',
        target: 'foo',
        qualifier: 'E2',
        value: 'X',
      } as never),
      marker(3, 4, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[2]!)).toEqual(
      new Map([['E1.bar', 'X']]),
    );
  });

  it('effect-level let is visible inside the effect but not in the test-only scope', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 2, shell: 'inner' }),
      buildSpan({ kind: 'shell-block', id: 4, parent: 1, shell: 'outer' }),
    ];
    const events: Event[] = [
      varLet(1, 2, null, 'inside', 'val'),
      marker(2, 3, 'inner'),
      marker(3, 4, 'outer'),
    ];
    const log = makeLog(spans, events);
    expect(varsAtSeq(log, events[1]!)).toEqual(new Map([['inside', 'val']]));
    expect(varsAtSeq(log, events[2]!)).toEqual(new Map());
  });

  it('cleanup-block sees the effect-level vars (via effect-cleanup hop)', () => {
    // test(1) -> effect-setup(2) declares `let X=v`.
    // test(1) -> effect-cleanup(3, setup_span=2) -> cleanup-block(4).
    // Selecting an event in cleanup-block must surface effect-level X.
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
      buildSpan({ kind: 'effect-cleanup', id: 3, parent: 1, effect: 'Eff', alias: 'E', setup_span: 2 }),
      buildSpan({ kind: 'cleanup-block', id: 4, parent: 3 }),
    ];
    const events = [
      varLet(1, 2, null, 'X', 'v'),
      marker(2, 4, '__cleanup'),
    ];
    const log = makeLog(spans, events);
    expect(varsAtSeq(log, events[1]!)).toEqual(new Map([['X', 'v']]));
  });

  it('cleanup-block does NOT see test-level vars (effect-scope isolation)', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'Eff', alias: 'E' }),
      buildSpan({ kind: 'effect-cleanup', id: 3, parent: 1, effect: 'Eff', alias: 'E', setup_span: 2 }),
      buildSpan({ kind: 'cleanup-block', id: 4, parent: 3 }),
    ];
    const events = [
      varLet(1, 1, null, 'Y', 'test-val'),
      varLet(2, 2, null, 'X', 'eff-val'),
      marker(3, 4, '__cleanup'),
    ];
    const log = makeLog(spans, events);
    expect(varsAtSeq(log, events[2]!)).toEqual(new Map([['X', 'eff-val']]));
  });
});

// ─── varsAtSeq — fn-call mode ───────────────────────────────────────

describe('varsAtSeq (fn-call mode)', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    warnSpy.mockRestore();
  });

  it('shows fn args at the very first event inside the fn-call', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({
        kind: 'fn-call',
        id: 3,
        parent: 2,
        args: [
          ['a', '1'],
          ['b', '2'],
        ],
      }),
    ];
    const sel = marker(1, 3, 'a');
    expect(varsAtSeq(makeLog(spans, [sel]), sel)).toEqual(
      new Map([
        ['a', '1'],
        ['b', '2'],
      ]),
    );
  });

  it('layers in-frame lets on top of args', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2, args: [['a', '1']] }),
    ];
    const events: Event[] = [
      varLet(1, 3, 'a', 'local', 'lv'),
      marker(2, 3, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(
      new Map([
        ['a', '1'],
        ['local', 'lv'],
      ]),
    );
  });

  it('does not surface outer shell vars while inside a fn-call', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2, args: [['x', 'arg']] }),
    ];
    const events: Event[] = [
      varLet(1, 2, 'a', 'shellVar', 'v'),
      marker(2, 3, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(
      new Map([['x', 'arg']]),
    );
  });

  it('frame-local lets are gone once the call returns', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      varLet(1, 3, 'a', 'frameVar', 'fv'),
      marker(2, 2, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[1]!)).toEqual(new Map());
  });

  it('nested fn-call seeds only from inner args', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2, args: [['outer', 'o']] }),
      buildSpan({ kind: 'fn-call', id: 4, parent: 3, args: [['inner', 'i']] }),
    ];
    const sel = marker(1, 4, 'a');
    expect(varsAtSeq(makeLog(spans, [sel]), sel)).toEqual(
      new Map([['inner', 'i']]),
    );
  });

  it('var-assign inside a fn-call frame updates the frame copy when the name was lifted there', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      varLet(1, 3, 'a', 'k', '1'),
      varAssign(2, 3, 'a', 'k', '2', '1'),
      marker(3, 3, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[2]!)).toEqual(
      new Map([['k', '2']]),
    );
  });

  it('var-assign inside a fn-call cascades to outer shell when frame has no shadow', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      varLet(1, 2, 'a', 'k', '1'),
      varAssign(2, 3, 'a', 'k', '2', '1'),
      marker(3, 2, 'a'),
    ];
    expect(varsAtSeq(makeLog(spans, events), events[2]!)).toEqual(
      new Map([['k', '2']]),
    );
  });
});

// ─── capturesAtSeq ──────────────────────────────────────────────────

describe('capturesAtSeq', () => {
  it('returns empty when shell is null', () => {
    const spans = [buildSpan({ kind: 'test', id: 1, parent: null })];
    const sel = marker(1, 1, null);
    expect(capturesAtSeq(makeLog(spans, [sel]), sel, null)).toEqual(new Map());
  });

  it('returns empty when no match-done has fired on the shell yet', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const sel = marker(1, 2, 'a');
    expect(capturesAtSeq(makeLog(spans, [sel]), sel, 'a')).toEqual(new Map());
  });

  it('returns the most recent match-done captures on the active shell', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'whole', host: 'localhost' }),
      marker(2, 2, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[1]!, 'a')).toEqual(
      new Map([
        ['0', 'whole'],
        ['host', 'localhost'],
      ]),
    );
  });

  it('a fresh match-done wholesale-replaces older captures (named keys disappear)', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'first', name: 'A' }),
      matchDone(2, 2, 'a', { '0': 'second' }),
      marker(3, 2, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[2]!, 'a')).toEqual(
      new Map([['0', 'second']]),
    );
  });

  it('ignores match-done from a different shell', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 1, shell: 'b' }),
    ];
    const events: Event[] = [
      matchDone(1, 3, 'b', { '0': 'other' }),
      marker(2, 2, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[1]!, 'a')).toEqual(
      new Map(),
    );
  });

  it("shows the fn-call's captures while selected event is inside the fn-call", () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'shell' }),
      matchDone(2, 3, 'a', { '0': 'frame' }),
      marker(3, 3, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[2]!, 'a')).toEqual(
      new Map([['0', 'frame']]),
    );
  });

  it("falls back to the outer shell's captures once the fn-call returned", () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'shell' }),
      matchDone(2, 3, 'a', { '0': 'frame' }),
      marker(3, 2, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[2]!, 'a')).toEqual(
      new Map([['0', 'shell']]),
    );
  });

  it('does not see outer-shell match-done captures from inside a fn-call', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'shell' }),
      marker(2, 3, 'a'),
    ];
    expect(capturesAtSeq(makeLog(spans, events), events[1]!, 'a')).toEqual(
      new Map(),
    );
  });
});

// ─── varsAtSpan ─────────────────────────────────────────────────────

describe('varsAtSpan', () => {
  it('returns null for spans that have no surfaceable outer scope (test span)', () => {
    const test = buildSpan({ kind: 'test', id: 1, parent: null });
    const log = makeLog([test], []);
    expect(varsAtSpan(log, test)).toBeNull();
  });

  it('returns null for an effect-setup span', () => {
    const test = buildSpan({ kind: 'test', id: 1, parent: null });
    const eff = buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'E', alias: 'E' });
    const log = makeLog([test, eff], []);
    expect(varsAtSpan(log, eff)).toBeNull();
  });

  it('returns the ambient test scope when a shell-block under test is selected', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', '42'),
      marker(2, 2, 'a'),
    ];
    expect(varsAtSpan(makeLog(spans, events), spans[1]!)).toEqual(
      new Map([['X', '42']]),
    );
  });

  it("uses the moment the span opens (vars added inside it don't appear)", () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', '42'),
      // This shell-local let is INSIDE the shell-block and must not
      // surface in the outer view.
      varLet(2, 2, 'a', 'inside', 'v'),
      marker(3, 2, 'a'),
    ];
    expect(varsAtSpan(makeLog(spans, events), spans[1]!)).toEqual(
      new Map([['X', '42']]),
    );
  });

  it('returns the effect scope when a shell-block under an effect is selected', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'E', alias: 'E' }),
      buildSpan({ kind: 'shell-block', id: 3, parent: 2, shell: 'es' }),
    ];
    const events: Event[] = [
      varLet(1, 2, null, 'EffectVar', 'v'),
      marker(2, 3, 'es'),
    ];
    expect(varsAtSpan(makeLog(spans, events), spans[2]!)).toEqual(
      new Map([['EffectVar', 'v']]),
    );
  });

  it('returns the caller shell scope when a fn-call from a shell-block is selected', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2, args: [['arg', 'v']] }),
    ];
    const events: Event[] = [
      varLet(1, 1, null, 'X', 'test-val'),
      varLet(2, 2, 'a', 'shellVar', 'sv'),
      marker(3, 3, 'a'),
    ];
    // Selecting the fn-call span returns the caller's view: test
    // ambient vars + shell-local lets, NOT the fn args.
    expect(varsAtSpan(makeLog(spans, events), spans[2]!)).toEqual(
      new Map([
        ['X', 'test-val'],
        ['shellVar', 'sv'],
      ]),
    );
  });

  it("returns the outer fn's frame when a nested fn-call is selected", () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2, args: [['outer', 'o']] }),
      buildSpan({ kind: 'fn-call', id: 4, parent: 3, args: [['inner', 'i']] }),
    ];
    const events: Event[] = [
      varLet(1, 3, 'a', 'frameVar', 'fv'),
      marker(2, 4, 'a'),
    ];
    // Selecting the inner fn-call returns the outer fn's frame: args
    // plus any in-frame lets emitted before the inner call opened.
    expect(varsAtSpan(makeLog(spans, events), spans[3]!)).toEqual(
      new Map([
        ['outer', 'o'],
        ['frameVar', 'fv'],
      ]),
    );
  });

  it("returns null when a fn-call is selected whose caller is the test span (pure-init context)", () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'fn-call', id: 2, parent: 1 }),
    ];
    const log = makeLog(spans, []);
    expect(varsAtSpan(log, spans[1]!)).toBeNull();
  });

  it('returns the effect scope when a cleanup-block under effect-cleanup is selected', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'effect-setup', id: 2, parent: 1, effect: 'E', alias: 'E' }),
      buildSpan({ kind: 'effect-cleanup', id: 3, parent: 1, effect: 'E', alias: 'E', setup_span: 2 }),
      buildSpan({ kind: 'cleanup-block', id: 4, parent: 3 }),
    ];
    const events: Event[] = [
      varLet(1, 2, null, 'EffectVar', 'v'),
      // Test-scope var must NOT appear in the cleanup outer view.
      varLet(2, 1, null, 'TestVar', 't'),
      marker(3, 4, '__cleanup'),
    ];
    expect(varsAtSpan(makeLog(spans, events), spans[3]!)).toEqual(
      new Map([['EffectVar', 'v']]),
    );
  });
});

// ─── capturesAtSpan ────────────────────────────────────────────────

describe('capturesAtSpan', () => {
  it('returns null for spans with no surfaceable outer scope', () => {
    const test = buildSpan({ kind: 'test', id: 1, parent: null });
    expect(capturesAtSpan(makeLog([test], []), test)).toBeNull();
  });

  it('returns the caller shell captures for a fn-call selected from a shell', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
      buildSpan({ kind: 'fn-call', id: 3, parent: 2 }),
    ];
    const events: Event[] = [
      matchDone(1, 2, 'a', { '0': 'before-call' }),
      // A match-done INSIDE the fn-call must not surface in the outer view.
      matchDone(2, 3, 'a', { '0': 'inside-call' }),
      marker(3, 3, 'a'),
    ];
    expect(capturesAtSpan(makeLog(spans, events), spans[2]!)).toEqual(
      new Map([['0', 'before-call']]),
    );
  });

  it('returns an empty map for a shell-block (no captures live on test/effect scope)', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'a' }),
    ];
    // The shell-block's caller is the test scope, which has no shell.
    expect(capturesAtSpan(makeLog(spans, []), spans[1]!)).toEqual(new Map());
  });
});

describe('scopeContext — transparent BIFs', () => {
  it('skips a transparent pure-BIF fn-call when computing innermostFn', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'sh' }),
        buildSpan({
          kind: 'fn-call',
          id: 3,
          parent: 2,
          name: 'trim',
          callee_kind: 'bif',
          is_pure: true,
        }),
      ],
      [],
    );
    const ctx = scopeContext(log, 3);
    expect(ctx.innermostFn).toBeNull();
    expect(ctx.ambientScope).toBe(1);
  });

  it('keeps a user fn-call as innermostFn', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'sh' }),
        buildSpan({
          kind: 'fn-call',
          id: 3,
          parent: 2,
          name: 'my_helper',
          callee_kind: 'user',
          is_pure: false,
        }),
      ],
      [],
    );
    expect(scopeContext(log, 3).innermostFn).toBe(3);
  });

  it('skips a transparent log fn-call from buildCallStack', () => {
    const spans = [
      buildSpan({ kind: 'test', id: 1, parent: null }),
      buildSpan({ kind: 'shell-block', id: 2, parent: 1, shell: 'sh' }),
      buildSpan({
        kind: 'fn-call',
        id: 3,
        parent: 2,
        name: 'trim',
        callee_kind: 'bif',
        is_pure: true,
      }),
    ];
    const log = makeLog(spans, [marker(1, 3, 'sh')]);
    const stack = buildCallStack(log, log.events[0]!);
    expect(stack.map((f) => f.kind)).toEqual(['test', 'shell-block']);
  });

  it('skips a transparent log fn-call', () => {
    const log = makeLog(
      [
        buildSpan({ kind: 'test', id: 1, parent: null }),
        buildSpan({
          kind: 'fn-call',
          id: 2,
          parent: 1,
          name: 'log',
          callee_kind: 'bif',
          is_pure: false,
        }),
      ],
      [],
    );
    expect(scopeContext(log, 2).innermostFn).toBeNull();
  });
});
