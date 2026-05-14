<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';
  import BufferRegions from './BufferRegions.svelte';
  import { formatDuration } from '../../lib/format';

  let { state }: { state: ViewerState } = $props();

  const activeShell = $derived(state.selected?.shell ?? null);
  const regions = $derived(
    activeShell !== null ? (state.bufferRegionsAt.get(activeShell) ?? null) : null,
  );
  const ctx = $derived(state.shellContext);

  const title = $derived(
    activeShell !== null ? `shell \u00b7 ${activeShell}` : 'shell',
  );
  const hint = $derived(buildHint());

  function buildHint(): string {
    if (!state.selected) return 'no event selected';
    const parts: string[] = [`@ t = ${formatDuration(state.selected.ts)}`];
    if (regions?.matched) parts.push('matched \u2713');
    else if (regions === null || activeShell === null) parts.push('no shell');
    else parts.push('idle');
    if (ctx?.timeout !== null && ctx?.timeout !== undefined) {
      parts.push(`timeout ${ctx.timeout}`);
    }
    if (ctx && ctx.failPatterns.length > 0) {
      parts.push(`${ctx.failPatterns.length} fail-patterns armed`);
    }
    return parts.join(' \u00b7 ');
  }
</script>

<Panel {title} {hint}>
  {#if activeShell === null}
    <p class="empty">this event has no shell context.</p>
  {:else}
    <div class="wrap">
      <BufferRegions {regions} />
    </div>
  {/if}
</Panel>

<style>
  .wrap {
    flex: 1 1 0;
    min-height: 0;
    min-width: 0;
    padding: var(--gap-sm) var(--gap-md);
    overflow: hidden;
    display: flex;
    align-items: stretch;
  }
  .empty {
    margin: 0;
    padding: var(--gap-md);
    color: var(--ink-faint);
    font-style: italic;
    font-size: 0.85rem;
  }
</style>
