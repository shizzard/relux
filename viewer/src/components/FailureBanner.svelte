<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  let { state }: { state: ViewerState } = $props();

  const failure = $derived(state.data.failure);
</script>

{#if failure}
  <div class="failure-banner" role="alert">
    <span class="reason">&lt;empty&gt;</span>
    <span class="sep">&middot;</span>
    <span class="shell">&lt;empty&gt;</span>
    <span class="sep">&middot;</span>
    <button class="jump" disabled>&lt;empty&gt;</button>
  </div>
{/if}

<style>
  .failure-banner {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: var(--gap-sm) var(--gap-lg);
    background: color-mix(in srgb, var(--fail) 12%, transparent);
    border-bottom: 1px solid var(--fail);
    color: var(--fg);
    font-size: 0.9rem;
  }
  .reason {
    color: var(--fail);
    font-weight: 600;
  }
  .sep {
    color: var(--muted);
  }
  .jump {
    border: none;
    background: transparent;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font-size: inherit;
  }
  .jump[disabled] {
    color: var(--muted);
    cursor: default;
  }
</style>
