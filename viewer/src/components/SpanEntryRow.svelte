<script lang="ts">
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import { spanTitle } from '../lib/format';
  import { toNumber as n } from '../lib/derive';
  import StyleBCard from './StyleBCard.svelte';

  let {
    state,
    span,
    depth,
  }: { state: ViewerState; span: Span; depth: number } = $props();

  const id = $derived(n(span.id));
  const expanded = $derived(state.expandedSpans.has(id));
  const selected = $derived(state.selectedSpanId === id);
  const title = $derived(spanTitle(span));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));
</script>

<li class="span-row" class:selected data-span-id={id}>
  <button class="row" type="button" onclick={() => state.toggleSpanFull(id)}>
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <span class="chevron" aria-hidden="true">
      {expanded ? '\u25BE' : '\u25B8'}
    </span>
    <span class="kind">{span.kind}</span>
    <span class="title">{title}</span>
  </button>
  {#if selected}
    <div class="card-slot" style:padding-left="{(depth + 1) * 24 + 20}px">
      <StyleBCard {state} mode={{ kind: 'span', span }} />
    </div>
  {/if}
</li>

<style>
  .span-row {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    appearance: none;
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    text-align: left;
    display: flex;
    align-items: center;
    width: 100%;
    min-height: 26px;
    cursor: pointer;
    padding: 0;
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
    height: 26px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
    align-self: stretch;
  }
  .chevron {
    width: 20px;
    color: var(--ink-dim);
    font-family: var(--font-mono);
    flex: 0 0 auto;
    text-align: center;
  }
  .row:hover .chevron {
    color: var(--accent);
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--ink-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
    min-width: 9ch;
  }
  .title {
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--ink);
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .card-slot {
    padding-right: var(--gap-md);
  }
</style>
