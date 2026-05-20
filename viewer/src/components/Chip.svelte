<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    kbd = null,
    active = false,
    disabled = false,
    onclick,
    title,
    children,
  }: {
    kbd?: string | null;
    active?: boolean;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
    title?: string;
    children: Snippet;
  } = $props();
</script>

<button
  type="button"
  class="chip"
  class:active
  {disabled}
  {title}
  {onclick}
>
  {@render children()}
  {#if kbd !== null}<kbd class="kbd">{kbd}</kbd>{/if}
</button>

<style>
  .chip {
    appearance: none;
    background: transparent;
    border: 1px solid var(--accent);
    color: var(--accent);
    font: inherit;
    font-size: 0.72rem;
    border-radius: 100px;
    padding: 2px 10px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .chip:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .chip.active {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .chip:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .kbd {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: 600;
    line-height: 1;
    padding: 2px 4px;
    border: 1px solid currentColor;
    border-radius: 3px;
    background: color-mix(in srgb, currentColor 8%, transparent);
    opacity: 0.9;
  }
</style>
