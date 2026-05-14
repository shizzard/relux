<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';
  import { escapeBytes, formatDuration, truncate } from '../../lib/format';
  import { toNumber as n } from '../../lib/derive';

  let { state }: { state: ViewerState } = $props();

  const captures = $derived(Array.from(state.capturesAt.entries()));
  const vars = $derived(Array.from(state.varsAt.entries()));
  const eventLocals = $derived(buildEventLocals());
  const isEmpty = $derived(
    captures.length === 0 && vars.length === 0 && eventLocals.length === 0,
  );
  const hint = $derived(
    `${captures.length} captures \u00b7 ${vars.length} span-local${
      eventLocals.length > 0 ? ` \u00b7 ${eventLocals.length} event` : ''
    }`,
  );

  function buildEventLocals(): Array<[string, string]> {
    const ev = state.selected;
    if (!ev) return [];
    if (ev.kind === 'match-done') {
      return [
        ['elapsed', formatDuration(ev.elapsed)],
        ['buffer_seq', String(n(ev.buffer_seq))],
      ];
    }
    if (ev.kind === 'match-start') {
      return [
        ['pattern', ev.pattern],
        ['is_regex', String(ev.is_regex)],
      ];
    }
    if (ev.kind === 'timeout') {
      return [['pattern', ev.pattern]];
    }
    return [];
  }
</script>

<Panel title="variables in scope" {hint}>
  <div class="content">
    {#if isEmpty}
      <p class="empty">no variables in scope at this point.</p>
    {:else}
      <table class="kv">
        <tbody>
          {#each captures as [name, value] (`cap:${name}`)}
            <tr>
              <th class="cap"><code>${name}</code></th>
              <td><code class="cap-val">{truncate(escapeBytes(value), 200)}</code></td>
            </tr>
          {/each}
          {#each vars as [name, value] (`var:${name}`)}
            <tr>
              <th>{name}</th>
              <td><code>{truncate(escapeBytes(value), 200)}</code></td>
            </tr>
          {/each}
          {#each eventLocals as [name, value] (`ev:${name}`)}
            <tr>
              <th class="ev">{name}</th>
              <td><code>{truncate(escapeBytes(value), 200)}</code></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <footer class="env-pointer">
      <span class="muted">env vars live in the</span>
      <button class="chip warn" onclick={() => state.openEnv()}>env <span class="kbd">&#x2318;E</span></button>
      <span class="muted">modal</span>
    </footer>
  </div>
</Panel>

<style>
  .content {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding: var(--gap-sm) var(--gap-md);
    display: flex;
    flex-direction: column;
  }
  .empty {
    margin: 0;
    color: var(--ink-faint);
    font-style: italic;
    font-size: 0.85rem;
  }
  .kv {
    border-collapse: collapse;
    font-size: 0.82rem;
    width: 100%;
  }
  .kv th {
    text-align: left;
    color: var(--ink-faint);
    font-weight: 400;
    padding: 2px 8px 2px 0;
    vertical-align: top;
    white-space: nowrap;
  }
  .kv th.cap {
    color: var(--accent);
  }
  .kv th.ev {
    color: var(--accent-2);
  }
  .kv td {
    padding: 2px 0;
    word-break: break-all;
    color: var(--ink);
  }
  code {
    font-family: var(--font-mono);
    background: var(--paper-2);
    color: var(--ink);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.78rem;
  }
  code.cap-val {
    color: var(--accent);
  }
  .env-pointer {
    margin-top: auto;
    padding-top: var(--gap-sm);
    border-top: 1px dashed var(--border);
    display: flex;
    gap: var(--gap-xs);
    align-items: baseline;
    color: var(--ink-faint);
    font-size: 0.75rem;
  }
  .muted {
    color: var(--ink-faint);
  }
  .chip {
    appearance: none;
    background: transparent;
    border: 1px solid var(--accent);
    color: var(--accent);
    font: inherit;
    font-size: 0.7rem;
    border-radius: 100px;
    padding: 1px 8px;
    cursor: pointer;
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
  }
  .chip:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .kbd {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    opacity: 0.85;
  }
</style>
