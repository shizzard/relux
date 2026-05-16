import { describe, expect, it } from 'vitest';
import type { StructuredLog } from '../types/StructuredLog';
import { ViewerState } from './state.svelte';

// Minimal structurally-valid skip log: one markers root span, one
// marker-eval child, one BoolCheck event.
function makeSkipLog(): StructuredLog {
  return {
    info: { name: 'skipped', path: 'tests/foo.relux', duration_ms: 0n },
    outcome: {
      kind: 'skip',
      span: 2n,
      event_seq: 1n,
      marker_kind: 'skip',
      evaluation: { shape: 'unconditional' },
    },
    env: { bootstrap: [] },
    shells: {},
    spans: {
      '1': {
        id: 1n,
        parent: null,
        kind: 'markers',
        open_ts: 0,
        close_ts: 0,
        location: null,
      },
      '2': {
        id: 2n,
        parent: 1n,
        kind: { 'marker-eval': { marker_kind: 'skip', modifier: 'if', decision: 'mark' } },
        open_ts: 0,
        close_ts: 0,
        location: null,
      },
    },
    events: [
      {
        seq: 1n,
        ts: 0,
        span: 2n,
        shell: null,
        shell_marker: null,
        source: null,
        kind: { 'bool-check': { evaluation: { shape: 'unconditional' } } },
      },
    ],
    buffer_events: [],
    sources: {},
    artifacts: [],
  } as unknown as StructuredLog;
}

describe('ViewerState constructor, skip outcome', () => {
  it('focuses the triggering marker event and expands its ancestors', () => {
    const state = new ViewerState(makeSkipLog());
    expect(state.selectedEventSeq).toBe(1);
    // `expandedSpans` is `Set<SpanId>` and `SpanId` in the viewer is `number`
    // (after `n()` conversion from the wire bigint).
    expect(state.expandedSpans.has(2)).toBe(true);
    expect(state.expandedSpans.has(1)).toBe(true);
  });
});
