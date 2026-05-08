<script lang="ts">
  import type { Event } from '../types/Event';
  import { escapeBytes, formatDuration } from '../lib/format';

  let { event }: { event: Event } = $props();
</script>

<dl class="details">
  {#if event.kind === 'send' || event.kind === 'recv'}
    <dt>data</dt>
    <dd><pre class="bytes">{escapeBytes(event.data)}</pre></dd>
  {:else if event.kind === 'match-start'}
    <dt>pattern</dt>
    <dd><code>{event.pattern}</code></dd>
    <dt>regex</dt>
    <dd>{event.is_regex ? 'yes' : 'no'}</dd>
  {:else if event.kind === 'match-done'}
    <dt>matched</dt>
    <dd><pre class="bytes">{escapeBytes(event.matched)}</pre></dd>
    <dt>elapsed</dt>
    <dd>{formatDuration(event.elapsed)}</dd>
    {#if event.captures}
      <dt>captures</dt>
      <dd>
        <table class="kv">
          <tbody>
          {#each Object.entries(event.captures) as [name, value] (name)}
            <tr>
              <th>{name}</th>
              <td><code>{value}</code></td>
            </tr>
          {/each}
          </tbody>
        </table>
      </dd>
    {/if}
  {:else if event.kind === 'timeout'}
    <dt>pattern</dt>
    <dd><code>{event.pattern}</code></dd>
  {:else if event.kind === 'fail-pattern-set'}
    <dt>pattern</dt>
    <dd><code>{event.pattern}</code></dd>
  {:else if event.kind === 'fail-pattern-cleared'}
    <dt>fail patterns</dt>
    <dd>cleared</dd>
  {:else if event.kind === 'fail-pattern-triggered'}
    <dt>pattern</dt>
    <dd><code>{event.pattern}</code></dd>
    <dt>matched line</dt>
    <dd><pre class="bytes">{escapeBytes(event.matched_line)}</pre></dd>
  {:else if event.kind === 'sleep-start'}
    <dt>duration</dt>
    <dd>{formatDuration(event.duration)}</dd>
  {:else if event.kind === 'timeout-set'}
    <dt>timeout</dt>
    <dd>{event.timeout}</dd>
    <dt>previous</dt>
    <dd>{event.previous}</dd>
  {:else if event.kind === 'var-let' || event.kind === 'var-assign'}
    <dt>name</dt>
    <dd><code>{event.name}</code></dd>
    <dt>value</dt>
    <dd><pre class="bytes">{escapeBytes(event.value)}</pre></dd>
  {:else if event.kind === 'string-eval'}
    <dt>result</dt>
    <dd><pre class="bytes">{escapeBytes(event.result)}</pre></dd>
  {:else if event.kind === 'interpolation'}
    <dt>template</dt>
    <dd><pre class="bytes">{escapeBytes(event.template)}</pre></dd>
    <dt>result</dt>
    <dd><pre class="bytes">{escapeBytes(event.result)}</pre></dd>
    {#if event.bindings.length > 0}
      <dt>bindings</dt>
      <dd>
        <table class="kv">
          <tbody>
          {#each event.bindings as [name, value] (name)}
            <tr>
              <th>{name}</th>
              <td><code>{value}</code></td>
            </tr>
          {/each}
          </tbody>
        </table>
      </dd>
    {/if}
  {:else if event.kind === 'annotate'}
    <dt>text</dt>
    <dd>{event.text}</dd>
  {:else if event.kind === 'log' || event.kind === 'warning' || event.kind === 'error'}
    <dt>message</dt>
    <dd>{event.message}</dd>
  {:else if event.kind === 'shell-spawn'}
    <dt>name</dt>
    <dd><code>{event.name}</code></dd>
    <dt>command</dt>
    <dd><code>{event.command}</code></dd>
  {:else if event.kind === 'shell-ready' || event.kind === 'shell-switch' || event.kind === 'shell-terminate'}
    <dt>shell</dt>
    <dd><code>{event.name}</code></dd>
  {:else}
    <dt>&nbsp;</dt>
    <dd class="muted">no additional details</dd>
  {/if}
</dl>

<style>
  .details {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--gap-xs) var(--gap-md);
    margin: 0;
    padding: var(--gap-sm) var(--gap-md);
    font-size: 0.85rem;
  }
  dt {
    color: var(--muted);
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    align-self: start;
    padding-top: 2px;
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  .bytes {
    margin: 0;
    padding: var(--gap-xs) var(--gap-sm);
    background: var(--code-bg);
    color: var(--code-fg);
    border-radius: var(--radius);
    font-family: var(--font-mono);
    font-size: 0.85rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 240px;
    overflow-y: auto;
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 1px 4px;
    border-radius: var(--radius);
    font-size: 0.85rem;
  }
  .kv {
    border-collapse: collapse;
    font-size: 0.85rem;
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
  .muted {
    color: var(--muted);
    font-style: italic;
  }
</style>
