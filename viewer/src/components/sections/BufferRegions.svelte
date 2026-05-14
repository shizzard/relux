<script lang="ts">
  import type { BufferRegions } from '../../lib/derive';
  import { escapeBufferBytes } from '../../lib/format';

  let { regions }: { regions: BufferRegions | null } = $props();

  const isEmpty = $derived(
    regions === null ||
      (regions.consumed.length === 0 &&
        regions.matched === null &&
        regions.tail.length === 0),
  );
</script>

<pre class="shell" class:empty={isEmpty}>{#if regions === null || isEmpty}<span class="empty-marker">(empty)</span>{:else}<span class="consumed">{escapeBufferBytes(regions.consumed)}</span>{#if regions.matched}<span class="matched">{escapeBufferBytes(regions.matched.bytes)}</span>{/if}<span class="tail">{escapeBufferBytes(regions.tail)}</span><span class="cursor"></span>{/if}</pre>

<style>
  .shell {
    margin: 0;
    padding: var(--gap-sm) var(--gap-md);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    word-break: break-all;
    background: var(--bg-deep);
    border-radius: var(--radius);
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    width: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .shell.empty {
    color: var(--ink-faint);
    font-style: italic;
  }
  .empty-marker {
    color: var(--ink-faint);
  }
  .consumed {
    color: var(--ink-faint);
  }
  .matched {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-radius: 2px;
    padding: 0 1px;
  }
  .tail {
    color: var(--ink);
  }
  .cursor {
    display: inline-block;
    width: 7px;
    height: 1em;
    background: var(--accent);
    margin-left: 1px;
    vertical-align: text-bottom;
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
</style>
