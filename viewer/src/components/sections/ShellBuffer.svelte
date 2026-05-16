<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';
  import SearchableBuffer from './SearchableBuffer.svelte';
  import { formatDuration, formatTimeout } from '../../lib/format';

  let { state }: { state: ViewerState } = $props();

  const activeMarker = $derived(state.bufferKey);
  const activeShellName = $derived(
    activeMarker !== null
      ? ((state.data.shells as unknown as Record<string, { name: string } | undefined>)[activeMarker]?.name ?? null)
      : null,
  );
  const regions = $derived(
    activeMarker !== null ? (state.bufferRegionsAt.get(activeMarker) ?? null) : null,
  );
  const ctx = $derived(state.shellContext);

  const title = $derived(
    activeShellName !== null ? `shell \u00b7 ${activeShellName}` : 'shell',
  );
  const hint = $derived(buildHint());

  function buildHint(): string {
    if (state.selected) {
      const parts: string[] = [`@ t = ${formatDuration(state.selected.ts)}`];
      if (regions?.matched) parts.push('matched \u2713');
      else if (regions === null || activeMarker === null) parts.push('no shell');
      else parts.push('idle');
      if (ctx?.timeout !== null && ctx?.timeout !== undefined) {
        parts.push(`timeout ${formatTimeout(ctx.timeout)}`);
      }
      if (ctx && ctx.failPatterns.length > 0) {
        parts.push(`${ctx.failPatterns.length} fail-patterns armed`);
      }
      return parts.join(' \u00b7 ');
    }
    if (state.selectedSpan) {
      const span = state.selectedSpan;
      if (
        span.kind === 'shell-block' ||
        span.kind === 'cleanup-block' ||
        span.kind === 'fn-call'
      ) {
        return `@ end of ${span.kind}`;
      }
      return 'no shell context';
    }
    return 'no event selected';
  }
</script>

<Panel {title} {hint}>
  {#if activeMarker === null}
    <p class="empty">this event has no shell context.</p>
  {:else}
    <div class="wrap">
      <SearchableBuffer {regions} />
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
