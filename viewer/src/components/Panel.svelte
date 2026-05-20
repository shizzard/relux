<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    hint = null,
    actions,
    children,
  }: {
    title: string;
    hint?: string | null;
    actions?: Snippet;
    children: Snippet;
  } = $props();
</script>

<section class="panel">
  <header class="panel-header">
    <span class="title">{title}</span>
    {#if hint !== null}
      <span class="hint">{hint}</span>
    {/if}
    {#if actions}
      <span class="actions">{@render actions()}</span>
    {/if}
  </header>
  <div class="body">
    {@render children()}
  </div>
</section>

<style>
  .panel {
    background: var(--paper);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .panel-header {
    display: flex;
    align-items: baseline;
    gap: var(--gap-sm);
    padding: var(--gap-sm) var(--gap-md);
    border-bottom: 1px dashed var(--border);
    flex: 0 0 auto;
  }
  .title {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--ink-dim);
  }
  .hint {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--ink-faint);
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    margin-left: auto;
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
    flex: 0 0 auto;
  }
  .body {
    flex: 1 1 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
