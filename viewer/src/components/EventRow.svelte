<script lang="ts">
  import type { Event } from '../types/Event';
  import type { ViewerState } from '../lib/state.svelte';
  import { eventSummary, formatTimestamp, kindGlyph } from '../lib/format';
  import { toNumber as n } from '../lib/derive';
  import EventDetails from './EventDetails.svelte';

  let {
    state,
    event,
    depth,
  }: { state: ViewerState; event: Event; depth: number } = $props();

  const seq = $derived(n(event.seq));
  const selected = $derived(state.selectedEventSeq === seq);
  const summary = $derived(eventSummary(event));
  const ts = $derived(formatTimestamp(event.ts));
  const glyph = $derived(kindGlyph(event.kind));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));
</script>

<li class="event-row" class:selected data-event-seq={seq}>
  <button class="row" type="button" onclick={() => state.selectEvent(seq)}>
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <span class="glyph" aria-hidden="true">{glyph}</span>
    <span class="kind">{event.kind}</span>
    <span class="summary">{summary}</span>
    <span class="ts">{ts}</span>
  </button>
  {#if selected}
    <div class="body">
      <EventDetails {event} />
    </div>
  {/if}
</li>

<style>
  .event-row {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: stretch;
    width: 100%;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    min-height: 24px;
  }
  .row:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .selected > .row {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .rail {
    width: 24px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
  }
  .glyph {
    width: 20px;
    text-align: center;
    color: var(--muted);
    font-family: var(--font-mono);
    flex: 0 0 auto;
    align-self: center;
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.8rem;
    color: var(--muted);
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
    min-width: 9ch;
  }
  .summary {
    font-family: var(--font-mono);
    font-size: 0.85rem;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    align-self: center;
  }
  .ts {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--muted);
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
  }
  .body {
    background: var(--sidebar);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
</style>
