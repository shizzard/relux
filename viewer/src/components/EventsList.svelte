<script lang="ts">
  import { onMount } from 'svelte';
  import type { ViewerState } from '../lib/state.svelte';
  import type { Row } from '../lib/flatten';
  import { foldedSeqs, leadEvent } from '../lib/flatten';
  import { toNumber as n } from '../lib/derive';
  import BifRow from './BifRow.svelte';
  import EventRow from './EventRow.svelte';
  import SpanEntryRow from './SpanEntryRow.svelte';
  import GapRow from './GapRow.svelte';
  import LogBar from './LogBar.svelte';
  import Chip from './Chip.svelte';
  import FilterPopup from './FilterPopup.svelte';

  let { state }: { state: ViewerState } = $props();

  const visibleRows = $derived(state.visibleRows);

  function rowKey(row: Row, index: number): string {
    if (row.kind === 'event') return `e:${n(leadEvent(row.folded).seq)}`;
    if (row.kind === 'span-entry') return `s:${n(row.span.id)}`;
    if (row.kind === 'log-bar') return `l:${n(row.event.seq)}`;
    if (row.kind === 'bif-row') return `b:${n(row.span.id)}`;
    return `g:${row.from}:${row.to}:${index}`;
  }

  function rowIndex(): number {
    for (let i = 0; i < visibleRows.length; i++) {
      const r = visibleRows[i]!;
      if (
        r.kind === 'event' &&
        state.selectedEventSeq !== null &&
        foldedSeqs(r.folded).includes(state.selectedEventSeq)
      )
        return i;
      if (r.kind === 'span-entry' && state.selectedSpanId !== null && n(r.span.id) === state.selectedSpanId) return i;
      if (r.kind === 'bif-row' && state.selectedSpanId !== null && n(r.span.id) === state.selectedSpanId) return i;
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
        state.selectedEventSeq = n(leadEvent(r.folded).seq);
        return;
      }
      if (r.kind === 'span-entry') {
        state.selectedEventSeq = null;
        state.selectedSpanId = n(r.span.id);
        return;
      }
      if (r.kind === 'bif-row') {
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
      state.selectSpan(state.selectedSpanId);
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

  let rowsEl: HTMLOListElement | undefined;
  onMount(() => {
    if (!rowsEl) return;
    const seq = state.selectedEventSeq;
    if (seq === null) return;
    // The lead seq is the canonical data-event-seq; fold halves carry the
    // other halves' seqs so the failure event still scrolls into view even
    // when it's the closing half (timeout, match-done) of a fold.
    const target = rowsEl.querySelector<HTMLElement>(
      `[data-event-seq="${seq}"], [data-fold-other-seq="${seq}"], [data-fold-extra-seq="${seq}"]`,
    );
    target?.scrollIntoView({ block: 'center', behavior: 'auto' });
  });
</script>

<section class="events" aria-label="Events">
  <ol
    class="rows"
    role="tree"
    tabindex="0"
    onkeydown={handleKey}
    bind:this={rowsEl}
  >
    {#each visibleRows as row, i (rowKey(row, i))}
      {#if row.kind === 'span-entry'}
        <SpanEntryRow {state} span={row.span} depth={row.depth} />
      {:else if row.kind === 'event'}
        <EventRow {state} folded={row.folded} depth={row.depth} />
      {:else if row.kind === 'log-bar'}
        <LogBar level={row.level} event={row.event} depth={row.depth} />
      {:else if row.kind === 'bif-row'}
        <BifRow {state} span={row.span} depth={row.depth} />
      {:else}
        <GapRow ms={row.ms} />
      {/if}
    {/each}
  </ol>
  <footer class="chips">
    <span class="chip-anchor">
      <Chip
        kbd="F"
        active={state.hiddenEventTypes.size > 0}
        onclick={() => state.toggleFilter()}
        title="filter events (F)"
      >filter</Chip>
      {#if state.openModal === 'filter'}
        <FilterPopup {state} />
      {/if}
    </span>
    <Chip
      kbd="T"
      active={state.isErrorPathPresetActive}
      disabled={!state.hasErrorPath}
      onclick={() => state.toggleErrorPath()}
      title={state.hasErrorPath ? 'error path (T)' : 'no errors in this run'}
    >error path</Chip>
    <Chip
      kbd="M"
      active={state.isSendMatchPresetActive}
      onclick={() => state.toggleSendMatch()}
      title="send / match only (M)"
    >send / match only</Chip>
    <span class="spacer"></span>
    <Chip
      kbd="C"
      onclick={() => state.collapseAll()}
      title="collapse all spans (C)"
    >collapse all</Chip>
    <Chip
      kbd="X"
      onclick={() => state.expandAll()}
      title="expand all spans (X)"
    >expand all</Chip>
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
    overflow: visible;
  }
  .rows {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .chips {
    display: flex;
    gap: var(--gap-xs);
    padding: var(--gap-xs) var(--gap-sm);
    border-top: 1px dashed var(--border);
    flex: 0 0 auto;
  }
  .chip-anchor {
    position: relative;
    display: inline-flex;
  }
  .spacer {
    flex: 1 1 auto;
  }
</style>
