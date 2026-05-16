import { describe, expect, it } from 'vitest';
import type { ArtifactEntry } from '../types/ArtifactEntry';
import type { StructuredLog } from '../types/StructuredLog';
import { ViewerState } from './state.svelte';

function makeLog(artifacts: ArtifactEntry[]): StructuredLog {
  return {
    info: { name: 't', path: 'p', duration_ms: 0n },
    outcome: { kind: 'pass' },
    env: { bootstrap: [] },
    shells: {},
    spans: {},
    events: [],
    buffer_events: [],
    sources: {},
    artifacts,
  } as unknown as StructuredLog;
}

describe('ViewerState — artifacts modal', () => {
  it('opens, closes, and is independent of other modals', () => {
    const state = new ViewerState(
      makeLog([{ path: 'out.txt', size: 12n, mime: 'text/plain' }]),
    );
    expect(state.openModal).toBeNull();
    state.openArtifacts();
    expect(state.openModal).toBe('artifacts');
    state.closeArtifacts();
    expect(state.openModal).toBeNull();
  });

  it('closeArtifacts is a no-op when a different modal is open', () => {
    const state = new ViewerState(makeLog([]));
    state.openEnv();
    state.closeArtifacts();
    expect(state.openModal).toBe('env');
  });
});
