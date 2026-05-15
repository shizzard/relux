<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import Panel from '../Panel.svelte';
  import { truncate } from '../../lib/format';
  import ValueCell from '../ValueCell.svelte';

  let { state }: { state: ViewerState } = $props();

  const frames = $derived(state.callStack.slice().reverse());
  const liveShells = $derived(state.liveShells);
  const hint = $derived(`depth ${frames.length}`);
</script>

<Panel title="call stack" {hint}>
  <div class="content">
    {#if frames.length === 0}
      <p class="empty">no frames for this event.</p>
    {:else}
      <ol class="frames">
        {#each frames as frame, i (i)}
          <li class="frame" class:top={i === 0}>
            <div class="head">
              <span class="idx">{i}</span>
              <span class="kind">{frame.kind}</span>
              {#if frame.name !== null}
                <span class="name">{frame.name}</span>
              {/if}
              {#if frame.alias !== null}
                <span class="alias">as <code>{frame.alias}</code></span>
              {/if}
              {#if frame.location !== null}
                <span class="loc"><code>{frame.location.file}:{frame.location.line}</code></span>
              {/if}
            </div>
            {#if frame.args.length > 0}
              <table class="kv">
                <tbody>
                  {#each frame.args as [k, v] (k)}
                    <tr>
                      <th>{k}</th>
                      <td>
                        <ValueCell value={v} {state} expandKey={`cs:${i}:${k}`} />
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
    {#if liveShells.length > 0}
      <footer class="also-live">
        also live: {liveShells.length} shell{liveShells.length === 1 ? '' : 's'} (
        {#each liveShells as sh, i (sh.marker)}
          {#if i > 0} &middot; {/if}
          <span class={sh.state}><code>{sh.name}</code></span>
          <span class="cmd">{truncate(sh.command, 36)}</span>
          <span class="state">{sh.state}</span>
        {/each}
        )
      </footer>
    {/if}
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
  .frames {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap-xs);
  }
  .frame {
    padding: var(--gap-xs) var(--gap-sm);
    background: var(--paper-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .frame.top {
    border-color: var(--accent);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
    flex-wrap: wrap;
    font-size: 0.82rem;
  }
  .idx {
    font-family: var(--font-mono);
    color: var(--ink-faint);
    width: 18px;
    text-align: right;
    flex: 0 0 auto;
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    color: var(--ink-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    flex: 0 0 auto;
  }
  .name {
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--ink);
    flex: 0 0 auto;
  }
  .alias {
    color: var(--ink-faint);
  }
  .loc {
    margin-left: auto;
    color: var(--ink-faint);
    font-size: 0.75rem;
  }
  code {
    font-family: var(--font-mono);
    background: var(--paper);
    color: var(--ink);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.76rem;
  }
  .kv {
    border-collapse: collapse;
    margin-top: 4px;
    font-size: 0.75rem;
  }
  .kv th {
    text-align: left;
    color: var(--ink-faint);
    font-weight: 400;
    padding: 1px 8px 1px 0;
    vertical-align: top;
  }
  .kv td {
    padding: 1px 0;
    min-width: 0;
    max-width: 0;
    width: 100%;
  }
  .also-live {
    margin-top: var(--gap-sm);
    padding-top: var(--gap-sm);
    border-top: 1px dashed var(--border);
    color: var(--ink-faint);
    font-size: 0.78rem;
    line-height: 1.45;
  }
  .also-live .ready {
    color: var(--accent-2);
  }
  .also-live .busy {
    color: var(--accent);
  }
  .also-live .ended {
    color: var(--ink-faint);
  }
  .also-live .error {
    color: var(--danger);
  }
  .also-live .cmd {
    font-family: var(--font-mono);
    color: var(--ink-dim);
  }
  .also-live .state {
    color: var(--ink-faint);
    font-style: italic;
  }
</style>
