<script lang="ts">
  import type { StructuredLog } from './types/StructuredLog';

  let { data }: { data: StructuredLog | null } = $props();
</script>

<header>
  <h1>Relux Test Report</h1>
  <p class="hint">Preview build &mdash; full timeline UI lands in a later commit.</p>
</header>

<main>
  {#if data}
    <section class="summary">
      <p>
        <strong>{data.test.name}</strong>
        &mdash; {data.test.outcome} in {data.test.duration_ms} ms
      </p>
      <p class="counts">
        Spans: {Object.keys(data.spans).length}
        &middot; Events: {data.events.length}
        &middot; Buffer events: {data.buffer_events.length}
        &middot; Shells: {Object.keys(data.shells).length}
      </p>
    </section>
    <details>
      <summary>Raw JSON</summary>
      <pre>{JSON.stringify(data, null, 2)}</pre>
    </details>
  {:else}
    <p class="empty">
      No data loaded. Set <code>window.RELUX_DATA</code> before this script runs.
    </p>
  {/if}
</main>

<style>
  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    margin: 2rem;
    line-height: 1.5;
    color: #222;
  }
  header h1 {
    margin: 0 0 0.25rem;
    font-size: 1.5rem;
  }
  .hint {
    margin: 0 0 1.5rem;
    color: #888;
    font-size: 0.9em;
  }
  .summary {
    margin-bottom: 1rem;
  }
  .counts {
    color: #555;
    font-size: 0.9em;
  }
  .empty {
    color: #888;
  }
  pre {
    overflow-x: auto;
    background: #f5f5f5;
    padding: 1rem;
    border-radius: 4px;
    font-size: 0.85em;
  }
</style>
