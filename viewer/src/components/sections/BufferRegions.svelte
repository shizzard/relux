<script lang="ts">
  import { tick } from 'svelte';
  import type { BufferRegions } from '../../lib/derive';
  import { escapeBufferBytes } from '../../lib/format';

  let { regions }: { regions: BufferRegions | null } = $props();

  let preEl: HTMLPreElement | undefined;

  const isEmpty = $derived(
    regions === null ||
      (regions.consumed.length === 0 &&
        regions.matched === null &&
        regions.tail.length === 0),
  );

  // After regions change, scroll the pre so the last match sits at the
  // viewport's vertical center. Phantom padding-bottom (half the viewport
  // height) ensures an end-of-buffer match can still center rather than
  // sticking to the bottom edge. No match -> scroll to the bottom (latest
  // tail / cursor).
  $effect(() => {
    void regions;
    if (!preEl) return;
    const pre = preEl;
    void (async () => {
      await tick();
      const half = pre.clientHeight / 2;
      pre.style.paddingBottom = `${half}px`;
      const matched = pre.querySelector<HTMLElement>('.matched');
      const target = matched
        ? matched.offsetTop + matched.offsetHeight / 2 - half
        : pre.scrollHeight - pre.clientHeight;
      const max = pre.scrollHeight - pre.clientHeight;
      pre.scrollTop = Math.max(0, Math.min(max, target));
    })();
  });
</script>

<pre bind:this={preEl} class="shell" class:empty={isEmpty}>{#if regions === null || isEmpty}<span class="empty-marker">(empty)</span>{:else}<span class="consumed">{escapeBufferBytes(regions.consumed)}</span>{#if regions.matched}<span class="matched">{escapeBufferBytes(regions.matched.bytes)}</span>{/if}<span class="tail">{escapeBufferBytes(regions.tail)}</span><span class="cursor"></span>{/if}</pre>

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
    border-radius: 2px;
    padding: 0 1px;
    /* Quadratic ease-in-out, alternating between 5% and 30% accent
       tint. Same cadence as the source view span frame. */
    animation: matched-pulse 0.8s cubic-bezier(0.45, 0, 0.55, 1) infinite alternate;
  }
  @keyframes matched-pulse {
    from {
      background-color: color-mix(in srgb, var(--accent) 5%, transparent);
    }
    to {
      background-color: color-mix(in srgb, var(--accent) 30%, transparent);
    }
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
    /* Toggle on/off twice per second. `step-end` (== one-step timing)
       holds each segment's start value until the next keyframe. */
    animation: blink 0.5s step-end infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
</style>
