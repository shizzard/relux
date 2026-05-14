<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { formatDuration } from '../lib/format';
  import { outcomeClass } from '../lib/theme';

  let { state }: { state: ViewerState } = $props();

  const test = $derived(state.data.test);
  const cls = $derived(outcomeClass(test.outcome));
  const duration = $derived(formatDuration(Number(test.duration_ms)));
  const shellCount = $derived(Object.keys(state.data.shells).length);
  const eventCount = $derived(state.data.events.length);
  const spanCount = $derived(Object.keys(state.data.spans).length);

  const breadcrumb = $derived(splitPath(test.path));

  function splitPath(path: string): { dir: string; file: string } {
    const slash = path.lastIndexOf('/');
    if (slash === -1) return { dir: '', file: path };
    return { dir: path.slice(0, slash), file: path.slice(slash + 1) };
  }
</script>

<header class="appbar">
  <span class="crumbs">
    {#if breadcrumb.dir.length > 0}<b class="dir">{breadcrumb.dir}</b><span class="sep">/</span>{/if}<span class="file">{breadcrumb.file}</span><span class="sep">&rsaquo;</span><b>{test.name}</b>
  </span>
  <span class="pill {cls}">{test.outcome}</span>
  <span class="chips">
    <button class="chip warn" onclick={() => state.openEnv()} title="environment (cmd-E)">
      env <span class="kbd">&#x2318;E</span>
    </button>
    <button class="chip ok" onclick={() => state.openShells()} title="all shells (cmd-\\)">
      shells &middot; {shellCount} <span class="kbd">&#x2318;\</span>
    </button>
    <span class="chip search" aria-disabled="true" title="search (cmd-K) \u2014 deferred">
      search <span class="kbd">&#x2318;K</span>
    </span>
  </span>
  <span class="timing">{duration} &middot; {eventCount} events &middot; {spanCount} spans</span>
</header>

<style>
  .appbar {
    display: flex;
    align-items: center;
    gap: var(--gap-md);
    padding: var(--gap-sm) var(--gap-lg);
    border-bottom: 1px solid var(--border);
    background: var(--bg);
    flex: 0 0 auto;
    font-size: 0.9rem;
    color: var(--ink-dim);
  }
  .crumbs {
    color: var(--ink);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.9rem;
  }
  .crumbs b {
    font-weight: 700;
  }
  .crumbs .file,
  .crumbs .dir {
    font-family: var(--font-mono);
    color: var(--ink);
  }
  .crumbs .sep {
    color: var(--ink-faint);
    margin: 0 6px;
  }
  .pill {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    padding: 2px 10px;
    border: 1px solid currentColor;
    border-radius: 100px;
    flex: 0 0 auto;
  }
  .pill.pass {
    color: var(--pass);
  }
  .pill.fail {
    color: var(--fail);
  }
  .pill.skip {
    color: var(--skip);
  }
  .pill.invalid {
    color: var(--invalid);
  }
  .chips {
    display: flex;
    gap: var(--gap-xs);
  }
  .chip {
    appearance: none;
    background: transparent;
    border: 1px solid var(--ink-faint);
    color: var(--ink-dim);
    font: inherit;
    font-size: 0.75rem;
    border-radius: 100px;
    padding: 2px 10px;
    cursor: pointer;
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
  }
  .chip:hover {
    color: var(--ink);
    border-color: var(--ink-dim);
  }
  .chip.warn {
    color: var(--accent);
    border-color: var(--accent);
  }
  .chip.warn:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .chip.ok {
    color: var(--accent-2);
    border-color: var(--accent-2);
  }
  .chip.ok:hover {
    background: color-mix(in srgb, var(--accent-2) 12%, transparent);
  }
  .chip.search {
    cursor: not-allowed;
    opacity: 0.6;
  }
  .kbd {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    opacity: 0.75;
  }
  .timing {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink-dim);
    flex: 0 0 auto;
  }
</style>
