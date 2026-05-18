<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import type { FoldedEvent } from '../lib/flatten';
  import { foldedSeqs, leadEvent } from '../lib/flatten';
  import { foldedFamily, foldedGlyph, foldedKindLabel, foldedSummary, formatTimestamp } from '../lib/format';
  import SelectionCard from './SelectionCard.svelte';

  let {
    state,
    folded,
    depth,
  }: { state: ViewerState; folded: FoldedEvent; depth: number } = $props();

  const lead = $derived(leadEvent(folded));
  const seqs = $derived(foldedSeqs(folded));
  const leadSeq = $derived(seqs[0]!);
  const otherSeq = $derived(seqs.length > 1 ? seqs[1]! : null);
  const extraSeq = $derived(seqs.length > 2 ? seqs[2]! : null);
  const selected = $derived(
    state.selectedEventSeq !== null && seqs.includes(state.selectedEventSeq),
  );
  const summary = $derived(foldedSummary(folded));
  const ts = $derived(formatTimestamp(lead.ts));
  const glyph = $derived(foldedGlyph(folded));
  const family = $derived(foldedFamily(folded));
  const label = $derived(foldedKindLabel(folded));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));
</script>

<li
  class="event-row"
  class:selected
  data-event-seq={leadSeq}
  data-fold-other-seq={otherSeq}
  data-fold-extra-seq={extraSeq}
>
  <div class="row">
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <button class="row-body" type="button" onclick={() => state.selectEvent(leadSeq)}>
      <span class="glyph {family}" aria-hidden="true">{glyph}</span>
      <span class="kind">{label}</span>
      <span class="summary">{summary}</span>
      <span class="ts">{ts}</span>
    </button>
  </div>
  {#if selected}
    <div class="card-slot" style:padding-left="{depth * 24}px">
      <SelectionCard {state} mode={{ kind: 'event', folded }} />
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
    min-height: 24px;
  }
  .row-body {
    display: flex;
    align-items: stretch;
    flex: 1 1 auto;
    min-width: 0;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 0;
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
  }
  .row-body:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .selected .row-body {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    border-radius: var(--radius);
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
