<script lang="ts">
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import {
    candidateSpansAt,
    eventRect,
    spanRect,
    type Rect,
  } from '../lib/timeline';
  import { foldEvents, type FoldedEvent } from '../lib/flatten';
  import { toNumber as n } from '../lib/derive';
  import TimelinePreviewCard from './TimelinePreviewCard.svelte';

  let { state: vs }: { state: ViewerState } = $props();

  const MIN_WIDTH_PX = 3;
  const CARD_WIDTH_PX = 300;

  let trackEl = $state<HTMLDivElement | undefined>(undefined);
  let containerWidth = $state(0);
  let cursorPercent = $state<number | null>(null);
  let lastCandidateKey = '';

  $effect(() => {
    if (!trackEl) return;
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      containerWidth = entry.contentRect.width;
    });
    ro.observe(trackEl);
    return () => ro.disconnect();
  });

  const selectedFolded = $derived<FoldedEvent | null>(computeSelectedFolded());

  function computeSelectedFolded(): FoldedEvent | null {
    if (vs.selected === null) return null;
    const folded = foldEvents(vs.data.events);
    const targetSeq = vs.selected.seq;
    for (const f of folded) {
      if (f.kind === 'single' && f.event.seq === targetSeq) return f;
      if (f.kind === 'sleep' && (f.start.seq === targetSeq || f.done.seq === targetSeq))
        return f;
      if (f.kind === 'match' && (f.start.seq === targetSeq || f.outcome.seq === targetSeq))
        return f;
    }
    return null;
  }

  const selectedRect = $derived<Rect | null>(computeSelectedRect());

  function computeSelectedRect(): Rect | null {
    if (containerWidth === 0) return null;
    if (vs.selectedSpan !== null) {
      return spanRect(vs.selectedSpan, vs.timeRange, MIN_WIDTH_PX, containerWidth);
    }
    if (selectedFolded !== null) {
      return eventRect(selectedFolded, vs.timeRange, MIN_WIDTH_PX, containerWidth);
    }
    return null;
  }

  function onMouseMove(e: MouseEvent): void {
    if (!trackEl) return;
    const rect = trackEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const pct = rect.width > 0 ? (x / rect.width) * 100 : 0;
    const clamped = Math.max(0, Math.min(100, pct));
    cursorPercent = clamped;

    const cursorTs =
      vs.timeRange.start + (clamped / 100) * vs.timeRange.duration;
    const candidates = candidateSpansAt(vs.data, cursorTs);
    const key = candidates
      .map((s) => String(s.id))
      .sort()
      .join(',');

    if (key !== lastCandidateKey) {
      lastCandidateKey = key;
      vs.timelineHover =
        candidates.length > 0 ? { percent: clamped, spans: candidates } : null;
    }
  }

  function onMouseLeave(): void {
    cursorPercent = null;
    lastCandidateKey = '';
    vs.timelineHover = null;
    // Note: do NOT clear timelineCardFocus here — the cursor may be
    // moving from the bar into a card. The card's own onmouseleave
    // clears it.
  }

  function onTrackClick(e: MouseEvent): void {
    if (!trackEl) return;
    const rect = trackEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const pct = rect.width > 0 ? (x / rect.width) * 100 : 0;
    const clamped = Math.max(0, Math.min(100, pct));

    const cursorTs =
      vs.timeRange.start + (clamped / 100) * vs.timeRange.duration;
    const candidates = candidateSpansAt(vs.data, cursorTs);

    if (candidates.length === 0) return;

    if (candidates.length === 1) {
      vs.revealAndSelect(n(candidates[0]!.id));
      vs.timelinePin = null;
      return;
    }

    vs.timelineHover = null;
    vs.timelinePin = { percent: clamped, spans: candidates };
  }

  function onCardClick(span: Span): void {
    vs.revealAndSelect(n(span.id));
    vs.timelinePin = null;
  }

  // Document-level click handler to dismiss a pin when the user clicks
  // outside the bar / card stack.
  $effect(() => {
    function handler(e: MouseEvent) {
      if (vs.timelinePin === null) return;
      const target = e.target;
      if (!(target instanceof Node)) return;
      if (trackEl && trackEl.contains(target)) return;
      const stack = document.querySelector('[data-timeline-card-stack="true"]');
      if (stack && stack.contains(target)) return;
      vs.timelinePin = null;
    }
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  });

  type PreviewSource = { percent: number; spans: Span[] };
  const previewSource = $derived<PreviewSource | null>(
    vs.timelinePin ?? vs.timelineHover,
  );

  const previewCards = $derived<Span[]>(
    previewSource === null
      ? []
      : previewSource.spans.filter((s) => n(s.id) !== vs.selectedSpanId),
  );

  type SelectedHover =
    | { kind: 'span'; span: Span }
    | { kind: 'event'; folded: FoldedEvent }
    | null;

  const selectedHover = $derived<SelectedHover>(computeSelectedHover());

  function computeSelectedHover(): SelectedHover {
    if (cursorPercent === null) return null;
    if (vs.timelinePin !== null) return null;
    if (selectedRect === null) return null;
    if (
      cursorPercent < selectedRect.leftPct ||
      cursorPercent > selectedRect.leftPct + selectedRect.widthPct
    ) {
      return null;
    }
    if (vs.selectedSpan !== null) return { kind: 'span', span: vs.selectedSpan };
    if (selectedFolded !== null) return { kind: 'event', folded: selectedFolded };
    return null;
  }

  function previewRect(span: Span): Rect {
    return spanRect(span, vs.timeRange, MIN_WIDTH_PX, containerWidth);
  }

  // Card left position in pixels relative to the track's left edge.
  // First tries to align the card's left border with the slice's left
  // border; if the resulting card would overflow the right side of the
  // track, flips to right-align (card's right border == slice's right
  // border). If neither fits (track narrower than card), clamps to 0.
  function cardLeftPx(rect: Rect): number {
    if (containerWidth === 0) return 0;
    const sliceLeftPx = (rect.leftPct / 100) * containerWidth;
    const sliceRightPx = ((rect.leftPct + rect.widthPct) / 100) * containerWidth;
    if (sliceLeftPx + CARD_WIDTH_PX <= containerWidth) {
      return sliceLeftPx;
    }
    return Math.max(0, sliceRightPx - CARD_WIDTH_PX);
  }
</script>

<div class="bar">
  <div
    class="track"
    bind:this={trackEl}
    onmousemove={onMouseMove}
    onmouseleave={onMouseLeave}
    onclick={onTrackClick}
    role="presentation"
  >
    {#if selectedRect !== null}
      <div
        class="slice selected"
        style:left="{selectedRect.leftPct}%"
        style:width="{selectedRect.widthPct}%"
      ></div>
    {/if}
    {#each previewCards as span (n(span.id))}
      {@const rect = previewRect(span)}
      <div
        class="slice preview"
        class:focused={vs.timelineCardFocus === n(span.id)}
        style:left="{rect.leftPct}%"
        style:width="{rect.widthPct}%"
      ></div>
    {/each}
    {#if selectedHover !== null && selectedRect !== null}
      <div class="floating-card" style:left="{cardLeftPx(selectedRect)}px">
        <TimelinePreviewCard mode={selectedHover} range={vs.timeRange} />
      </div>
    {/if}
    {#if previewCards.length > 0 && selectedHover === null}
      <div class="floating-cards" data-timeline-card-stack="true">
        {#each previewCards as span (n(span.id))}
          {@const rect = previewRect(span)}
          <div class="floating-card" style:left="{cardLeftPx(rect)}px">
            <TimelinePreviewCard
              mode={{ kind: 'span', span }}
              range={vs.timeRange}
              focused={vs.timelineCardFocus === n(span.id)}
              onclick={() => onCardClick(span)}
              onmouseenter={() => (vs.timelineCardFocus = n(span.id))}
              onmouseleave={() => (vs.timelineCardFocus = null)}
            />
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .bar {
    flex: 0 0 auto;
    padding: 4px var(--gap-md);
    border-bottom: 1px solid var(--border);
    background: var(--bg);
    position: relative;
  }
  .track {
    position: relative;
    height: 22px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: transparent;
    cursor: pointer;
  }
  .slice {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    pointer-events: none;
  }
  .slice.selected {
    background: var(--accent);
    /* Quadratic ease-in-out, alternating between 30% and 85% opacity.
       Same cadence as the source-view span frame and shell buffer's
       matched-pulse. */
    animation: timeline-slice-pulse 0.8s cubic-bezier(0.45, 0, 0.55, 1) infinite alternate;
  }
  @keyframes timeline-slice-pulse {
    from {
      opacity: 0.3;
    }
    to {
      opacity: 0.85;
    }
  }
  .slice.preview {
    border: 1px dashed var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .slice.preview.focused {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 24%, transparent);
  }
  .floating-cards {
    pointer-events: none;
  }
  .floating-card {
    position: absolute;
    top: 100%;
    margin-top: 6px;
    z-index: 10;
    pointer-events: auto;
  }
</style>
