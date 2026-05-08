<script lang="ts">
  import { onMount } from 'svelte';
  import type { ViewerState } from '../lib/state.svelte';
  import type { Row } from '../lib/flatten';
  import { toNumber as n } from '../lib/derive';
  import EventRow from './EventRow.svelte';
  import SpanEntryRow from './SpanEntryRow.svelte';
  import GapRow from './GapRow.svelte';

  let { state }: { state: ViewerState } = $props();

  function rowKey(row: Row, index: number): string {
    if (row.kind === 'event') return `e:${n(row.event.seq)}`;
    if (row.kind === 'span-entry') return `s:${n(row.span.id)}`;
    return `g:${row.from}:${row.to}:${index}`;
  }

  let listEl: HTMLOListElement | undefined;
  onMount(() => {
    if (!listEl) return;
    const seq = state.selectedEventSeq;
    if (seq === null) return;
    const target = listEl.querySelector<HTMLElement>(`[data-event-seq="${seq}"]`);
    target?.scrollIntoView({ block: 'center', behavior: 'auto' });
  });
</script>

<section class="events" aria-label="Events">
  <ol class="rows" bind:this={listEl}>
    {#each state.rows as row, i (rowKey(row, i))}
      {#if row.kind === 'span-entry'}
        <SpanEntryRow {state} span={row.span} depth={row.depth} />
      {:else if row.kind === 'event'}
        <EventRow {state} event={row.event} depth={row.depth} />
      {:else}
        <GapRow ms={row.ms} />
      {/if}
    {/each}
  </ol>
</section>

<style>
  .events {
    border-right: 1px solid var(--border);
    overflow-y: auto;
    min-height: 50vh;
    max-height: calc(100vh - 60px);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
</style>
