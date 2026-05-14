<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import type { SpanId } from '../lib/derive';

  type Props = {
    state: ViewerState;
    marker: string;
    prefix?: 'reused' | 'deferred' | null;
    jumpTo?: SpanId | null;
  };
  let { state, marker, prefix = null, jumpTo = null }: Props = $props();

  function onClick(e: MouseEvent) {
    if (jumpTo === null) return;
    e.stopPropagation();
    state.revealAndSelect(jumpTo);
  }
</script>

<button
  type="button"
  class="pill"
  class:clickable={jumpTo !== null}
  disabled={jumpTo === null}
  onclick={onClick}
  title={jumpTo !== null ? 'Jump to source' : marker}
>
  {#if prefix !== null}
    <span class="prefix">{prefix}</span>
  {/if}
  <span class="marker">{marker}</span>
</button>

<style>
  .pill {
    display: inline-flex;
    gap: var(--gap-xs);
    align-items: center;
    padding: 0 var(--gap-sm);
    height: 18px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--ink) 8%, transparent);
    color: var(--ink-dim);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    line-height: 1;
    cursor: default;
    appearance: none;
  }
  .pill[disabled] {
    /* decorative — keep neutral look, no hover affordance */
    pointer-events: none;
  }
  .pill.clickable {
    cursor: pointer;
  }
  .pill.clickable:hover {
    border-color: var(--accent);
    color: var(--ink);
  }
  .prefix {
    color: var(--ink);
    font-weight: 600;
    text-transform: lowercase;
  }
  .marker {
    color: inherit;
  }
</style>
