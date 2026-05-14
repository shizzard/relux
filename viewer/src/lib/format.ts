import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { TimeoutValue } from '../types/TimeoutValue';
import type { FoldedEvent } from './flatten';

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

// Buffer rendering version of escapeBytes. Designed for `<pre>` blocks
// where the browser handles whitespace natively: CR is stripped (terminals
// emit CRLF; the LF alone is enough to break a line), LF and TAB pass
// through, other non-printable bytes still escape as `\xNN`.
export function escapeBufferBytes(s: string): string {
  let out = '';
  for (const ch of s) {
    const code = ch.charCodeAt(0);
    if (ch === CR) continue;
    if (ch === LF || ch === TAB) out += ch;
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

export type KindFamily = 'event' | 'ok' | 'danger' | 'info';

const KIND_FAMILY: Partial<Record<Event['kind'], KindFamily>> = {
  send: 'ok',
  'match-done': 'ok',
  'shell-spawn': 'ok',
  'shell-ready': 'ok',
  timeout: 'danger',
  'fail-pattern-triggered': 'danger',
  error: 'danger',
  log: 'info',
  warning: 'info',
  annotate: 'info',
  'sleep-start': 'info',
  'sleep-done': 'info',
};

export function kindFamily(kind: Event['kind']): KindFamily {
  return KIND_FAMILY[kind] ?? 'event';
}

const UNITS = ['B', 'KB', 'MB', 'GB'];

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${UNITS[unit]}`;
}

// Compact one-line display for a structured `TimeoutValue`.
//   tolerance, multiplier 1.0  -> '5s'
//   tolerance, multiplier 1.5  -> '5s \u{00D7} 1.5'  (mid-dot is multiplication)
//   assertion                  -> '5s exact'
export function formatTimeout(t: TimeoutValue): string {
  if (t.type === 'assertion') return `${t.duration} exact`;
  if (t.multiplier === '1.0') return t.duration;
  return `${t.duration} \u{00D7} ${t.multiplier}`;
}

export function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return s.slice(0, n - 1) + '\u{2026}';
}

const SUMMARY_MAX = 80;

export function eventSummary(event: Event): string {
  switch (event.kind) {
    case 'send':
    case 'recv':
      return truncate(escapeBytes(event.data), SUMMARY_MAX);
    case 'match-start':
      return `${event.is_regex ? 'regex' : 'literal'} ${truncate(event.pattern, SUMMARY_MAX)} (\u{2264} ${formatTimeout(event.effective)})`;
    case 'match-done':
      return `${formatDuration(event.elapsed)} ${truncate(escapeBytes(event.matched), SUMMARY_MAX)}`;
    case 'timeout':
      return `${truncate(event.pattern, SUMMARY_MAX)} after ${formatTimeout(event.effective)}`;
    case 'fail-pattern-set':
      return truncate(event.pattern, SUMMARY_MAX);
    case 'fail-pattern-cleared':
      return '';
    case 'fail-pattern-triggered':
      return truncate(event.pattern, SUMMARY_MAX);
    case 'sleep-start':
      return formatDuration(event.duration);
    case 'sleep-done':
      return '';
    case 'timeout-set':
      return `${formatTimeout(event.previous)} \u{2192} ${formatTimeout(event.timeout)}`;
    case 'var-let':
    case 'var-assign':
      return `${event.name} = ${truncate(escapeBytes(event.value), SUMMARY_MAX)}`;
    case 'string-eval':
      return truncate(escapeBytes(event.result), SUMMARY_MAX);
    case 'interpolation':
      return truncate(escapeBytes(event.result), SUMMARY_MAX);
    case 'annotate':
      return truncate(event.text, SUMMARY_MAX);
    case 'log':
    case 'warning':
    case 'error':
      return truncate(event.message, SUMMARY_MAX);
    case 'shell-spawn':
      return `${event.name}: ${truncate(event.command, SUMMARY_MAX)}`;
    case 'shell-ready':
    case 'shell-switch':
    case 'shell-terminate':
      return event.name;
    case 'effect-expose-shell':
      return event.qualifier !== null
        ? `${event.name} \u{2190} ${event.qualifier}.${event.target}`
        : event.name;
    case 'effect-expose-var':
      return `${event.name} = ${truncate(escapeBytes(event.value), SUMMARY_MAX)}`;
  }
}

// Folded helpers — pair-aware variants used by the timeline rows. For
// single-event folds we delegate to the existing per-event helpers; for
// merged folds the glyph / family reflect the closing half (match outcome,
// spawn readiness) and the summary stitches the halves together.

export function foldedGlyph(f: FoldedEvent): string {
  switch (f.kind) {
    case 'single':
      return kindGlyph(f.event.kind);
    case 'sleep':
      return kindGlyph('sleep-start');
    case 'match':
      return kindGlyph(f.outcome.kind);
    case 'spawn':
      return kindGlyph('shell-spawn');
  }
}

export function foldedKindLabel(f: FoldedEvent): string {
  switch (f.kind) {
    case 'single':
      return f.event.kind;
    case 'sleep':
      return 'sleep';
    case 'match':
      return 'match';
    case 'spawn':
      return 'shell-spawn';
  }
}

export function foldedFamily(f: FoldedEvent): KindFamily {
  switch (f.kind) {
    case 'single':
      return kindFamily(f.event.kind);
    case 'sleep':
      return 'info';
    case 'match':
      return kindFamily(f.outcome.kind);
    case 'spawn':
      return 'ok';
  }
}

export function foldedSummary(f: FoldedEvent): string {
  switch (f.kind) {
    case 'single':
      return eventSummary(f.event);
    case 'sleep':
      return formatDuration(f.start.duration);
    case 'match': {
      const start = f.start;
      const outcome = f.outcome;
      if (start.kind !== 'match-start') return '';
      if (outcome.kind === 'match-done') {
        const pat = truncate(start.pattern, 40);
        const matched = truncate(escapeBytes(outcome.matched), 40);
        return `${pat} \u{2192} ${matched} (${formatDuration(outcome.elapsed)})`;
      }
      if (outcome.kind === 'timeout') {
        return `${truncate(start.pattern, SUMMARY_MAX)} timed out after ${formatTimeout(outcome.effective)}`;
      }
      return truncate(start.pattern, SUMMARY_MAX);
    }
    case 'spawn': {
      const spawn = f.spawn;
      if (spawn.kind !== 'shell-spawn') return '';
      return `${spawn.name}: ${truncate(spawn.command, SUMMARY_MAX)}`;
    }
  }
}

export function spanTitle(span: Span): string {
  switch (span.kind) {
    case 'test':
      return span.name;
    case 'effect-setup':
      return span.alias ? `${span.effect} as ${span.alias}` : span.effect;
    case 'effect-cleanup':
      return span.effect;
    case 'shell-block':
      return span.shell;
    case 'cleanup-block':
      return 'cleanup';
    case 'fn-call': {
      const head = `${span.name}/${span.args.length}`;
      if (span.result === null) return head;
      return `${head} \u{2192} "${escapeBytes(span.result)}"`;
    }
  }
}
