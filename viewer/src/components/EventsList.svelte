<script lang="ts">
  import { onMount } from 'svelte';
  import type { ViewerState } from '../lib/state.svelte';
  import type { Row } from '../lib/flatten';
  import { toNumber as n } from '../lib/derive';
  import { kindFamily } from '../lib/format';
  import EventRow from './EventRow.svelte';
  import SpanEntryRow from './SpanEntryRow.svelte';
  import GapRow from './GapRow.svelte';

  let { state }: { state: ViewerState } = $props();

  const allRows = $derived(state.rows);
  const visibleRows = $derived(filterRows(allRows));

  function filterRows(rows: Row[]): Row[] {
    switch (state.filter) {
      case 'all':
        return rows;
      case 'errors':
        return rows.filter((r) => {
          if (r.kind === 'event') {
            const fam = kindFamily(r.event.kind);
            return fam === 'danger';
          }
          return false;
        });
      case 'send-match':
        return rows.filter((r) => {
          if (r.kind !== 'event') return false;
          const k = r.event.kind;
          return k === 'send' || k === 'match-start' || k === 'match-done' || k === 'timeout';
        });
    }
  }

  function rowKey(row: Row, index: number): string {
    if (row.kind === 'event') return `e:${n(row.event.seq)}`;
    if (row.kind === 'span-entry') return `s:${n(row.span.id)}`;
    return `g:${row.from}:${row.to}:${index}`;
  }

  function rowIndex(): number {
    for (let i = 0; i < visibleRows.length; i++) {
      const r = visibleRows[i]!;
      if (r.kind === 'event' && state.selectedEventSeq !== null && n(r.event.seq) === state.selectedEventSeq) return i;
      if (r.kind === 'span-entry' && state.selectedSpanId !== null && n(r.span.id) === state.selectedSpanId) return i;
    }
    return -1;
  }

  function moveSelection(delta: 1 | -1): void {
    if (visibleRows.length === 0) return;
    const cur = rowIndex();
    let next = cur === -1 ? (delta === 1 ? 0 : visibleRows.length - 1) : cur + delta;
    while (next >= 0 && next < visibleRows.length) {
      const r = visibleRows[next]!;
      if (r.kind === 'event') {
        state.selectedSpanId = null;
        state.selectedEventSeq = n(r.event.seq);
        return;
      }
      if (r.kind === 'span-entry') {
        state.selectedEventSeq = null;
        state.selectedSpanId = n(r.span.id);
        return;
      }
      next += delta;
    }
  }

  function toggleCurrent(): void {
    if (state.selectedEventSeq !== null) {
      state.selectedEventSeq = null;
    } else if (state.selectedSpanId !== null) {
      state.toggleSpanFull(state.selectedSpanId);
    }
  }

  function handleKey(event: KeyboardEvent): void {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveSelection(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveSelection(-1);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      toggleCurrent();
    } else if (event.key === 'ArrowRight' && state.selectedSpanId !== null) {
      if (!state.expandedSpans.has(state.selectedSpanId)) {
        event.preventDefault();
        state.toggleSpan(state.selectedSpanId);
      }
    } else if (event.key === 'ArrowLeft' && state.selectedSpanId !== null) {
      if (state.expandedSpans.has(state.selectedSpanId)) {
        event.preventDefault();
        state.toggleSpan(state.selectedSpanId);
      }
    }
  }

  let listEl: HTMLElement | undefined;
  onMount(() => {
    if (!listEl) return;
    const seq = state.selectedEventSeq;
    if (seq === null) return;
    const target = listEl.querySelector<HTMLElement>(`[data-event-seq="${seq}"]`);
    target?.scrollIntoView({ block: 'center', behavior: 'auto' });
  });
</script>

<section class="events" aria-label="Events" tabindex="0" onkeydown={handleKey} bind:this={listEl}>
  <ol class="rows">
    {#each visibleRows as row, i (rowKey(row, i))}
      {#if row.kind === 'span-entry'}
        <SpanEntryRow {state} span={row.span} depth={row.depth} />
      {:else if row.kind === 'event'}
        <EventRow {state} event={row.event} depth={row.depth} />
      {:else}
        <GapRow ms={row.ms} />
      {/if}
    {/each}
  </ol>
  <footer class="chips">
    <button class="chip" class:active={state.filter === 'all'} onclick={() => (state.filter = 'all')}>filter</button>
    <button class="chip warn" class:active={state.filter === 'errors'} onclick={() => (state.filter = state.filter === 'errors' ? 'all' : 'errors')}>errors only</button>
    <button class="chip" class:active={state.filter === 'send-match'} onclick={() => (state.filter = state.filter === 'send-match' ? 'all' : 'send-match')}>send / match</button>
  </footer>
</section>

<style>
  .events {
    background: var(--paper);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .events:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .rows {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .chips {
    display: flex;
    gap: var(--gap-xs);
    padding: var(--gap-xs) var(--gap-sm);
    border-top: 1px dashed var(--border);
    flex: 0 0 auto;
  }
  .chip {
    appearance: none;
    background: transparent;
    border: 1px solid var(--ink-faint);
    color: var(--ink-dim);
    font: inherit;
    font-size: 0.72rem;
    border-radius: 100px;
    padding: 1px 8px;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--ink);
    border-color: var(--ink-dim);
  }
  .chip.warn {
    color: var(--accent);
    border-color: var(--accent);
  }
  .chip.active {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
