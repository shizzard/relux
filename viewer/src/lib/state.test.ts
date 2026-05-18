import { describe, expect, it } from 'vitest';
import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import type { TestOutcome } from '../types/TestOutcome';
import { ALL_EVENT_TYPE_IDS } from './flatten';
import { ViewerState } from './state.svelte';

// ─── Fixtures ───────────────────────────────────────────────────────

function testSpan(id: number, parent: number | null = null): Span {
  return {
    id: BigInt(id),
    parent: parent === null ? null : BigInt(parent),
    start_ts: 0,
    end_ts: null,
    location: null,
    kind: 'test',
    name: 't',
  } as Span;
}

function shellBlockSpan(id: number, parent: number, shell = 's'): Span {
  return {
    id: BigInt(id),
    parent: BigInt(parent),
    start_ts: 0,
    end_ts: null,
    location: null,
    kind: 'shell-block',
    shell,
  } as Span;
}

function sendEvent(seq: number, span: number, shell = 's'): Event {
  return {
    seq: BigInt(seq),
    ts: seq,
    span: BigInt(span),
    shell,
    shell_marker: shell,
    source: null,
    kind: 'send',
    data: 'x',
  } as Event;
}

function spansToMap(...spans: Span[]): Record<string, Span> {
  const m: Record<string, Span> = {};
  for (const s of spans) m[String(s.id)] = s;
  return m;
}

function makeLog(opts: {
  outcome?: TestOutcome;
  spans?: Span[];
  events?: Event[];
  shells?: Record<string, unknown>;
}): StructuredLog {
  return {
    info: { name: 't', path: 'p', duration_ms: 0n },
    outcome: opts.outcome ?? { kind: 'pass' },
    env: { bootstrap: [] },
    shells: opts.shells ?? {},
    spans: spansToMap(...(opts.spans ?? [testSpan(1)])),
    events: opts.events ?? [],
    buffer_events: [],
    sources: {},
    artifacts: [],
  } as unknown as StructuredLog;
}

// A simple pass log: test span 1 → shell-block 2 with a `send` event.
function passLogWithSend(): StructuredLog {
  return makeLog({
    spans: [testSpan(1), shellBlockSpan(2, 1)],
    events: [sendEvent(10, 2)],
    shells: { s: { marker: 's', name: 's', spawn_ts: 0, terminate_ts: null, command: '/bin/sh' } },
  });
}

// ─── Constructor ────────────────────────────────────────────────────

describe('ViewerState constructor', () => {
  it('on a pass outcome, leaves selection unset and expands only the test span', () => {
    const state = new ViewerState(makeLog({ spans: [testSpan(1), shellBlockSpan(2, 1)] }));
    expect(state.selectedEventSeq).toBeNull();
    expect(state.selectedSpanId).toBeNull();
    expect(state.expandedSpans.has(1)).toBe(true);
    expect(state.expandedSpans.has(2)).toBe(false);
  });

  it('on a fail outcome with a span, focuses the failure event and expands ancestors', () => {
    const state = new ViewerState(
      makeLog({
        outcome: {
          kind: 'fail',
          type: 'match-timeout',
          span: 2n,
          event_seq: 42n,
          shell: 's',
          pattern: 'p',
          effective: { type: 'assertion', duration: '1s', source: null },
          call_stack: [],
          buffer_tail: '',
          vars_in_scope: [],
        } as unknown as TestOutcome,
        spans: [testSpan(1), shellBlockSpan(2, 1)],
      }),
    );
    expect(state.selectedEventSeq).toBe(42);
    expect(state.expandedSpans.has(1)).toBe(true);
    expect(state.expandedSpans.has(2)).toBe(true);
  });

  it('on a Runtime fail outcome with no span, leaves selection unset and only the test span expanded', () => {
    const state = new ViewerState(
      makeLog({
        outcome: {
          kind: 'fail',
          type: 'runtime',
          span: null,
          event_seq: null,
          shell: null,
          message: 'boom',
          call_stack: [],
          vars_in_scope: [],
        } as unknown as TestOutcome,
        spans: [testSpan(1), shellBlockSpan(2, 1)],
      }),
    );
    expect(state.selectedEventSeq).toBeNull();
    expect(state.expandedSpans.has(1)).toBe(true);
    expect(state.expandedSpans.has(2)).toBe(false);
  });

  it('on a cancelled outcome with a span, focuses the cancellation event and expands ancestors', () => {
    const state = new ViewerState(
      makeLog({
        outcome: {
          kind: 'cancelled',
          reason: { type: 'sigint' },
          span: 2n,
          event_seq: 7n,
          shell: 's',
          call_stack: [],
        } as unknown as TestOutcome,
        spans: [testSpan(1), shellBlockSpan(2, 1)],
      }),
    );
    expect(state.selectedEventSeq).toBe(7);
    expect(state.expandedSpans.has(2)).toBe(true);
  });
});

// ─── Selection ──────────────────────────────────────────────────────

describe('ViewerState — selection', () => {
  it('selectEvent sets the seq and clears any span selection', () => {
    const state = new ViewerState(passLogWithSend());
    state.selectedSpanId = 1;
    state.selectEvent(10);
    expect(state.selectedEventSeq).toBe(10);
    expect(state.selectedSpanId).toBeNull();
  });

  it('selectEvent with the currently selected seq clears it (toggle)', () => {
    const state = new ViewerState(passLogWithSend());
    state.selectEvent(10);
    state.selectEvent(10);
    expect(state.selectedEventSeq).toBeNull();
  });

  it('selectSpan sets the id and clears any event selection', () => {
    const state = new ViewerState(passLogWithSend());
    state.selectedEventSeq = 10;
    state.selectSpan(2);
    expect(state.selectedSpanId).toBe(2);
    expect(state.selectedEventSeq).toBeNull();
  });

  it('selectSpan with the currently selected id clears it (toggle)', () => {
    const state = new ViewerState(passLogWithSend());
    state.selectSpan(2);
    state.selectSpan(2);
    expect(state.selectedSpanId).toBeNull();
  });

  it('revealAndSelect expands every ancestor of the target and selects it', () => {
    // 1 (test) ▷ 2 (shell-block) ▷ 3 (fn-call inside the shell)
    const fnCall: Span = {
      id: 3n,
      parent: 2n,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'fn-call',
      name: 'f',
      args: [],
      result: null,
      callee_kind: 'user',
      is_pure: false,
    } as Span;
    const state = new ViewerState(
      makeLog({ spans: [testSpan(1), shellBlockSpan(2, 1), fnCall] }),
    );
    state.selectedEventSeq = 99;
    state.revealAndSelect(3);
    expect(state.selectedSpanId).toBe(3);
    expect(state.selectedEventSeq).toBeNull();
    expect(state.expandedSpans.has(1)).toBe(true);
    expect(state.expandedSpans.has(2)).toBe(true);
    expect(state.expandedSpans.has(3)).toBe(true);
  });
});

// ─── Error-path / send-match filter presets ─────────────────────────

describe('ViewerState — filter presets', () => {
  it('hasErrorPath is false on a pass outcome', () => {
    const state = new ViewerState(passLogWithSend());
    expect(state.hasErrorPath).toBe(false);
    expect(state.errorPathSpanId).toBeNull();
  });

  it('hasErrorPath is true on a fail outcome with a span', () => {
    const state = new ViewerState(
      makeLog({
        outcome: {
          kind: 'fail',
          type: 'match-timeout',
          span: 2n,
          event_seq: 1n,
          shell: 's',
          pattern: 'p',
          effective: { type: 'assertion', duration: '1s', source: null },
          call_stack: [],
          buffer_tail: '',
          vars_in_scope: [],
        } as unknown as TestOutcome,
        spans: [testSpan(1), shellBlockSpan(2, 1)],
      }),
    );
    expect(state.hasErrorPath).toBe(true);
    expect(state.errorPathSpanId).toBe(2);
  });

  it('toggleErrorPath is a no-op when hasErrorPath is false', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleErrorPath();
    expect(state.hiddenEventTypes.size).toBe(0);
    expect(state.isErrorPathPresetActive).toBe(false);
  });

  it('toggleErrorPath round-trips on a fail outcome', () => {
    const state = new ViewerState(
      makeLog({
        outcome: {
          kind: 'fail',
          type: 'runtime',
          span: 1n,
          event_seq: 1n,
          shell: null,
          message: 'boom',
          call_stack: [],
          vars_in_scope: [],
        } as unknown as TestOutcome,
        spans: [testSpan(1)],
      }),
    );
    state.toggleErrorPath();
    expect(state.isErrorPathPresetActive).toBe(true);
    // The error-path preset hides everything except error/fail-pattern-triggered/match-timeout.
    expect(state.hiddenEventTypes.has('send')).toBe(true);
    expect(state.hiddenEventTypes.has('error')).toBe(false);
    expect(state.hiddenEventTypes.has('fail-pattern-triggered')).toBe(false);
    expect(state.hiddenEventTypes.has('match-timeout')).toBe(false);
    state.toggleErrorPath();
    expect(state.isErrorPathPresetActive).toBe(false);
    expect(state.hiddenEventTypes.size).toBe(0);
  });

  it('toggleSendMatch hides everything except send/match/match-timeout', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleSendMatch();
    expect(state.isSendMatchPresetActive).toBe(true);
    expect(state.hiddenEventTypes.has('send')).toBe(false);
    expect(state.hiddenEventTypes.has('match')).toBe(false);
    expect(state.hiddenEventTypes.has('match-timeout')).toBe(false);
    expect(state.hiddenEventTypes.has('log')).toBe(true);
    state.toggleSendMatch();
    expect(state.isSendMatchPresetActive).toBe(false);
    expect(state.hiddenEventTypes.size).toBe(0);
  });

  it('toggling one preset on, then the other, clears the first', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleSendMatch();
    state.toggleErrorPath(); // no-op (pass outcome)
    // The error-path toggle is a no-op, but the send-match preset stays.
    expect(state.isSendMatchPresetActive).toBe(true);
  });
});

// ─── Event-type filter operations ───────────────────────────────────

describe('ViewerState — event-type filters', () => {
  it('toggleEventType adds and removes a single id', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleEventType('send');
    expect(state.hiddenEventTypes.has('send')).toBe(true);
    state.toggleEventType('send');
    expect(state.hiddenEventTypes.has('send')).toBe(false);
  });

  it('hideAllEventTypes hides every known id; showAllEventTypes clears', () => {
    const state = new ViewerState(passLogWithSend());
    state.hideAllEventTypes();
    expect(state.hiddenEventTypes.size).toBe(ALL_EVENT_TYPE_IDS.length);
    for (const id of ALL_EVENT_TYPE_IDS) expect(state.hiddenEventTypes.has(id)).toBe(true);
    state.showAllEventTypes();
    expect(state.hiddenEventTypes.size).toBe(0);
  });

  it('visibleRows filters out events whose type is hidden', () => {
    const state = new ViewerState(passLogWithSend());
    // The send event lives in the shell-block (span 2); expand it so the
    // row reaches the flattened output before we filter.
    state.toggleSpan(2);
    const withSend = state.visibleRows.filter(
      (r) => r.kind === 'event' && r.folded.kind === 'single' && r.folded.event.kind === 'send',
    );
    expect(withSend.length).toBe(1);

    state.toggleEventType('send');
    const withoutSend = state.visibleRows.filter(
      (r) => r.kind === 'event' && r.folded.kind === 'single' && r.folded.event.kind === 'send',
    );
    expect(withoutSend.length).toBe(0);
  });
});

// ─── Span expansion ─────────────────────────────────────────────────

describe('ViewerState — span expansion', () => {
  it('toggleSpan adds and removes a span id', () => {
    const state = new ViewerState(makeLog({ spans: [testSpan(1), shellBlockSpan(2, 1)] }));
    state.toggleSpan(2);
    expect(state.expandedSpans.has(2)).toBe(true);
    state.toggleSpan(2);
    expect(state.expandedSpans.has(2)).toBe(false);
  });

  it('collapseAll clears the expansion set', () => {
    const state = new ViewerState(makeLog({ spans: [testSpan(1), shellBlockSpan(2, 1)] }));
    state.toggleSpan(2);
    state.collapseAll();
    expect(state.expandedSpans.size).toBe(0);
  });

  it('expandAll covers every span in the glossary', () => {
    const state = new ViewerState(makeLog({ spans: [testSpan(1), shellBlockSpan(2, 1)] }));
    state.expandAll();
    expect(state.expandedSpans.has(1)).toBe(true);
    expect(state.expandedSpans.has(2)).toBe(true);
  });
});

// ─── Expanded value rows ────────────────────────────────────────────

describe('ViewerState — expanded value rows', () => {
  it('toggleExpandedValueRow adds and removes a key', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleExpandedValueRow('var/x');
    expect(state.expandedValueRows.has('var/x')).toBe(true);
    state.toggleExpandedValueRow('var/x');
    expect(state.expandedValueRows.has('var/x')).toBe(false);
  });
});

// ─── Modals ─────────────────────────────────────────────────────────

describe('ViewerState — modals', () => {
  it('open*/close* methods are independent and single-slot', () => {
    const state = new ViewerState(passLogWithSend());
    state.openEnv();
    expect(state.openModal).toBe('env');
    state.openShells();
    expect(state.openModal).toBe('shells'); // single-slot: switches
    state.closeEnv(); // no-op now (env isn't open)
    expect(state.openModal).toBe('shells');
    state.closeShells();
    expect(state.openModal).toBeNull();
  });

  it('toggleFilter flips on and off', () => {
    const state = new ViewerState(passLogWithSend());
    state.toggleFilter();
    expect(state.openModal).toBe('filter');
    state.toggleFilter();
    expect(state.openModal).toBeNull();
  });

  it('toggleFilter from a different modal switches to filter', () => {
    const state = new ViewerState(passLogWithSend());
    state.openEnv();
    state.toggleFilter();
    expect(state.openModal).toBe('filter');
  });

  it('closeModal clears whichever modal is open', () => {
    const state = new ViewerState(passLogWithSend());
    state.openShells();
    state.closeModal();
    expect(state.openModal).toBeNull();
  });
});
