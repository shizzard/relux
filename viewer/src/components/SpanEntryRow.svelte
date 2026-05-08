<script lang="ts">
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import { formatDuration, spanTitle } from '../lib/format';
  import { effectSetupProps, shellBlockProps, toNumber as n } from '../lib/derive';

  let {
    state,
    span,
    depth,
  }: { state: ViewerState; span: Span; depth: number } = $props();

  const id = $derived(n(span.id));
  const expanded = $derived(state.expandedSpans.has(id));
  const title = $derived(spanTitle(span));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));
  const shellProps = $derived(
    expanded && span.kind === 'shell-block' ? shellBlockProps(state.data, id) : null,
  );
  const effectProps = $derived(
    expanded && span.kind === 'effect-setup' ? effectSetupProps(state.data, id) : null,
  );
  const hasEffectProps = $derived(
    effectProps !== null &&
      (effectProps.overlay.length > 0 ||
        effectProps.shellExposes.length > 0 ||
        effectProps.varExposes.length > 0),
  );
  const propsRails = $derived(Array.from({ length: depth + 1 }, (_, i) => i));
  const ARROW = '\u{2190}';
</script>

<li class="span-row" data-span-id={id}>
  <button class="row" type="button" onclick={() => state.toggleSpan(id)}>
    {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
    <span class="chevron" aria-hidden="true">{expanded ? '\u25BE' : '\u25B8'}</span>
    <span class="kind">{span.kind}</span>
    <span class="title">{title}</span>
  </button>
  {#if shellProps}
    <div class="props-row">
      {#each propsRails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
      <dl class="props">
        <dt>command</dt>
        <dd><code>{shellProps.command}</code></dd>
        {#if shellProps.startupMs !== null}
          <dt>startup</dt>
          <dd>{formatDuration(shellProps.startupMs)}</dd>
        {/if}
      </dl>
    </div>
  {/if}
  {#if hasEffectProps && effectProps}
    <div class="props-row">
      {#each propsRails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
      <dl class="props">
        {#if effectProps.overlay.length > 0}
          <dt>expects</dt>
          <dd>
            <table class="kv">
              <tbody>
                {#each effectProps.overlay as [k, v] (k)}
                  <tr>
                    <th>{k}</th>
                    <td><code>{v}</code></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </dd>
        {/if}
        {#if effectProps.shellExposes.length > 0}
          <dt>exposes shells</dt>
          <dd>
            <ul class="bullets">
              {#each effectProps.shellExposes as e (e.name)}
                <li>
                  <code>{e.name}</code>
                  {#if e.qualifier !== null || e.target !== e.name}
                    <span class="muted">
                      {ARROW}
                      {#if e.qualifier !== null}
                        <code>{e.qualifier}.{e.target}</code>
                      {:else}
                        <code>{e.target}</code>
                      {/if}
                    </span>
                  {/if}
                </li>
              {/each}
            </ul>
          </dd>
        {/if}
        {#if effectProps.varExposes.length > 0}
          <dt>exposes vars</dt>
          <dd>
            <table class="kv">
              <tbody>
                {#each effectProps.varExposes as v (v.name)}
                  <tr>
                    <th>{v.name}</th>
                    <td>
                      <code>{v.value}</code>
                      {#if v.qualifier !== null || v.target !== v.name}
                        <span class="muted">
                          ({#if v.qualifier !== null}<code>{v.qualifier}.{v.target}</code
                            >{:else}<code>{v.target}</code>{/if})
                        </span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </dd>
        {/if}
      </dl>
    </div>
  {/if}
</li>

<style>
  .span-row {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: stretch;
    width: 100%;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    min-height: 26px;
  }
  .row:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .rail {
    width: 24px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
  }
  .chevron {
    width: 20px;
    text-align: center;
    color: var(--muted);
    font-family: var(--font-mono);
    flex: 0 0 auto;
    align-self: center;
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
    min-width: 11ch;
  }
  .title {
    font-weight: 600;
    font-size: 0.9rem;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    align-self: center;
  }
  .props-row {
    display: flex;
    align-items: stretch;
    background: var(--sidebar);
    border-bottom: 1px solid var(--border);
  }
  .props {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px var(--gap-md);
    margin: 0;
    padding: var(--gap-xs) var(--gap-sm);
    font-size: 0.8rem;
    flex: 1 1 auto;
    min-width: 0;
  }
  .props dt {
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 0.75rem;
    align-self: center;
  }
  .props dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .props code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 1px 4px;
    border-radius: var(--radius);
  }
  .kv {
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  .kv th {
    text-align: left;
    color: var(--muted);
    font-weight: 400;
    padding: 1px 8px 1px 0;
    vertical-align: top;
  }
  .kv td {
    padding: 1px 0;
  }
  .bullets {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .bullets li {
    padding: 1px 0;
  }
  .muted {
    color: var(--muted);
  }
</style>
