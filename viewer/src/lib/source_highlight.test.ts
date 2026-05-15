import { describe, expect, it } from 'vitest';
import { utf8ByteToChar, wrapByteRange } from './source_highlight';

describe('wrapByteRange', () => {
  it('wraps a single token entirely inside an existing hljs span', () => {
    const html = '<span class="hljs-keyword">let</span> X = "hi"';
    const out = wrapByteRange(html, 0, 3);
    expect(out).toBe(
      '<span class="hljs-keyword"><mark class="span-frame">let</mark></span> X = "hi"',
    );
  });

  it('wraps a range that straddles plain text and an hljs span', () => {
    const html = 'abc<span class="hljs-string">"de"</span>fg';
    // Bytes seen by the counter: a(0) b(1) c(2) "(3) d(4) e(5) "(6) f(7) g(8)
    // Range [2,5) covers `c"d` -- the wrap must close at the next-span
    // boundary and reopen inside the hljs-string container.
    const out = wrapByteRange(html, 2, 5);
    expect(out).toBe(
      'ab<mark class="span-frame">c</mark><span class="hljs-string"><mark class="span-frame">"d</mark>e"</span>fg',
    );
  });

  it('reopens the wrap across newline boundaries', () => {
    const html = 'a\nb\nc';
    const out = wrapByteRange(html, 0, 5);
    expect(out).toBe(
      '<mark class="span-frame">a</mark>\n<mark class="span-frame">b</mark>\n<mark class="span-frame">c</mark>',
    );
  });

  it('clamps end beyond input length', () => {
    expect(wrapByteRange('abc', 1, 99)).toBe('a<mark class="span-frame">bc</mark>');
  });

  it('zero-width range emits an empty mark', () => {
    expect(wrapByteRange('abc', 2, 2)).toBe('ab<mark class="span-frame"></mark>c');
  });

  it('range entirely outside text is a no-op', () => {
    expect(wrapByteRange('abc', 10, 20)).toBe('abc');
  });

  it('treats html entities as a single source byte', () => {
    // Source `>"a"` (4 bytes). hljs would escape it to `&gt;&quot;a&quot;`.
    // Range [0, 4) must wrap the whole escaped form.
    const html = '&gt;&quot;a&quot;';
    expect(wrapByteRange(html, 0, 4)).toBe(
      '<mark class="span-frame">&gt;&quot;a&quot;</mark>',
    );
    // Range [1, 3) wraps `"a` -- entity at position 1, plain `a` at 2.
    expect(wrapByteRange(html, 1, 3)).toBe(
      '&gt;<mark class="span-frame">&quot;a</mark>&quot;',
    );
  });
});

describe('utf8ByteToChar', () => {
  it('is identity for ASCII source', () => {
    const src = 'fn http_request() { }';
    expect(utf8ByteToChar(src, 0)).toBe(0);
    expect(utf8ByteToChar(src, 3)).toBe(3);
    expect(utf8ByteToChar(src, src.length)).toBe(src.length);
  });

  it('accounts for 3-byte BMP characters (em dash)', () => {
    // U+2014 (em dash) is 3 UTF-8 bytes but 1 JS code unit. This is
    // the exact regression that shifted edgescript.relux spans right
    // by 2.
    const src = 'a\u{2014}b';
    // Bytes: a(1) U+2014(3) b(1) = 5 UTF-8 bytes total.
    // Chars: a(1) U+2014(1) b(1) = 3 JS code units total.
    expect(utf8ByteToChar(src, 0)).toBe(0);
    expect(utf8ByteToChar(src, 1)).toBe(1); // start of em dash
    expect(utf8ByteToChar(src, 4)).toBe(2); // start of 'b'
    expect(utf8ByteToChar(src, 5)).toBe(3); // end of string
  });

  it('accounts for 2-byte characters (Latin-1 Supplement)', () => {
    // U+00E9 is 2 UTF-8 bytes, 1 JS code unit.
    const src = 'caf\u{00E9}';
    // Bytes: c(1) a(1) f(1) U+00E9(2) = 5 bytes
    // Chars: c(1) a(1) f(1) U+00E9(1) = 4 chars
    expect(utf8ByteToChar(src, 3)).toBe(3); // start of accented char
    expect(utf8ByteToChar(src, 5)).toBe(4); // end of string
  });

  it('accounts for surrogate pairs (4-byte code points)', () => {
    // U+1F600 is 4 UTF-8 bytes, 2 JS code units (high+low surrogate).
    const src = 'a\u{1F600}b';
    // Bytes: a(1) U+1F600(4) b(1) = 6 bytes
    // Chars: a(1) U+1F600(2) b(1) = 4 chars
    expect(utf8ByteToChar(src, 1)).toBe(1); // start of supplementary char
    expect(utf8ByteToChar(src, 5)).toBe(3); // start of 'b' (past both surrogates)
    expect(utf8ByteToChar(src, 6)).toBe(4); // end of string
  });

  it('clamps at end of string for out-of-range byte offsets', () => {
    expect(utf8ByteToChar('abc', 999)).toBe(3);
  });

  it('returns 0 for byte offset 0', () => {
    expect(utf8ByteToChar('abc', 0)).toBe(0);
    expect(utf8ByteToChar('', 0)).toBe(0);
  });
});
