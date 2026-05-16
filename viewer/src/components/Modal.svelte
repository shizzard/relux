<script lang="ts">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  let {
    title,
    subtitle = null,
    onClose,
    actions,
    children,
  }: {
    title: string;
    subtitle?: string | null;
    onClose: () => void;
    actions?: Snippet;
    children: Snippet;
  } = $props();

  let dialogEl: HTMLElement | undefined;

  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialogEl?.focus();
    return () => {
      previous?.focus?.();
    };
  });

  function handleKey(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
    }
  }
</script>

<div
  class="overlay"
  role="presentation"
  onclick={onClose}
  onkeydown={handleKey}
>
  <div
    class="dialog"
    role="dialog"
    aria-label={title}
    tabindex="-1"
    bind:this={dialogEl}
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    }}
  >
    <header class="modal-header">
      <h2>{title}</h2>
      {#if subtitle !== null}
        <span class="subtitle">{subtitle}</span>
      {/if}
      <div class="actions">
        {#if actions}{@render actions()}{/if}
        <span class="esc">esc</span>
        <button class="x" type="button" aria-label="close" onclick={onClose}>&times;</button>
      </div>
    </header>
    <div class="body">{@render children()}</div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(7, 11, 18, 0.72);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .dialog {
    background: var(--paper);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    width: calc(100% - 56px);
    height: calc(100% - 56px);
    display: flex;
    flex-direction: column;
    box-shadow: 8px 12px 0 rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  .dialog:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .modal-header {
    display: flex;
    align-items: center;
    gap: var(--gap-md);
    padding: var(--gap-md) var(--gap-lg);
    border-bottom: 1px dashed var(--border);
    flex: 0 0 auto;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--ink);
  }
  .subtitle {
    color: var(--ink-dim);
    font-family: var(--font-mono);
    font-size: 0.78rem;
  }
  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--gap-sm);
    align-items: center;
  }
  .esc {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--ink-faint);
  }
  .x {
    width: 26px;
    height: 26px;
    border: 1px solid var(--ink-faint);
    border-radius: 4px;
    background: transparent;
    color: var(--ink-dim);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .x:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .body {
    flex: 1 1 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
