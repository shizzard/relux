<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  let { state }: { state: ViewerState } = $props();
</script>

{#if state.data.failure}
  <button class="jtf" disabled aria-label="Jump to failure">&#x2193; failure</button>
{/if}

<style>
  .jtf {
    position: fixed;
    right: var(--gap-lg);
    bottom: var(--gap-lg);
    padding: var(--gap-sm) var(--gap-md);
    background: var(--bg);
    border: 1px solid var(--fail);
    border-radius: var(--radius);
    color: var(--fail);
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 2px 8px color-mix(in srgb, var(--fg) 15%, transparent);
  }
  .jtf[disabled] {
    cursor: default;
    opacity: 0.7;
  }
</style>
