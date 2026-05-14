<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { copy } from '../lib/clipboard';
  import { overflowProbe } from '../lib/actions';
  import { escapeBytes } from '../lib/format';

  let {
    value,
    state: viewer,
    expandKey,
    accent = false,
  }: {
    value: string;
    state: ViewerState;
    expandKey: string;
    accent?: boolean;
  } = $props();

  const expanded = $derived(viewer.expandedValueRows.has(expandKey));
  const display = $derived(escapeBytes(value));
  let overflowing = $state(false);

  // When expanded, treat as overflowing (so the toggle remains active).
  // When collapsed, the action's `scrollWidth > clientWidth` check
  // decides if the value is actually clipped.
  const interactive = $derived(expanded || overflowing);

  function onClick(): void {
    if (!interactive) return;
    viewer.toggleExpandedValueRow(expandKey);
  }

  function onKey(e: KeyboardEvent): void {
    if (!interactive) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      viewer.toggleExpandedValueRow(expandKey);
    }
  }

  function onCopy(e: MouseEvent): void {
    e.stopPropagation();
    copy(value);
  }
</script>

<span class="cell" class:interactive class:accent>
  <button
    class="copy"
    type="button"
    tabindex="-1"
    title="copy value"
    onclick={onCopy}
  >
    &#x29C9;
  </button>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <code
    class:expanded
    role={interactive ? 'button' : undefined}
    tabindex={interactive ? 0 : -1}
    onclick={onClick}
    onkeydown={onKey}
    use:overflowProbe={(o) => (overflowing = o)}
  >{display}</code>
</span>

<style>
  .cell {
    position: relative;
    display: block;
    min-width: 0;
  }
  .cell code {
    display: block;
    font-family: var(--font-mono);
    font-size: inherit;
    background: var(--paper);
    color: inherit;
    padding: 0 4px;
    border-radius: 3px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    outline: none;
  }
  .cell.accent code {
    color: var(--accent);
  }
  .cell.interactive code {
    cursor: pointer;
  }
  .cell code.expanded {
    white-space: pre-wrap;
    word-break: break-all;
    padding: var(--gap-xs) var(--gap-sm);
  }
  .cell code:focus-visible {
    outline: 1px dashed var(--accent);
    outline-offset: -1px;
  }
  /* Overlays the first characters of the code chip on hover so the button
     sits on the LEFT side of the value, sized identically to the text it
     replaces (no padding, transparent background that picks up the chip's
     own paper colour). */
  .copy {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    font: inherit;
    color: var(--ink-faint);
    background: var(--paper);
    border: none;
    padding: 0 4px;
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition: opacity 80ms;
    border-radius: 3px 0 0 3px;
  }
  .cell:hover .copy,
  .cell:focus-within .copy {
    opacity: 1;
    pointer-events: auto;
  }
  .copy:hover {
    color: var(--accent);
  }
</style>
