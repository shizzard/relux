<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  let { state }: { state: ViewerState } = $props();
</script>

<section class="events" aria-label="Events">
  <p class="empty">&lt;empty&gt; ({state.rows.length} rows pending render)</p>
</section>

<style>
  .events {
    padding: var(--gap-md) var(--gap-lg);
    border-right: 1px solid var(--border);
    overflow-y: auto;
    min-height: 50vh;
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-style: italic;
  }
</style>
