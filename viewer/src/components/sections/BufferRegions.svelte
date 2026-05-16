<script lang="ts">
  import { tick } from 'svelte';
  import type { BufferRegions } from '../../lib/derive';
  import { escapeBufferBytes } from '../../lib/format';

  export type Hit = { start: number; end: number };

  let {
    regions,
    hits = [],
    currentHitIndex = null,
  }: {
    regions: BufferRegions | null;
    hits?: readonly Hit[];
    currentHitIndex?: number | null;
  } = $props();

  let preEl: HTMLPreElement | undefined;

  const isEmpty = $derived(
    regions === null ||
      (regions.consumed.length === 0 &&
        regions.matched === null &&
        regions.tail.length === 0),
  );

  // Escape each region independently, concatenate to form the user-visible
  // text. Search positions live in this concatenated space so a hit that
  // straddles the consumed/matched boundary is just a normal interval.
  const escaped = $derived(buildEscaped(regions));

  function buildEscaped(r: BufferRegions | null): {
    consumed: string;
    matched: string;
    tail: string;
    full: string;
    matchedRange: { s: number; e: number } | null;
  } {
    if (r === null) {
      return { consumed: '', matched: '', tail: '', full: '', matchedRange: null };
    }
    const consumed = escapeBufferBytes(r.consumed);
    const matched = r.matched ? escapeBufferBytes(r.matched.bytes) : '';
    const tail = escapeBufferBytes(r.tail);
    const full = consumed + matched + tail;
    const matchedRange = r.matched
      ? { s: consumed.length, e: consumed.length + matched.length }
      : null;
    return { consumed, matched, tail, full, matchedRange };
  }

  // Build a list of segments [start, end, classes, hitIndex] over the
  // concatenated escaped text. Each segment is bounded by edges of the
  // matched range and every search hit, so all overlap cases collapse to a
  // single set of active classes per segment.
  type Segment = {
    start: number;
    end: number;
    matched: boolean;
    hit: boolean;
    hitIndex: number | null;
  };

  const segments = $derived<Segment[]>(buildSegments());

  function buildSegments(): Segment[] {
    const text = escaped.full;
    if (text.length === 0) return [];
    // Always edge at consumed/tail boundary so the two regions render as
    // separate spans even when there is no matched range.
    const edges = new Set<number>([0, text.length, escaped.consumed.length]);
    if (escaped.matchedRange !== null) {
      edges.add(escaped.matchedRange.s);
      edges.add(escaped.matchedRange.e);
    }
    for (const h of hits) {
      edges.add(h.start);
      edges.add(h.end);
    }
    const sorted = Array.from(edges).sort((a, b) => a - b);
    const out: Segment[] = [];
    for (let i = 0; i < sorted.length - 1; i++) {
      const start = sorted[i]!;
      const end = sorted[i + 1]!;
      if (start === end) continue;
      const matched =
        escaped.matchedRange !== null &&
        start >= escaped.matchedRange.s &&
        start < escaped.matchedRange.e;
      let hitIndex: number | null = null;
      for (let j = 0; j < hits.length; j++) {
        const h = hits[j]!;
        if (start >= h.start && start < h.end) {
          hitIndex = j;
          break;
        }
      }
      out.push({ start, end, matched, hit: hitIndex !== null, hitIndex });
    }
    return out;
  }

  // After regions or current-hit change, scroll the pre so the focus target
  // sits at the viewport's vertical center. Search-current wins over the
  // test's last-match; falls back to bottom-of-buffer when neither exists.
  $effect(() => {
    void regions;
    void currentHitIndex;
    void hits;
    if (!preEl) return;
    const pre = preEl;
    void (async () => {
      await tick();
      const half = pre.clientHeight / 2;
      pre.style.paddingBottom = `${half}px`;
      const target =
        pre.querySelector<HTMLElement>('.search-hit-current') ??
        pre.querySelector<HTMLElement>('.matched');
      const top = target
        ? target.offsetTop + target.offsetHeight / 2 - half
        : pre.scrollHeight - pre.clientHeight;
      const max = pre.scrollHeight - pre.clientHeight;
      pre.scrollTop = Math.max(0, Math.min(max, top));
    })();
  });

  // Class priority: current-hit beats matched beats search-hit. Base color
  // (consumed/tail) is always applied so a hit in the dim consumed region
  // stays dim, except on matched (which sets its own accent color) or
  // search-hit-current (accent color via its own rule).
  function segmentClass(seg: Segment): string {
    const isCurrent = seg.hit && seg.hitIndex === currentHitIndex;
    if (isCurrent) return 'search-hit-current';
    if (seg.matched) return 'matched';
    const baseColor = seg.start < escaped.consumed.length ? 'consumed' : 'tail';
    return seg.hit ? `search-hit ${baseColor}` : baseColor;
  }
</script>

<pre bind:this={preEl} class="shell" class:empty={isEmpty}>{#if regions === null || isEmpty}<span class="empty-marker">(empty)</span>{:else}{#each segments as seg, i (i)}<span class={segmentClass(seg)} data-hit-index={seg.hitIndex !== null ? seg.hitIndex : undefined}>{escaped.full.slice(seg.start, seg.end)}</span>{/each}<span class="cursor"></span>{/if}</pre>

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
  .search-hit {
    background-color: color-mix(in srgb, var(--accent) 12%, transparent);
    border-radius: 2px;
  }
  .search-hit-current {
    background-color: color-mix(in srgb, var(--accent) 36%, transparent);
    color: var(--accent);
    border-radius: 2px;
    outline: 1px solid var(--accent);
    outline-offset: 0;
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
