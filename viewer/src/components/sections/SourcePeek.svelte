<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  let { state: _state }: { state: ViewerState } = $props();
</script>

<section class="section">
  <h3>Source</h3>
  <p class="empty">&lt;empty&gt;</p>
</section>

<style>
  .section {
    padding: var(--gap-md) 0;
    border-bottom: 1px solid var(--border);
  }
  .section:last-child {
    border-bottom: none;
  }
  h3 {
    margin: 0 0 var(--gap-sm);
    font-size: 0.85em;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-style: italic;
    font-size: 0.9em;
  }
</style>
