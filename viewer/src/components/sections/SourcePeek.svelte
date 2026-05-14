<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';

  let { state }: { state: ViewerState } = $props();

  const location = $derived(state.callStack.at(-1)?.location ?? null);
  const hint = $derived(location ? `${location.file}:${location.line}` : 'no location');
</script>

<Panel title="source" {hint}>
  <div class="content">
    {#if location !== null}
      <p class="placeholder">
        source bytes are not shipped in this report (deferred).
      </p>
      <p class="loc"><code>{location.file}:{location.line}</code></p>
    {:else}
      <p class="placeholder">no location available for this event.</p>
    {/if}
  </div>
</Panel>

<style>
  .content {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding: var(--gap-sm) var(--gap-md);
  }
  .placeholder {
    margin: 0 0 var(--gap-sm);
    color: var(--ink-faint);
    font-style: italic;
    font-size: 0.85rem;
  }
  .loc {
    margin: 0;
    font-size: 0.85rem;
  }
  code {
    font-family: var(--font-mono);
    background: var(--paper-2);
    color: var(--ink);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.8rem;
  }
</style>
