<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import type { EnvValue } from '../types/EnvValue';
  import { buildEnvSections, findSuiteRoot } from '../lib/env';
  import { copy } from '../lib/clipboard';
  import Modal from './Modal.svelte';
  import NameCell from './NameCell.svelte';
  import ValueCell from './ValueCell.svelte';

  let { state }: { state: ViewerState } = $props();

  const filtered = $derived(applyFilter(state.data.env.bootstrap));
  // Suite root comes from the full dump, not `filtered`, so `.env` labels stay
  // stable while filtering (the filter can hide `__RELUX_SUITE_ROOT`).
  const suiteRoot = $derived(findSuiteRoot(state.data.env.bootstrap));
  const sections = $derived(buildEnvSections(filtered, suiteRoot));
  const total = $derived(state.data.env.bootstrap.length);
  const filteredCount = $derived(filtered.length);

  const isMac = typeof navigator !== 'undefined' && /Mac|iPod|iPhone|iPad/.test(navigator.platform);
  const kbdLabel = isMac ? '\u2318S' : 'Ctrl+S';

  function applyFilter(rs: EnvValue[]): EnvValue[] {
    const q = state.envFilter.trim();
    if (q.length === 0) return rs;
    const lc = q.toLowerCase();
    return rs.filter((r) => {
      const key = r.key.toLowerCase();
      const val = r.value.toLowerCase();
      switch (state.envFilterScope) {
        case 'name':
          return key.includes(lc);
        case 'value':
          return val.includes(lc);
        case 'name-matches':
          return key.includes(lc) || val.includes(lc);
      }
    });
  }

</script>

{#if state.openModal === 'env'}
  <Modal
    title="environment"
    subtitle={`bootstrap \u00b7 captured at t = 0 \u00b7 ${total} vars`}
    width="50%"
    onClose={() => state.closeEnv()}
  >
    {#snippet actions()}
      <button class="chip" onclick={() => copy(state.data.env.bootstrap.map((r) => `${r.key}=${r.value}`).join('\n'))}>copy all</button>
    {/snippet}

    <div class="modal-body">
      <div class="filter-row">
        <div class="search-input">
          <span class="glyph">&#x2315;</span>
          <input
            type="search"
            data-search-input
            placeholder={`filter\u2026`}
            bind:value={state.envFilter}
            aria-label="filter env vars"
          />
          <span class="count">{filteredCount} / {total}</span>
          <kbd class="kbd" title="cycle search inputs">{kbdLabel}</kbd>
        </div>
        <div class="scope-toggle">
          <button class:active={state.envFilterScope === 'name'} onclick={() => (state.envFilterScope = 'name')}>name</button>
          <button class:active={state.envFilterScope === 'value'} onclick={() => (state.envFilterScope = 'value')}>value</button>
          <button class:active={state.envFilterScope === 'name-matches'} onclick={() => (state.envFilterScope = 'name-matches')}>name &middot; matches</button>
        </div>
      </div>

      <div class="list">
        {#if filtered.length === 0}
          <p class="empty">no matches.</p>
        {/if}
        {#each sections as section (section.id)}
          <div class="group-header" class:path={section.tier === 'dotenv'} title={section.path}>
            &mdash; {section.label} ({section.rows.length})
          </div>
          {#each section.rows as row (section.id + ':' + row.key)}
            <div class="env-row">
              <span class="k"><NameCell name={row.key} /></span>
              <span class="v">
                <ValueCell value={row.value} {state} expandKey={`env:${section.id}:${row.key}`} />
              </span>
            </div>
          {/each}
        {/each}
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
  .search-input .kbd {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: 600;
    line-height: 1;
    padding: 2px 4px;
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .scope-toggle {
    display: flex;
    gap: var(--gap-xs);
  }
  .scope-toggle button {
    appearance: none;
    background: transparent;
    border: 1px solid var(--ink-faint);
    color: var(--ink-dim);
    font: inherit;
    font-size: 0.72rem;
    border-radius: 100px;
    padding: 2px 10px;
    cursor: pointer;
  }
  .scope-toggle button.active {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
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
  .group-header {
    color: var(--ink-faint);
    font-size: 0.76rem;
    padding: var(--gap-sm) var(--gap-xs) 2px;
    text-transform: lowercase;
    letter-spacing: 0.04em;
  }
  /* .env headers carry a real file path; keep the grey header style but skip
     the case-folding so path casing is preserved. */
  .group-header.path {
    text-transform: none;
  }
  .env-row {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: 260px minmax(0, 1fr);
    gap: var(--gap-sm);
    align-items: baseline;
    padding: 3px var(--gap-sm);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink);
  }
  .env-row .v {
    color: var(--ink-dim);
    min-width: 0;
    display: block;
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
