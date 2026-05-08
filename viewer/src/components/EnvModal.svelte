<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  let { state }: { state: ViewerState } = $props();
</script>

{#if state.envModalOpen}
  <div
    class="overlay"
    role="presentation"
    onclick={() => state.closeEnv()}
    onkeydown={(e) => {
      if (e.key === 'Escape') state.closeEnv();
    }}
  >
    <div
      class="modal"
      role="dialog"
      aria-label="Environment"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <header class="modal-header">
        <h2>Environment</h2>
        <button class="close" onclick={() => state.closeEnv()}>close</button>
      </header>
      <div class="body">
        <p class="empty">&lt;empty&gt;</p>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--fg) 30%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    width: min(720px, 92vw);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px color-mix(in srgb, var(--fg) 20%, transparent);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--gap-md) var(--gap-lg);
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }
  .close {
    border: 1px solid var(--border);
    background: transparent;
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .close:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .body {
    padding: var(--gap-lg);
    overflow-y: auto;
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-style: italic;
  }
</style>
