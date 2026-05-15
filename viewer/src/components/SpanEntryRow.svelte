<script lang="ts">
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import { displaySpanCallKind, spanTitle } from '../lib/format';
  import {
    bootstrapForReuse,
    finalCleanupForDeferred,
    firstEventInSpan,
    firstUseShellBlockForMarker,
    shellBlockLifecycle,
    toNumber as n,
  } from '../lib/derive';
  import type { SpanId } from '../lib/derive';
  import StyleBCard from './StyleBCard.svelte';
  import MarkerPill from './MarkerPill.svelte';

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

  type PillProps = {
    marker: string;
    prefix: 'reused' | 'deferred' | null;
    jumpTo: SpanId | null;
  };
  const pillProps = $derived.by<PillProps | null>(() => {
    if (span.kind === 'effect-setup') {
      return {
        marker: span.marker,
        prefix: span.is_reuse ? 'reused' : null,
        jumpTo: span.is_reuse ? bootstrapForReuse(state.data, span.marker) : null,
      };
    }
    if (span.kind === 'effect-cleanup') {
      return {
        marker: span.marker,
        prefix: span.is_deferred ? 'deferred' : null,
        jumpTo: span.is_deferred ? finalCleanupForDeferred(state.data, span.marker) : null,
      };
    }
    if (span.kind === 'shell-block') {
      const firstEv = firstEventInSpan(state.data, id);
      const marker = firstEv?.shell_marker ?? null;
      if (marker === null) return null;
      const lifecycle = shellBlockLifecycle(state.data, id);
      return {
        marker,
        prefix: null,
        jumpTo: lifecycle.firstUse ? null : firstUseShellBlockForMarker(state.data, marker),
      };
    }
    return null;
  });
</script>

<li class="span-row" class:selected data-span-id={id}>
  <div class="row">
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <button
      class="chevron-btn"
      type="button"
      aria-label={expanded ? 'collapse' : 'expand'}
      onclick={() => state.toggleSpan(id)}
    >
      <span class="chevron" aria-hidden="true">
        {expanded ? '\u25BC' : '\u25B6'}
      </span>
    </button>
    <button class="select-btn" type="button" onclick={() => state.selectSpan(id)}>
      <span class="kind">{displaySpanCallKind(span)}</span>
      <span class="title">{title}</span>
    </button>
    {#if pillProps}
      <span class="pill-slot">
        <MarkerPill
          {state}
          marker={pillProps.marker}
          prefix={pillProps.prefix}
          jumpTo={pillProps.jumpTo}
        />
      </span>
    {/if}
  </div>
  {#if selected}
    <div class="card-slot" style:padding-left="{depth * 24}px">
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
    display: flex;
    align-items: stretch;
    width: 100%;
    min-height: 26px;
  }
  .selected > .row {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    border-radius: var(--radius);
  }
  .rail {
    width: 24px;
    height: 26px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
    align-self: stretch;
  }
  .chevron-btn,
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
  }
  .chevron-btn {
    width: 24px;
    flex: 0 0 auto;
    justify-content: center;
  }
  .chevron-btn:hover .chevron {
    color: var(--accent);
  }
  .select-btn {
    flex: 1 1 auto;
    min-width: 0;
  }
  .select-btn:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .chevron {
    color: var(--ink-dim);
    font-family: var(--font-mono);
    text-align: center;
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
    font-family: var(--font-mono);
    font-size: 0.82rem;
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
  .pill-slot {
    display: inline-flex;
    align-items: center;
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
  }
</style>
