import type { Event } from '../types/Event';

export function formatTimestamp(ms: number): string {
  if (ms < 1000) return `${ms.toFixed(0)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)}s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = ((ms % 60_000) / 1000).toFixed(0);
  return `${minutes}m ${seconds}s`;
}

export function formatDuration(ms: number): string {
  return formatTimestamp(ms);
}

const CR = '\u{000D}';
const LF = '\u{000A}';
const TAB = '\u{0009}';

export function escapeBytes(s: string): string {
  let out = '';
  for (const ch of s) {
    const code = ch.charCodeAt(0);
    if (ch === CR) out += '\\r';
    else if (ch === LF) out += '\\n\n';
    else if (ch === TAB) out += '\\t';
    else if (code < 0x20 || code === 0x7f) out += `\\x${code.toString(16).padStart(2, '0')}`;
    else out += ch;
  }
  return out;
}

const KIND_GLYPHS: Record<string, string> = {
  'shell-spawn': '\u{229E}',
  'shell-ready': '\u{2713}',
  'shell-switch': '\u{21C4}',
  'shell-terminate': '\u{2715}',
  'shell-alias': '\u{2261}',
  send: '\u{2192}',
  recv: '\u{2190}',
  'match-start': '\u{003F}',
  'match-done': '\u{2713}',
  timeout: '\u{23F1}',
  'fail-pattern-set': '\u{2691}',
  'fail-pattern-cleared': '\u{2690}',
  'fail-pattern-triggered': '\u{2691}',
  'sleep-start': '\u{23F8}',
  'sleep-done': '\u{25B6}',
  'timeout-set': '\u{23F1}',
  'var-let': '\u{003D}',
  'var-assign': '\u{003D}',
  'string-eval': '\u{0024}',
  interpolation: '\u{0024}',
  annotate: '\u{266B}',
  log: '\u{00B7}',
  warning: '\u{0021}',
  error: '\u{2717}',
};

export function kindGlyph(kind: Event['kind']): string {
  return KIND_GLYPHS[kind] ?? '\u{2022}';
}

export function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return s.slice(0, n - 1) + '\u{2026}';
}
