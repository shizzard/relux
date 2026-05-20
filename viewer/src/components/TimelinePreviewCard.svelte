<script lang="ts">
  import type { Span } from '../types/Span';
  import type { FoldedEvent } from '../lib/flatten';
  import { leadEvent } from '../lib/flatten';
  import {
    displaySpanCallKind,
    foldedFamily,
    foldedGlyph,
    foldedKindLabel,
    foldedSummary,
    formatDuration,
    formatTimestamp,
    spanTitle,
    type KindFamily,
  } from '../lib/format';
  import type { TimeRange } from '../lib/timeline';

  type Mode = { kind: 'span'; span: Span } | { kind: 'event'; folded: FoldedEvent };

  let {
    mode,
    range,
    onclick,
    onmouseenter,
    onmouseleave,
    focused = false,
  }: {
    mode: Mode;
    range: TimeRange;
    onclick?: () => void;
    onmouseenter?: () => void;
    onmouseleave?: () => void;
    focused?: boolean;
  } = $props();

  type Timing = { startMs: number; endMs: number; durationMs: number };

  const timing = $derived<Timing>(computeTiming());

  function computeTiming(): Timing {
    if (mode.kind === 'span') {
      const start = mode.span.start_ts - range.start;
      const endAbs = mode.span.end_ts ?? range.end;
      const end = endAbs - range.start;
      return { startMs: start, endMs: end, durationMs: end - start };
    }
    const lead = leadEvent(mode.folded);
    let endAbs: number;
    switch (mode.folded.kind) {
      case 'single':
        endAbs = lead.ts;
        break;
      case 'sleep':
        endAbs = mode.folded.done.ts;
        break;
      case 'match':
        endAbs = mode.folded.outcome.ts;
        break;
    }
    const start = lead.ts - range.start;
    const end = endAbs - range.start;
    return { startMs: start, endMs: end, durationMs: end - start };
  }

  const header = $derived(buildHeader());

  type Header = {
    kind: string;
    title: string;
    glyph: string | null;
    family: KindFamily | null;
  };

  function buildHeader(): Header {
    if (mode.kind === 'span') {
      return {
        kind: displaySpanCallKind(mode.span),
        title: spanTitle(mode.span),
        glyph: null,
        family: null,
      };
    }
    return {
      kind: foldedKindLabel(mode.folded),
      title: foldedSummary(mode.folded),
      glyph: foldedGlyph(mode.folded),
      family: foldedFamily(mode.folded),
    };
  }

  const clickable = $derived(onclick !== undefined);

  const ARROW = '\u{2192}';
  const MIDDOT = '\u{00B7}';
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="card"
  class:clickable
  class:focused
  onclick={onclick
    ? (e: MouseEvent) => {
        // Stop the click from bubbling to the parent `.track`'s
        // onclick. Without this the bubbled handler re-runs
        // `candidateSpansAt` at the same cursor position, finds the
        // same multi-candidate set, and re-pins the timeline — the
        // selection set by `onclick` is correct, but the cards never
        // close and the user can't tell the click landed.
        e.stopPropagation();
        onclick();
      }
    : undefined}
  onmouseenter={onmouseenter}
  onmouseleave={onmouseleave}
  role={clickable ? 'button' : undefined}
  tabindex={clickable ? 0 : undefined}
  onkeydown={clickable
    ? (e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onclick?.();
        }
      }
    : undefined}
>
  <div class="head">
    {#if header.glyph !== null}
      <span class="glyph {header.family}" aria-hidden="true">{header.glyph}</span>
    {/if}
    <span class="kind">{header.kind}</span>
    <span class="title">{header.title}</span>
  </div>
  <div class="timing">
    <span class="mono">{formatTimestamp(timing.startMs)}</span>
    <span class="arrow">{ARROW}</span>
    <span class="mono">{formatTimestamp(timing.endMs)}</span>
    <span class="middot">{MIDDOT}</span>
    <span class="mono">{formatDuration(timing.durationMs)}</span>
  </div>
</div>

<style>
  .card {
    background: var(--paper);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--gap-xs) var(--gap-sm);
    width: 300px;
    font-size: 0.82rem;
    line-height: 1.4;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.08);
    transition: border-color 80ms ease;
  }
  .card.clickable {
    cursor: pointer;
  }
  .card.clickable:hover,
  .card.focused {
    border-color: var(--accent);
  }
  .head {
    display: flex;
    gap: var(--gap-sm);
    align-items: baseline;
    margin-bottom: 2px;
  }
  .glyph {
    width: 16px;
    text-align: center;
    color: var(--ink-faint);
    font-family: var(--font-mono);
    flex: 0 0 auto;
  }
  .glyph.ok {
    color: var(--accent-2);
  }
  .glyph.danger {
    color: var(--danger);
  }
  .glyph.info {
    color: var(--ink-dim);
  }
  .kind {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--ink-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    flex: 0 0 auto;
  }
  .title {
    font-family: var(--font-mono);
    color: var(--ink);
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .timing {
    display: flex;
    gap: 4px;
    align-items: baseline;
    color: var(--ink-dim);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .arrow,
  .middot {
    color: var(--ink-faint);
  }
</style>
