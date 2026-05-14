<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { formatBytes } from '../lib/format';
  import Modal from './Modal.svelte';

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

  async function copy(value: string): Promise<void> {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(value);
        return;
      }
    } catch {
      // fall through
    }
    const el = document.createElement('textarea');
    el.value = value;
    el.style.position = 'fixed';
    el.style.opacity = '0';
    document.body.appendChild(el);
    el.select();
    try {
      document.execCommand('copy');
    } catch {
      // best-effort
    }
    document.body.removeChild(el);
  }

  function highlight(text: string, query: string): string {
    if (query.length === 0) return text;
    return text; // mark element handled inline below via split
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      const sel = state.envSelectedKey;
      if (sel === null) return;
      const row = rows.find((r) => r.key === sel);
      if (!row) return;
      event.preventDefault();
      if (event.shiftKey) copy(`${row.key}=${row.value}`);
      else copy(row.value);
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const cur = filtered.findIndex((r) => r.key === state.envSelectedKey);
      let next = cur + (event.key === 'ArrowDown' ? 1 : -1);
      if (next < 0) next = 0;
      if (next >= filtered.length) next = filtered.length - 1;
      const target = filtered[next];
      if (target) {
        state.envSelectedKey = target.key;
        event.preventDefault();
      }
    }
  }

  function selectedDisplay(): string {
    const sel = state.envSelectedKey;
    if (sel === null) return '\u2014 select a row \u2014';
    const row = rows.find((r) => r.key === sel);
    return row ? `${row.key}=${row.value}` : '\u2014';
  }

  function toggleBlob(key: string): void {
    state.toggleEnvExpandedBlob(key);
  }
</script>

{#if state.openModal === 'env'}
  <Modal
    title="environment"
    subtitle={`bootstrap \u00b7 captured at t = 0 \u00b7 ${total} vars`}
    onClose={() => state.closeEnv()}
  >
    {#snippet actions()}
      <button class="chip" onclick={() => copy(rows.map((r) => `${r.key}=${r.value}`).join('\n'))}>copy all</button>
    {/snippet}

    <div class="modal-body" onkeydown={handleKeydown} role="presentation">
      <div class="filter-row">
        <div class="search-input">
          <span class="glyph">&#x2315;</span>
          <input
            type="search"
            placeholder="filter\u2026"
            bind:value={state.envFilter}
            aria-label="filter env vars"
          />
          <span class="count">{filteredCount} / {total}</span>
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
            {@const isLarge = row.group === 'large-blobs'}
            {@const open = state.envExpandedBlobs.has(row.key)}
            <div
              class="env-row"
              class:selected={state.envSelectedKey === row.key}
              role="button"
              tabindex="0"
              onclick={() => (state.envSelectedKey = row.key)}
              ondblclick={() => copy(row.value)}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  state.envSelectedKey = row.key;
                }
              }}
            >
              <span class="k">{row.key}</span>
              <span class="v">
                {#if isLarge && !open}
                  <span class="size-badge">({formatBytes(row.size)} \u00b7 click to expand)</span>
                {:else}
                  <code>{row.value}</code>
                {/if}
              </span>
              {#if isLarge}
                <button
                  class="blob-toggle"
                  type="button"
                  onclick={(e) => {
                    e.stopPropagation();
                    toggleBlob(row.key);
                  }}
                >
                  {open ? 'collapse' : 'expand'}
                </button>
              {:else}
                <button
                  class="copy"
                  type="button"
                  onclick={(e) => {
                    e.stopPropagation();
                    copy(row.value);
                  }}
                  title="copy value"
                >&#x29C9;</button>
              {/if}
            </div>
          {/each}
        {/each}
      </div>

      <footer class="sticky">
        <span class="muted">selected:</span>
        <span class="selected-row">{selectedDisplay()}</span>
        <span class="hint">&#x23ce; copy value \u00b7 shift+&#x23ce; copy KEY=VALUE</span>
      </footer>
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
    grid-template-columns: 260px minmax(0, 1fr) 28px;
    gap: var(--gap-sm);
    align-items: baseline;
    padding: 3px var(--gap-sm);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink);
    cursor: pointer;
  }
  .env-row:hover {
    background: color-mix(in srgb, var(--ink) 5%, transparent);
  }
  .env-row.selected {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    outline: 1px dashed var(--accent);
    outline-offset: -1px;
  }
  .env-row .k {
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .env-row .v {
    color: var(--ink-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .env-row .v code {
    background: transparent;
    color: inherit;
    padding: 0;
  }
  .size-badge {
    color: var(--ink-faint);
    font-style: italic;
  }
  .copy,
  .blob-toggle {
    appearance: none;
    background: transparent;
    border: none;
    color: var(--ink-faint);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    padding: 0 4px;
  }
  .blob-toggle {
    color: var(--accent);
    font-size: 0.7rem;
  }
  .copy:hover,
  .blob-toggle:hover {
    color: var(--accent);
  }
  .sticky {
    display: flex;
    gap: var(--gap-md);
    align-items: center;
    padding: var(--gap-sm) var(--gap-lg);
    border-top: 1px dashed var(--border);
    background: rgba(0, 0, 0, 0.18);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    flex: 0 0 auto;
  }
  .sticky .muted {
    color: var(--ink-faint);
  }
  .sticky .selected-row {
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1 1 auto;
    min-width: 0;
  }
  .sticky .hint {
    margin-left: auto;
    color: var(--ink-faint);
    font-size: 0.74rem;
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
