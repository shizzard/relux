<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { formatDuration } from '../lib/format';
  import { outcomeClass } from '../lib/theme';
  import Chip from './Chip.svelte';

  let { state }: { state: ViewerState } = $props();

  const info = $derived(state.data.info);
  const outcomeKind = $derived(state.data.outcome.kind);
  const cls = $derived(outcomeClass(outcomeKind));
  // Display label for the pill — kept short and parallel (PASS/FAIL/CANCEL).
  const pillLabel = $derived(outcomeKind === 'cancelled' ? 'cancel' : outcomeKind);
  const duration = $derived(formatDuration(Number(info.duration_ms)));
  const shellCount = $derived(Object.keys(state.data.shells).length);
  const artifactCount = $derived(state.data.artifacts.length);
  const eventCount = $derived(state.data.events.length);
  const spanCount = $derived(Object.keys(state.data.spans).length);

  const breadcrumb = $derived(splitPath(info.path));

  function splitPath(path: string): { dir: string; file: string } {
    const slash = path.lastIndexOf('/');
    if (slash === -1) return { dir: '', file: path };
    return { dir: path.slice(0, slash), file: path.slice(slash + 1) };
  }
</script>

<header class="appbar">
  <span class="crumbs">
    {#if breadcrumb.dir.length > 0}<b class="dir">{breadcrumb.dir}</b><span class="sep">/</span>{/if}<span class="file">{breadcrumb.file}</span><span class="sep">&rsaquo;</span><b>{info.name}</b>
  </span>
  <span class="pill {cls}">{pillLabel}</span>
  <span class="chips">
    <Chip kbd="E" onclick={() => state.openEnv()} title="environment (E)">env</Chip>
    <Chip kbd="S" onclick={() => state.openShells()} title="all shells (S)">shells &middot; {shellCount}</Chip>
    <Chip
      kbd="A"
      disabled={artifactCount === 0}
      onclick={() => state.openArtifacts()}
      title="artifacts (A)"
    >artifacts &middot; {artifactCount}</Chip>
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
  .pill.cancel {
    color: var(--cancel);
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
  .timing {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink-dim);
    flex: 0 0 auto;
  }
</style>
