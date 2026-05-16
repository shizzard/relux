<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { copy } from '../lib/clipboard';
  import Modal from './Modal.svelte';
  import ValueCell from './ValueCell.svelte';

  let { state }: { state: ViewerState } = $props();

  type EnvRow = { key: string; value: string; size: number; group: GroupKey };
  type GroupKey = 'relux' | 'cargo' | 'nix' | 'shell' | 'large-blobs' | 'other';

  const GROUP_ORDER: GroupKey[] = ['relux', 'cargo', 'nix', 'shell', 'large-blobs', 'other'];
  const GROUP_LABEL: Record<GroupKey, string> = {
    relux: 'relux internals',
    cargo: 'cargo',
    nix: 'nix / toolchain',
    shell: 'shell & terminal',
    'large-blobs': 'large blobs',
    other: 'other',
  };
  // Threshold used purely for grouping in the modal; rendering itself is
  // uniform across all sizes thanks to ValueCell's CSS-driven truncation.
  const LARGE_BLOB = 1024;

  function groupOf(key: string, byteLen: number): GroupKey {
    if (byteLen > LARGE_BLOB) return 'large-blobs';
    if (key.startsWith('__RELUX')) return 'relux';
    if (key.startsWith('CARGO')) return 'cargo';
    if (
      key.startsWith('NIX') ||
      key === 'IN_NIX_SHELL' ||
      key === 'NIX_STORE'
    )
      return 'nix';
    if (
      key === 'SHELL' ||
      key === 'TERM' ||
      key === 'PAGER' ||
      key === 'EDITOR' ||
      key === 'PS1' ||
      key === 'PROMPT_COMMAND'
    )
      return 'shell';
    return 'other';
  }

  const rows = $derived<EnvRow[]>(buildRows());
  const filtered = $derived(applyFilter(rows));
  const grouped = $derived(groupRows(filtered));
  const total = $derived(rows.length);
  const filteredCount = $derived(filtered.length);

  const isMac = typeof navigator !== 'undefined' && /Mac|iPod|iPhone|iPad/.test(navigator.platform);
  const kbdLabel = isMac ? '\u2318S' : 'Ctrl+S';

  function buildRows(): EnvRow[] {
    return state.data.env.bootstrap.map(([k, v]) => ({
      key: k,
      value: v,
      size: byteLength(v),
      group: groupOf(k, byteLength(v)),
    }));
  }

  function byteLength(s: string): number {
    // approximate: utf-16 length is close enough for grouping
    return s.length;
  }

  function applyFilter(rs: EnvRow[]): EnvRow[] {
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

  function groupRows(rs: EnvRow[]): Array<{ group: GroupKey; rows: EnvRow[] }> {
    const buckets = new Map<GroupKey, EnvRow[]>();
    for (const r of rs) {
      let bucket = buckets.get(r.group);
      if (!bucket) {
        bucket = [];
        buckets.set(r.group, bucket);
      }
      bucket.push(r);
    }
    return GROUP_ORDER.filter((g) => buckets.has(g)).map((g) => ({
      group: g,
      rows: buckets.get(g)!,
    }));
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
      <button class="chip" onclick={() => copy(rows.map((r) => `${r.key}=${r.value}`).join('\n'))}>copy all</button>
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
        {#each grouped as group (group.group)}
          <div class="group-header">&mdash; {GROUP_LABEL[group.group]} ({group.rows.length})</div>
          {#each group.rows as row (row.key)}
            <div class="env-row">
              <span class="k">{row.key}</span>
              <span class="v">
                <ValueCell value={row.value} {state} expandKey={`env:${row.key}`} />
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
  .env-row .k {
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
