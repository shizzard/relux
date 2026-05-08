<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { formatDuration } from '../lib/format';
  import { outcomeClass } from '../lib/theme';

  let { state }: { state: ViewerState } = $props();

  const test = $derived(state.data.test);
  const cls = $derived(outcomeClass(test.outcome));
  const duration = $derived(formatDuration(Number(test.duration_ms)));
</script>

<header class="viewer-header">
  <div class="title">
    <h1>{test.name}</h1>
    <span class="pill {cls}">{test.outcome}</span>
    <span class="duration">{duration}</span>
  </div>
  <div class="meta">
    <code class="path">{test.path}</code>
    <button class="env-link" onclick={() => state.openEnv()}>environment &#x2197;</button>
  </div>
</header>

<style>
  .viewer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--gap-md);
    padding: var(--gap-md) var(--gap-lg);
    border-bottom: 1px solid var(--border);
    background: var(--sidebar);
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: var(--gap-md);
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pill {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px 8px;
    border-radius: var(--radius);
    border: 1px solid currentColor;
  }
  .pill.pass {
    color: var(--pass);
  }
  .pill.fail {
    color: var(--fail);
  }
  .pill.skip {
    color: var(--skip);
  }
  .pill.invalid {
    color: var(--invalid);
  }
  .duration {
    font-family: var(--font-mono);
    font-size: 0.85rem;
    color: var(--muted);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--gap-md);
  }
  .path {
    font-size: 0.85rem;
    color: var(--muted);
  }
  .env-link {
    border: 1px solid var(--border);
    background: transparent;
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: 0.85rem;
    color: var(--accent);
    cursor: pointer;
  }
  .env-link:hover {
    border-color: var(--accent);
  }
</style>
