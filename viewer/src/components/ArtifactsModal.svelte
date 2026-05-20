<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { encodeArtifactPath, filterArtifacts } from '../lib/artifacts';
  import { copy } from '../lib/clipboard';
  import { formatBytes } from '../lib/format';
  import Modal from './Modal.svelte';

  let { state }: { state: ViewerState } = $props();

  const total = $derived(state.data.artifacts.length);
  const filtered = $derived(filterArtifacts(state.data.artifacts, state.artifactFilter));
</script>

{#if state.openModal === 'artifacts'}
  <Modal
    title="artifacts"
    subtitle={`${total} ${total === 1 ? 'file' : 'files'}`}
    width="50%"
    onClose={() => state.closeArtifacts()}
  >
    {#snippet actions()}
      <button
        class="chip"
        onclick={() => copy(state.data.artifacts.map((r) => r.path).join('\n'))}
      >copy all</button>
    {/snippet}

    <div class="modal-body">
      <div class="filter-row">
        <div class="search-input">
          <span class="glyph">&#x2315;</span>
          <input
            type="search"
            data-search-input
            placeholder={`filter\u2026`}
            bind:value={state.artifactFilter}
            aria-label="filter artifacts"
          />
          <span class="count">{filtered.length} / {total}</span>
        </div>
      </div>

      <div class="list">
        {#if total === 0}
          <p class="empty">no artifacts.</p>
        {:else if filtered.length === 0}
          <p class="empty">no matches.</p>
        {:else}
          {#each filtered as row (row.path)}
            <div class="row">
              <a
                class="path"
                href={`./artifacts/${encodeArtifactPath(row.path)}`}
                target="_blank"
                rel="noopener noreferrer"
              >{row.path}</a>
              <span class="size">{formatBytes(Number(row.size))}</span>
              {#if row.mime}<span class="mime">{row.mime}</span>{/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </Modal>
{/if}

<style>
  .modal-body {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .filter-row {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: var(--gap-sm) var(--gap-lg);
    border-bottom: 1px dashed var(--border);
    flex: 0 0 auto;
  }
  .search-input {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: 6px 10px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .search-input input {
    flex: 1 1 auto;
    background: transparent;
    border: none;
    color: var(--ink);
    font: inherit;
    font-family: var(--font-mono);
    font-size: 0.85rem;
    outline: none;
  }
  .search-input .glyph {
    color: var(--ink-faint);
  }
  .search-input .count {
    font-family: var(--font-mono);
    color: var(--ink-faint);
    font-size: 0.72rem;
  }
  .list {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding: var(--gap-sm) var(--gap-md);
  }
  .empty {
    color: var(--ink-faint);
    font-style: italic;
    margin: var(--gap-lg) 0;
    text-align: center;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--gap-md);
    align-items: baseline;
    padding: 3px var(--gap-sm);
    font-family: var(--font-mono);
    font-size: 0.78rem;
  }
  .row .path {
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-decoration: none;
    border-bottom: 1px dotted var(--ink-dim);
  }
  .row .path:hover {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }
  .row .size,
  .row .mime {
    color: var(--ink-faint);
    font-size: 0.72rem;
  }
  .chip {
    appearance: none;
    background: transparent;
    border: 1px solid var(--ink-faint);
    color: var(--ink-dim);
    font: inherit;
    font-size: 0.74rem;
    border-radius: 100px;
    padding: 2px 10px;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--ink);
    border-color: var(--ink-dim);
  }
</style>
