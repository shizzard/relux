<script lang="ts">
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import { toNumber as n } from '../lib/derive';
  import { escapeBytes, formatTimestamp } from '../lib/format';
  import StyleBCard from './StyleBCard.svelte';

  let {
    state,
    span,
    depth,
  }: { state: ViewerState; span: Span; depth: number } = $props();

  const id = $derived(n(span.id));
  const selected = $derived(state.selectedSpanId === id);
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));

  const title = $derived.by(() => {
    if (span.kind !== 'fn-call') return '';
    const head = `${span.name}/${span.args.length}`;
    if (span.result === null) return head;
    return `${head} \u{2192} "${escapeBytes(span.result)}"`;
  });

  const ts = $derived(formatTimestamp(span.start_ts));
</script>

<li class="bif-row" class:selected data-span-id={id}>
  <div class="row">
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <button class="select-btn" type="button" onclick={() => state.selectSpan(id)}>
      <span class="glyph" aria-hidden="true">&#x192;</span>
      <span class="title">{title}</span>
      <span class="ts">{ts}</span>
    </button>
  </div>
  {#if selected}
    <div class="card-slot" style:padding-left="{depth * 24}px">
      <StyleBCard {state} mode={{ kind: 'span', span }} />
    </div>
  {/if}
</li>

<style>
  .bif-row {
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
  .selected > .row {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    border-radius: var(--radius);
  }
  .rail {
    width: 24px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
  }
  .select-btn {
    appearance: none;
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    padding: 0;
    display: flex;
    align-items: center;
    flex: 1 1 auto;
    min-width: 0;
  }
  .select-btn:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .glyph {
    width: 20px;
    text-align: center;
    color: var(--ink-faint);
    font-family: var(--font-mono);
    flex: 0 0 auto;
    align-self: center;
  }
  .title {
    font-family: var(--font-mono);
    font-size: 0.82rem;
    color: var(--ink);
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 var(--gap-sm);
    align-self: center;
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
