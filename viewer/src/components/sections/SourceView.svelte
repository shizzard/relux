<script lang="ts">
  import { tick } from 'svelte';
  import type { ViewerState } from '../../lib/state.svelte';
  import { selectionSourceRange } from '../../lib/derive';
  import { utf8ByteToChar } from '../../lib/source_highlight';

  // Rename `state` -> `view` locally so the `$state(...)` rune below
  // doesn't collide with the Svelte 5 auto-store-subscription syntax
  // for a prop literally named `state`.
  let { state: view }: { state: ViewerState } = $props();

  const range = $derived(
    selectionSourceRange(view.data, view.selectedSpanId, view.selectedEventSeq),
  );
  const source = $derived(range ? view.data.sources[range.file] ?? null : null);

  type FileCache = { lineCharStarts: number[]; lines: string[] };
  const cache: Map<string, FileCache> = new Map();

  // Split the highlighted HTML into one self-contained line per
  // entry. As the walker meets `\n` it closes every currently-open
  // hljs span, finalizes the line, then re-opens those same spans on
  // the next line — so each rendered line stays a valid HTML fragment.
  // `lineCharStarts[i]` is the JS-char index of line `i`'s first char
  // in the **source** string, computed from `src` itself rather than
  // counted off the html (where entity expansion confuses things).
  function buildCache(file: string, src: string): FileCache {
    const hit = cache.get(file);
    if (hit) return hit;
    const html = window.hljs.highlight(src, { language: 'relux' }).value;
    const lines: string[] = [];
    const openTags: string[] = [];
    let cur = '';
    let i = 0;
    while (i < html.length) {
      if (html[i] === '<') {
        const tagEnd = html.indexOf('>', i);
        if (tagEnd === -1) {
          cur += html.slice(i);
          break;
        }
        const tag = html.slice(i, tagEnd + 1);
        cur += tag;
        if (tag.startsWith('</')) {
          openTags.pop();
        } else if (!tag.startsWith('<!') && !tag.endsWith('/>')) {
          openTags.push(tag);
        }
        i = tagEnd + 1;
        continue;
      }
      if (html[i] === '\n') {
        for (let j = openTags.length - 1; j >= 0; j--) cur += '</span>';
        lines.push(cur);
        cur = openTags.join('');
        i++;
        continue;
      }
      cur += html[i]!;
      i++;
    }
    for (let j = openTags.length - 1; j >= 0; j--) cur += '</span>';
    lines.push(cur);

    const lineCharStarts: number[] = [0];
    for (let c = 0; c < src.length; c++) {
      if (src[c] === '\n') lineCharStarts.push(c + 1);
    }
    const entry: FileCache = { lineCharStarts, lines };
    cache.set(file, entry);
    return entry;
  }

  type RenderedLine = { num: number; html: string; anchor: boolean };

  const rendered = $derived.by<{ lines: RenderedLine[] } | null>(() => {
    if (!range || source === null) return null;
    const { lineCharStarts, lines } = buildCache(range.file, source);
    const fileChars = source.length;
    const startChar = utf8ByteToChar(source, range.start);
    const out: RenderedLine[] = [];
    for (let li = 0; li < lines.length; li++) {
      const lineStart = lineCharStarts[li]!;
      const lineEnd =
        li + 1 < lineCharStarts.length ? lineCharStarts[li + 1]! - 1 : fileChars;
      const anchor = startChar >= lineStart && startChar <= lineEnd;
      out.push({ num: li + 1, html: lines[li]!, anchor });
    }
    return { lines: out };
  });

  let preEl = $state<HTMLPreElement | undefined>(undefined);
  let contentEl = $state<HTMLDivElement | undefined>(undefined);

  type Frame = { top: number; left: number; width: number; height: number };
  let frames = $state<Frame[]>([]);

  // Walk text nodes under `codeEl` until `offset` source bytes have
  // been consumed, return the (text node, offset-within-node) pair.
  // Text node `.data.length` equals source-byte length for ASCII
  // source (entities are already decoded by the browser).
  function locateInLine(codeEl: Element, offset: number): [Node, number] {
    const walker = document.createTreeWalker(codeEl, NodeFilter.SHOW_TEXT);
    let n = walker.nextNode();
    let last: Node = codeEl;
    let lastLen = 0;
    let remaining = offset;
    while (n) {
      const len = n.nodeValue?.length ?? 0;
      if (remaining <= len) return [n, remaining];
      remaining -= len;
      last = n;
      lastLen = len;
      n = walker.nextNode();
    }
    return [last, lastLen];
  }

  $effect(() => {
    void rendered;
    void range;
    if (!preEl || !contentEl) {
      frames = [];
      return;
    }
    const pre = preEl;
    const content = contentEl;
    void (async () => {
      await tick();
      const half = pre.clientHeight / 2;
      pre.style.paddingBottom = `${half}px`;

      if (!range || source === null) {
        frames = [];
      } else {
        const { lineCharStarts } = buildCache(range.file, source);
        const fileChars = source.length;
        const startChar = Math.min(utf8ByteToChar(source, range.start), fileChars);
        const endChar = Math.min(utf8ByteToChar(source, range.end), fileChars);
        // find start and end line indices in JS-char space
        let startLineIdx = 0;
        while (
          startLineIdx + 1 < lineCharStarts.length &&
          lineCharStarts[startLineIdx + 1]! <= startChar
        )
          startLineIdx++;
        let endLineIdx = startLineIdx;
        while (
          endLineIdx + 1 < lineCharStarts.length &&
          lineCharStarts[endLineIdx + 1]! <= endChar
        )
          endLineIdx++;

        const startCodeEl = content.querySelector(
          `[data-line="${startLineIdx + 1}"] .code`,
        );
        const endCodeEl = content.querySelector(
          `[data-line="${endLineIdx + 1}"] .code`,
        );
        if (startCodeEl && endCodeEl) {
          const startLineStart = lineCharStarts[startLineIdx]!;
          const endLineStart = lineCharStarts[endLineIdx]!;
          const [startNode, startOff] = locateInLine(
            startCodeEl,
            Math.max(0, startChar - startLineStart),
          );
          const [endNode, endOff] = locateInLine(
            endCodeEl,
            Math.max(0, endChar - endLineStart),
          );

          try {
            const domRange = document.createRange();
            domRange.setStart(startNode, startOff);
            domRange.setEnd(endNode, endOff);
            const rects = Array.from(domRange.getClientRects()).filter(
              (r) => r.width !== 0 || r.height !== 0,
            );
            // `getClientRects()` returns one rect per inline box inside
            // the range — so a `foo(a, b)` Range yields rects for `foo`,
            // for `(a, b)`, AND for each `<span class="hljs-...">` child
            // inside. The inner rects are visually contained by the
            // outer ones; drop any rect that another rect fully covers.
            const TOL = 1;
            const dedup = rects.filter(
              (r, i) =>
                !rects.some(
                  (other, j) =>
                    j !== i &&
                    other.left - TOL <= r.left &&
                    other.top - TOL <= r.top &&
                    other.right + TOL >= r.right &&
                    other.bottom + TOL >= r.bottom &&
                    // tie-breaker: when two rects are identical, keep the
                    // one with the lower index, drop the higher.
                    !(
                      other.left === r.left &&
                      other.top === r.top &&
                      other.right === r.right &&
                      other.bottom === r.bottom &&
                      j > i
                    ),
                ),
            );
            const contentRect = content.getBoundingClientRect();
            const out: Frame[] = dedup.map((rect) => ({
              top: rect.top - contentRect.top,
              left: rect.left - contentRect.left,
              width: Math.max(rect.width, 2),
              height: rect.height,
            }));
            frames = out;
          } catch {
            frames = [];
          }
        } else {
          frames = [];
        }
      }

      const anchor = pre.querySelector<HTMLElement>('[data-anchor]');
      const target = anchor ? anchor.offsetTop + anchor.offsetHeight / 2 - half : 0;
      const max = pre.scrollHeight - pre.clientHeight;
      pre.scrollTop = Math.max(0, Math.min(max, target));

      // Horizontal: if the (first) frame isn't fully on-screen, scroll
      // the pre horizontally to center it. Same rationale as the
      // vertical centering: spans deep into long lines (e.g. a
      // parenthesized arg list past column 120) would otherwise sit
      // off-screen.
      if (frames.length > 0) {
        const frame = frames[0]!;
        const halfW = pre.clientWidth / 2;
        const visibleLeft = pre.scrollLeft;
        const visibleRight = pre.scrollLeft + pre.clientWidth;
        if (frame.left < visibleLeft || frame.left + frame.width > visibleRight) {
          const targetLeft = frame.left + frame.width / 2 - halfW;
          const maxLeft = Math.max(0, pre.scrollWidth - pre.clientWidth);
          pre.scrollLeft = Math.max(0, Math.min(maxLeft, targetLeft));
        }
      }
    })();
  });
</script>

{#if range === null}
  <p class="placeholder">no source location for this selection.</p>
{:else if source === null}
  <p class="placeholder">source bytes not shipped for <code>{range.file}</code>.</p>
{:else}
  <pre bind:this={preEl} class="source"><div class="content" bind:this={contentEl}
      >{#each rendered?.lines ?? [] as line (line.num)}<div
          class="line"
          data-line={line.num}
          data-anchor={line.anchor ? '' : undefined}
        ><span class="gutter">{line.num}</span><span class="code"
          >{@html line.html}</span
        ></div
      >{/each}{#key frames}{#each frames as f, i (i)}<div
            class="span-frame"
            style:top="{f.top}px"
            style:left="{f.left}px"
            style:width="{f.width}px"
            style:height="{f.height}px"
          ></div>{/each}{/key}</div></pre>
{/if}

<style>
  .placeholder {
    margin: var(--gap-sm) var(--gap-md);
    color: var(--ink-faint);
    font-style: italic;
    font-size: 0.85rem;
  }
  .source {
    flex: 1 1 0;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: var(--gap-sm) 0;
    font-family: var(--font-mono);
    font-size: 0.82rem;
    line-height: 1.4;
    background: var(--paper);
    color: var(--ink);
  }
  .content {
    position: relative;
  }
  .line {
    display: flex;
    white-space: pre;
  }
  .gutter {
    /* `box-sizing: content-box` overrides the global border-box so
       `min-width: 4ch` truly reserves 4 chars of *content* width —
       otherwise the 8px side padding eats into it and 3-digit line
       numbers force the gutter to grow, shifting the code right at
       line 100. */
    box-sizing: content-box;
    flex: 0 0 auto;
    min-width: 4ch;
    padding: 0 var(--gap-sm);
    text-align: right;
    color: var(--ink-faint);
    user-select: none;
    border-right: 1px solid var(--border);
    margin-right: var(--gap-sm);
  }
  .code {
    flex: 1 1 auto;
    min-width: 0;
  }
  .span-frame {
    position: absolute;
    background: var(--accent);
    pointer-events: none;
    /* Quadratic ease-in-out, alternating between 5% and 30% opacity. */
    animation: span-frame-pulse 0.8s cubic-bezier(0.45, 0, 0.55, 1) infinite alternate;
  }
  @keyframes span-frame-pulse {
    from {
      opacity: 0.05;
    }
    to {
      opacity: 0.3;
    }
  }
  .code :global(.hljs-keyword) {
    color: var(--accent);
    font-weight: 700;
  }
  .code :global(.hljs-duration) {
    font-weight: 400;
  }
  .code :global(.hljs-variable),
  .code :global(.hljs-subst) {
    color: var(--info);
  }
  .code :global(.hljs-type) {
    color: var(--accent-3);
    font-weight: 700;
  }
  .code :global(.hljs-title),
  .code :global(.hljs-built_in) {
    color: var(--accent-3);
  }
  .code :global(.hljs-string),
  .code :global(.hljs-number) {
    color: var(--accent-2);
  }
  .code :global(.hljs-comment) {
    color: var(--ink-faint);
    font-style: italic;
  }
</style>
