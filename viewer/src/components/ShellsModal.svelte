<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { formatBytes, formatDuration } from '../lib/format';
  import { toNumber as n } from '../lib/derive';
  import Modal from './Modal.svelte';
  import BufferRegions from './sections/BufferRegions.svelte';
  import ValueCell from './ValueCell.svelte';

  let { state }: { state: ViewerState } = $props();

  const shells = $derived(buildShells());
  const ev = $derived(state.selected);
  const test = $derived(state.data.test);
  const shellCount = $derived(shells.length);

  function buildShells(): Array<{
    marker: string;
    name: string;
    command: string;
    spawn_ts: number;
    terminate_ts: number | null;
  }> {
    const recs = state.data.shells as unknown as Record<
      string,
      | {
          marker: string;
          name: string;
          command: string;
          spawn_ts: number;
          terminate_ts: number | null;
        }
      | undefined
    >;
    const out: Array<{
      marker: string;
      name: string;
      command: string;
      spawn_ts: number;
      terminate_ts: number | null;
    }> = [];
    for (const marker of Object.keys(recs)) {
      const r = recs[marker];
      if (!r) continue;
      out.push({
        marker,
        name: r.name,
        command: r.command,
        spawn_ts: r.spawn_ts,
        terminate_ts: r.terminate_ts,
      });
    }
    return out.sort((a, b) => a.spawn_ts - b.spawn_ts);
  }

  function shellStateLabel(marker: string): { label: string; cls: string } {
    const live = state.liveShells.find((s) => s.marker === marker);
    if (!live) return { label: 'unknown', cls: 'dead' };
    switch (live.state) {
      case 'ready':
        return { label: 'ready', cls: 'ok' };
      case 'busy':
        return { label: 'awaiting input', cls: 'busy' };
      case 'ended':
        return { label: 'ended', cls: 'dead' };
      case 'error':
        return { label: 'error', cls: 'err' };
    }
  }

  function bufferEventCount(marker: string): number {
    if (!ev) return 0;
    const seq = n(ev.seq);
    let count = 0;
    for (const be of state.data.buffer_events) {
      if (n(be.seq) > seq) break;
      if (be.shell_marker === marker) count++;
    }
    return count;
  }

  function bufferSize(marker: string): number {
    const r = state.bufferRegionsAt.get(marker);
    if (!r) return 0;
    return (
      r.consumed.length + (r.matched?.bytes.length ?? 0) + r.tail.length
    );
  }

  function endedBefore(terminate_ts: number | null): boolean {
    if (terminate_ts === null) return false;
    return ev !== null && terminate_ts <= ev.ts;
  }

  const subtitle = $derived(buildSubtitle());

  function buildSubtitle(): string {
    if (!ev) return `\u2014 \u00b7 in ${test.name}`;
    return `@ event #${n(ev.seq)} \u00b7 ${ev.kind} \u00b7 t = ${formatDuration(ev.ts)} \u00b7 in ${test.name}`;
  }
</script>

{#if state.openModal === 'shells'}
  <Modal title="all shells" {subtitle} onClose={() => state.closeShells()}>
    {#snippet actions()}
      <span class="caption">{shellCount} shell{shellCount === 1 ? '' : 's'}</span>
    {/snippet}

    <div class="cards">
      {#each shells as sh (sh.marker)}
        {@const isCurrent = ev?.shell_marker === sh.marker}
        {@const isEnded = endedBefore(sh.terminate_ts)}
        {@const stateInfo = shellStateLabel(sh.marker)}
        {@const regions = state.bufferRegionsAt.get(sh.marker) ?? null}
        <div class="sh-card" class:current={isCurrent} class:ended={isEnded}>
          <div class="meta-col">
            <div class="sh-name">
              {sh.name}
              {#if isCurrent}<span class="badge">&#x2605; this event</span>{/if}
            </div>
            <div class="sh-cmd">
              <ValueCell value={sh.command} {state} expandKey={`sh:${sh.marker}:cmd`} />
            </div>
            <div class="sh-state {stateInfo.cls}">
              <span class="dot"></span>
              <span class="state-label">{stateInfo.label}</span>
            </div>
            <div class="filler"></div>
            <div class="sh-row"><span>spawned</span><b>{formatDuration(sh.spawn_ts)}</b></div>
            {#if isEnded && sh.terminate_ts !== null}
              <div class="sh-row"><span>ended</span><b>{formatDuration(sh.terminate_ts)}</b></div>
            {/if}
            <div class="sh-row"><span>buffer size</span><b>{formatBytes(bufferSize(sh.marker))}</b></div>
            <div class="sh-row"><span>buffer events</span><b>{bufferEventCount(sh.marker)}</b></div>
          </div>
          <div class="buf-col">
            <BufferRegions {regions} />
            {#if isEnded && sh.terminate_ts !== null}
              <p class="ended-note">&mdash; shell ended at t = {formatDuration(sh.terminate_ts)} (before this event) &mdash;</p>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </Modal>
{/if}

<style>
  .caption {
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink-faint);
  }
  .cards {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding: var(--gap-md) var(--gap-lg);
    display: flex;
    flex-direction: column;
    gap: var(--gap-md);
  }
  .sh-card {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    gap: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--paper-2);
    overflow: hidden;
    min-height: 168px;
  }
  .sh-card.current {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .sh-card.ended {
    opacity: 0.62;
  }
  .meta-col {
    padding: var(--gap-sm) var(--gap-md);
    border-right: 1px dashed var(--border);
    background: rgba(0, 0, 0, 0.18);
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .sh-name {
    font-family: var(--font-mono);
    font-size: 1.05rem;
    font-weight: 600;
    color: var(--ink);
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
  }
  .sh-card.current .sh-name {
    color: var(--accent);
  }
  .badge {
    font-size: 0.65rem;
    color: var(--accent);
    border: 1px solid var(--accent);
    padding: 1px 6px;
    border-radius: 100px;
  }
  .sh-cmd {
    font-size: 0.76rem;
    color: var(--ink-dim);
    min-width: 0;
  }
  .sh-state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: 0.78rem;
  }
  .sh-state .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: 0 0 auto;
  }
  .sh-state.ok {
    color: var(--accent-2);
  }
  .sh-state.ok .dot {
    background: var(--accent-2);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-2) 18%, transparent);
  }
  .sh-state.busy {
    color: var(--accent);
  }
  .sh-state.busy .dot {
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .sh-state.dead {
    color: var(--ink-faint);
  }
  .sh-state.dead .dot {
    background: var(--ink-faint);
  }
  .sh-state.err {
    color: var(--danger);
  }
  .sh-state.err .dot {
    background: var(--danger);
  }
  .filler {
    flex: 1 1 auto;
  }
  .sh-row {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--ink-faint);
    display: flex;
    justify-content: space-between;
    gap: var(--gap-sm);
  }
  .sh-row b {
    color: var(--ink-dim);
    font-weight: 500;
  }
  .buf-col {
    position: relative;
    min-width: 0;
    overflow: hidden;
    padding: var(--gap-xs) var(--gap-sm);
    display: flex;
    flex-direction: column;
    align-items: stretch;
  }
  .ended-note {
    margin: 4px 0 0;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--ink-faint);
    font-style: italic;
  }
</style>
