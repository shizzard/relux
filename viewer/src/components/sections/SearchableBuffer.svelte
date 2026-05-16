<script lang="ts">
  import type { BufferRegions as BufferRegionsType } from '../../lib/derive';
  import { escapeBufferBytes } from '../../lib/format';
  import BufferRegions, { type Hit } from './BufferRegions.svelte';

  let {
    regions,
  }: {
    regions: BufferRegionsType | null;
  } = $props();

  let query = $state('');
  let currentIndex = $state<number | null>(null);

  // The user is searching the on-screen representation, not raw bytes, so
  // escape each region the same way BufferRegions does and concatenate.
  const escapedFull = $derived(buildEscapedFull(regions));

  function buildEscapedFull(r: BufferRegionsType | null): string {
    if (r === null) return '';
    const c = escapeBufferBytes(r.consumed);
    const m = r.matched ? escapeBufferBytes(r.matched.bytes) : '';
    const t = escapeBufferBytes(r.tail);
    return c + m + t;
  }

  const matches = $derived<Hit[]>(findHits(escapedFull, query));

  // Smart-case: insensitive unless the query contains an uppercase letter.
  // Plain substring search (regex chars escaped) iterated via a global
  // RegExp. Empty-query short-circuits.
  function findHits(text: string, q: string): Hit[] {
    if (q.length === 0 || text.length === 0) return [];
    const insensitive = q === q.toLowerCase();
    const re = new RegExp(escapeRegex(q), insensitive ? 'gi' : 'g');
    const out: Hit[] = [];
    for (let m = re.exec(text); m !== null; m = re.exec(text)) {
      const end = m.index + m[0].length;
      out.push({ start: m.index, end });
      if (m.index === re.lastIndex) re.lastIndex++;
    }
    return out;
  }

  function escapeRegex(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  // Whenever the match set changes, snap currentIndex to the first hit when
  // valid; clear when empty. Keeps the cycle pointer sane after typing.
  $effect(() => {
    if (matches.length === 0) {
      currentIndex = null;
    } else if (currentIndex === null || currentIndex >= matches.length) {
      currentIndex = 0;
    }
  });

  const isMac = typeof navigator !== 'undefined' && /Mac|iPod|iPhone|iPad/.test(navigator.platform);
  const kbdLabel = isMac ? '\u2318S' : 'Ctrl+S';

  function onKey(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      if (matches.length === 0) return;
      const delta = event.shiftKey ? -1 : 1;
      const i = currentIndex ?? 0;
      currentIndex = (i + delta + matches.length) % matches.length;
    } else if (event.key === 'Escape') {
      event.preventDefault();
      if (query.length > 0) {
        query = '';
      } else {
        (event.currentTarget as HTMLInputElement).blur();
      }
    }
  }

  const counterText = $derived(
    query.length === 0
      ? null
      : matches.length === 0
        ? 'no matches'
        : `${(currentIndex ?? 0) + 1}/${matches.length}`,
  );
</script>

<div class="searchable">
  <div class="bar">
    <input
      type="search"
      data-search-input
      placeholder="find in buffer\u2026"
      bind:value={query}
      onkeydown={onKey}
      aria-label="search buffer"
    />
    {#if counterText !== null}
      <span class="count" class:zero={matches.length === 0}>{counterText}</span>
    {/if}
    <kbd class="kbd" title="cycle search inputs">{kbdLabel}</kbd>
  </div>
  <div class="buf">
    <BufferRegions {regions} hits={matches} currentHitIndex={currentIndex} />
  </div>
</div>

<style>
  .searchable {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    min-height: 0;
    min-width: 0;
    gap: 4px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: 3px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 3%, transparent);
    flex: 0 0 auto;
  }
  .bar:focus-within {
    border-color: var(--accent);
  }
  input {
    flex: 1 1 auto;
    min-width: 0;
    background: transparent;
    border: none;
    color: var(--ink);
    font-family: var(--font-mono);
    font-size: 0.76rem;
    outline: none;
    padding: 0;
  }
  input::placeholder {
    color: var(--ink-faint);
    font-style: italic;
  }
  .count {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    color: var(--ink-faint);
    flex: 0 0 auto;
  }
  .count.zero {
    color: var(--danger, var(--ink-dim));
  }
  .kbd {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: 600;
    line-height: 1;
    padding: 2px 4px;
    border: 1px solid var(--ink-faint);
    border-radius: 3px;
    color: var(--ink-faint);
    background: color-mix(in srgb, var(--ink-faint) 8%, transparent);
    flex: 0 0 auto;
  }
  .bar:focus-within .kbd {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .buf {
    flex: 1 1 0;
    min-height: 0;
    min-width: 0;
    display: flex;
    align-items: stretch;
  }
</style>
