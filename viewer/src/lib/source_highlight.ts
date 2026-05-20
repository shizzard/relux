const MARK_OPEN = '<mark class="span-frame">';
const MARK_CLOSE = '</mark>';

/**
 * Wrap bytes [start, end) of the highlighted HTML in `<mark
 * class="span-frame">`. Walks the HTML linearly, tracking byte offsets
 * through text nodes only — HTML tags are pass-through and don't
 * advance the offset.
 *
 * The wrap is closed and reopened around each `\n` so multi-line
 * ranges don't span line boundaries (line-by-line rendering wraps
 * each line independently and a mark crossing a `</div>\n<div>`
 * would be invalid). Existing `<span class="hljs-...">` containers
 * are split when the range starts or ends inside them: the wrap
 * closes before the container's closing tag and reopens after the
 * container's opening tag on the other side, so the syntax class
 * coverage stays intact.
 *
 * Byte offsets reference the *decoded source* fed to
 * `hljs.highlight()`. hljs v11 escapes `&`, `<`, `>`, `"`, `'` in
 * text nodes, so we detect `&...;` entities and count each as a
 * single source byte (matches the original ASCII char).
 */
const ENTITY_RE = /^&[a-zA-Z#0-9]+;/;

export function wrapByteRange(html: string, start: number, end: number): string {
  if (end < start) return html;
  let out = '';
  let byteIdx = 0;
  let i = 0;
  let inMark = false;

  const open = (): void => {
    if (!inMark) {
      out += MARK_OPEN;
      inMark = true;
    }
  };
  const close = (): void => {
    if (inMark) {
      out += MARK_CLOSE;
      inMark = false;
    }
  };
  // Emit an empty mark exactly once for zero-width ranges. Guarded so
  // we don't double-emit when the loop later reaches `end`.
  let zeroWidthEmitted = false;
  const tryEmitZeroWidth = (): void => {
    if (!zeroWidthEmitted && start === end && byteIdx === start) {
      out += MARK_OPEN + MARK_CLOSE;
      zeroWidthEmitted = true;
    }
  };

  while (i < html.length) {
    if (html[i] === '<') {
      const tagEnd = html.indexOf('>', i);
      if (tagEnd === -1) {
        out += html.slice(i);
        break;
      }
      const wasInMark = inMark;
      close();
      out += html.slice(i, tagEnd + 1);
      // Reopen only if the range hasn't ended yet — otherwise we'd
      // emit a stray `<mark></mark>` between the tag and the next
      // text byte.
      if (wasInMark && byteIdx < end) open();
      i = tagEnd + 1;
      continue;
    }
    tryEmitZeroWidth();
    if (byteIdx === start && byteIdx < end) open();
    if (byteIdx === end) close();
    if (html[i] === '&') {
      const m = html.slice(i, i + 12).match(ENTITY_RE);
      if (m) {
        out += m[0];
        byteIdx++;
        i += m[0].length;
        continue;
      }
    }
    const ch = html[i]!;
    if (ch === '\n' && inMark) {
      close();
      out += ch;
      byteIdx++;
      i++;
      if (byteIdx < end) open();
      continue;
    }
    out += ch;
    byteIdx++;
    i++;
  }
  // Zero-width range falling on or past end-of-input.
  tryEmitZeroWidth();
  close();
  return out;
}

/**
 * Convert a UTF-8 byte offset (as emitted by Rust's `Span`) into a JS
 * char offset (UTF-16 code unit index) within `src`. Source ranges in
 * `events.json` are UTF-8 byte offsets, but the DOM Range API and
 * `node.nodeValue.length` use UTF-16 code units, so any non-ASCII
 * character (em dash, smart quote, accented letter, emoji) makes the
 * two coordinate systems drift apart past it.
 *
 * If `byteOff` is past the end of `src`, returns `src.length`.
 */
export function utf8ByteToChar(src: string, byteOff: number): number {
  let bytes = 0;
  let chars = 0;
  const n = src.length;
  while (chars < n && bytes < byteOff) {
    const code = src.charCodeAt(chars);
    if (code < 0x80) {
      bytes += 1;
      chars += 1;
    } else if (code < 0x800) {
      bytes += 2;
      chars += 1;
    } else if (code >= 0xd800 && code <= 0xdbff) {
      // High surrogate paired with low surrogate: 4 UTF-8 bytes,
      // 2 JS code units.
      bytes += 4;
      chars += 2;
    } else {
      bytes += 3;
      chars += 1;
    }
  }
  return chars;
}
