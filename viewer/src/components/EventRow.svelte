<script lang="ts">
  import type { Event } from '../types/Event';
  import type { ViewerState } from '../lib/state.svelte';
  import { eventSummary, formatTimestamp, kindFamily, kindGlyph } from '../lib/format';
  import { toNumber as n } from '../lib/derive';
  import StyleBCard from './StyleBCard.svelte';

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
  const family = $derived(kindFamily(event.kind));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));
</script>

<li class="event-row" class:selected data-event-seq={seq}>
  <button class="row" type="button" onclick={() => state.selectEvent(seq)}>
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <span class="glyph {family}" aria-hidden="true">{glyph}</span>
    <span class="kind">{event.kind}</span>
    <span class="summary">{summary}</span>
    <span class="ts">{ts}</span>
  </button>
  {#if selected}
    <div class="card-slot" style:padding-left="{(depth + 1) * 24}px">
      <StyleBCard {state} mode={{ kind: 'event', event }} />
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
    outline: 1px dashed var(--accent);
    outline-offset: -1px;
  }
  .rail {
    width: 24px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
  }
  .glyph {
    width: 20px;
    text-align: center;
    color: var(--ink-faint);
    font-family: var(--font-mono);
    flex: 0 0 auto;
    align-self: center;
  }
  .glyph.ok {
    color: var(--accent-2);
  }
  .glyph.danger {
    color: var(--danger);
  }
  .glyph.info {
    color: var(--ink-dim);
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--ink-dim);
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
    min-width: 9ch;
  }
  .summary {
    font-family: var(--font-mono);
    font-size: 0.82rem;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    align-self: center;
    color: var(--ink);
  }
  .ts {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--ink-faint);
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
  }
  .card-slot {
    padding-right: var(--gap-md);
  }
</style>
