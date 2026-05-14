<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';
  import ValueCell from '../ValueCell.svelte';

  let { state }: { state: ViewerState } = $props();

  const notApplicable = $derived(state.varsAt === null && state.capturesAt === null);
  const captures = $derived(Array.from((state.capturesAt ?? new Map()).entries()));
  const vars = $derived(Array.from((state.varsAt ?? new Map()).entries()));
  const isEmpty = $derived(
    !notApplicable && captures.length === 0 && vars.length === 0,
  );
  const hint = $derived(
    notApplicable ? '' : `${captures.length} captures \u00b7 ${vars.length} vars`,
  );
</script>

<Panel title="variables in scope" {hint}>
  <div class="content">
    {#if notApplicable}
      <p class="empty">no scope context at this point.</p>
    {:else if isEmpty}
      <p class="empty">no variables in scope at this point.</p>
    {:else}
      <table class="kv">
        <tbody>
          {#each captures as [name, value] (`cap:${name}`)}
            <tr>
              <th class="cap">${name}</th>
              <td>
                <ValueCell {value} {state} expandKey={`var:cap:${name}`} accent />
              </td>
            </tr>
          {/each}
          {#each vars as [name, value] (`var:${name}`)}
            <tr>
              <th>{name}</th>
              <td>
                <ValueCell {value} {state} expandKey={`var:${name}`} />
              </td>
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
    font-family: var(--font-mono);
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
  .kv td {
    padding: 2px 0;
    color: var(--ink);
    min-width: 0;
    max-width: 0;
    width: 100%;
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
