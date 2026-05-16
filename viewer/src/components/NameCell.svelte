<script lang="ts">
  import { overflowProbe } from '../lib/actions';

  let { name, accent = false }: { name: string; accent?: boolean } = $props();

  const HOVER_DELAY_MS = 500;

  let overflowing = $state(false);
  let revealed = $state(false);
  let timer: number | null = null;

  function onEnter(): void {
    if (!overflowing) return;
    cancelTimer();
    timer = window.setTimeout(() => {
      timer = null;
      revealed = true;
    }, HOVER_DELAY_MS);
  }

  function onLeave(): void {
    cancelTimer();
    revealed = false;
  }

  function cancelTimer(): void {
    if (timer !== null) {
      window.clearTimeout(timer);
      timer = null;
    }
  }

  // If the cell stops overflowing while the overlay is showing (e.g. parent
  // column resized so the name now fits), close immediately. Keeps the
  // overlay honest with the underlying overflow state.
  $effect(() => {
    if (!overflowing) {
      cancelTimer();
      revealed = false;
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span
  class="cell"
  class:accent
  onmouseenter={onEnter}
  onmouseleave={onLeave}
>
  <span class="text" use:overflowProbe={(o) => (overflowing = o)}>{name}</span>
  {#if revealed}
    <span class="overlay" aria-hidden="true">{name}</span>
  {/if}
</span>

<style>
  .cell {
    position: relative;
    display: block;
    min-width: 0;
    max-width: 100%;
  }
  .text {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cell.accent .text {
    color: var(--accent);
  }
  .overlay {
    position: absolute;
    left: 0;
    top: 0;
    width: max-content;
    white-space: nowrap;
    background: var(--paper);
    color: inherit;
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
    margin: -1px -5px;
    z-index: 5;
    pointer-events: none;
  }
  .cell.accent .overlay {
    color: var(--accent);
  }
</style>
