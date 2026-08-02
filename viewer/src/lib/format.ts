import type { CancelReasonRecord } from '../types/CancelReasonRecord';
import type { Event } from '../types/Event';
import type { MatchContext } from '../types/MatchContext';
import type { MultiMatchPattern } from '../types/MultiMatchPattern';
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
  'pure-match-start': '\u{003F}',
  'pure-match-done': '\u{2713}',
  'pure-match-failed': '\u{2717}',
  'var-read': '\u{2261}',
  'bool-check': '\u{2714}',
  annotate: '\u{266B}',
  log: '\u{00B7}',
  warning: '\u{0021}',
  error: '\u{2717}',
  cancelled: '\u{23F9}',
  'multi-match-start': '\u{29C9}',
  'multi-match-pattern-done': '\u{2713}',
  'multi-match-done': '\u{29C9}\u{2713}',
  'multi-match-timeout': '\u{29C9}\u{23F1}',
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
  cancelled: 'danger',
  log: 'info',
  warning: 'info',
  annotate: 'info',
  'sleep-start': 'info',
  'sleep-done': 'info',
  'multi-match-pattern-done': 'ok',
  'multi-match-done': 'ok',
  'multi-match-timeout': 'danger',
  'pure-match-done': 'ok',
  'pure-match-failed': 'danger',
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

// Card-row variant: includes the source location (or `default` when absent).
//   tolerance, multiplier 1.0  -> '5s (foo.relux:12)'
//   tolerance, multiplier 1.5  -> '5s \u{00D7} 1.5 = 7.5s (foo.relux:12)'
//   assertion                  -> '5s (foo.relux:12)'
//   no source                  -> '... (default)'
export function formatTimeoutLine(t: TimeoutValue): string {
  const src = t.source !== null ? `${t.source.file}:${t.source.line}` : 'default';
  if (t.type === 'tolerance' && t.multiplier !== '1.0') {
    return `${t.duration} \u{00D7} ${t.multiplier} = ${t.total_duration} (${src})`;
  }
  return `${t.duration} (${src})`;
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
    case 'pure-match-start':
      return `${truncate(event.pattern, SUMMARY_MAX)}`;
    case 'pure-match-done':
      return `${truncate(escapeBytes(event.matched), SUMMARY_MAX)}`;
    case 'pure-match-failed':
      return '(no match)';
    case 'var-read':
      return `${event.name} = ${truncate(escapeBytes(event.value), SUMMARY_MAX)}`;
    case 'bool-check': {
      const ev = event.evaluation;
      switch (ev.shape) {
        case 'unconditional':
          return 'unconditional';
        case 'bare':
          return `"${truncate(escapeBytes(ev.value), SUMMARY_MAX)}" \u{2192} ${ev.met}`;
        case 'pure-match':
          return ev.is_regex
            ? `"${truncate(escapeBytes(ev.value), 32)}" ? ${truncate(ev.pattern, 32)} \u{2192} ${ev.met}`
            : `"${truncate(escapeBytes(ev.value), 32)}" = ${truncate(ev.pattern, 32)} \u{2192} ${ev.met}`;
      }
    }
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
    case 'cancelled':
      return cancelReasonSummary(event.reason);
    case 'multi-match-start':
      return `${event.patterns.length} patterns (\u{2264} ${formatTimeout(event.effective)})`;
    case 'multi-match-pattern-done':
      return `#${event.index} \u{2192} ${formatDuration(event.elapsed)}`;
    case 'multi-match-done':
      return 'all patterns matched';
    case 'multi-match-timeout':
      return `${event.unmatched.length} pattern${event.unmatched.length === 1 ? '' : 's'} unmatched`;
  }
}

export function cancelReasonSummary(reason: CancelReasonRecord): string {
  switch (reason.type) {
    case 'test-timeout':
      return `test-timeout (duration ${reason.duration_ms}ms)`;
    case 'suite-timeout':
      return `suite-timeout (duration ${reason.duration_ms}ms)`;
    case 'fail-fast':
      return `fail-fast (triggered by ${reason.trigger_test})`;
    case 'sigint':
      return 'sigint';
  }
}

// Folded helpers - pair-aware variants used by the timeline rows. For
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
    case 'pure-match':
      return kindGlyph(f.outcome.kind);
  }
}

// Viewer-side display label for an event kind. Most kinds render their
// schema string verbatim; entries here override that for readability.
// Per-pattern multimatch completions read as `match` so they look the
// same as a folded single-shell match-start/match-done pair.
const EVENT_KIND_LABELS: Partial<Record<Event['kind'], string>> = {
  'multi-match-pattern-done': 'match',
};

export function foldedKindLabel(f: FoldedEvent): string {
  switch (f.kind) {
    case 'single':
      return EVENT_KIND_LABELS[f.event.kind] ?? f.event.kind;
    case 'sleep':
      return 'sleep';
    case 'match':
      return 'match';
    case 'pure-match':
      return 'pure-match';
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
    case 'pure-match':
      return kindFamily(f.outcome.kind);
  }
}

export function foldedSummary(f: FoldedEvent): string {
  switch (f.kind) {
    case 'single':
      return eventSummary(f.event);
    case 'sleep':
      return f.start.kind === 'sleep-start' ? formatDuration(f.start.duration) : '';
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
    case 'pure-match': {
      const start = f.start;
      const outcome = f.outcome;
      if (start.kind !== 'pure-match-start') return '';
      const op = start.is_regex ? '?' : '=';
      const value = truncate(escapeBytes(start.value), 40);
      const pat = truncate(start.pattern, 40);
      const head = `"${value}" ${op} ${pat}`;
      if (outcome.kind === 'pure-match-done') {
        return `${head} \u{2192} ${truncate(escapeBytes(outcome.matched), 40)}`;
      }
      return `${head} (no match)`;
    }
  }
}

// Viewer-side display label for `span.kind`. The schema strings
// (`effect-setup`, `shell-block`, `fn-call`, `effect-cleanup`) are
// implementation-leaning; the viewer surfaces shorter, DSL-aligned
// terms in the kind badge and card title.
const SPAN_KIND_LABELS: Partial<Record<Span['kind'], string>> = {
  'effect-setup': 'setup',
  'effect-cleanup': 'cleanup',
  'shell-block': 'shell',
  'fn-call': 'call',
  markers: 'MARKERS',
  'marker-eval': 'marker',
  'multi-match': 'multimatch',
};

export function displaySpanKind(kind: Span['kind']): string {
  return SPAN_KIND_LABELS[kind] ?? kind;
}

// Span-aware variant: BIF fn-call spans render as `BIF` instead of `call`
// so the viewer's kind badge / card title distinguish built-ins from
// user-defined function calls. Falls back to `displaySpanKind` for every
// other span kind.
export function displaySpanCallKind(span: Span): string {
  if (span.kind === 'fn-call' && span.callee_kind === 'bif') {
    return 'BIF';
  }
  return displaySpanKind(span.kind);
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
    case 'multi-match':
      return span.shell;
    case 'cleanup-block':
      return 'cleanup';
    case 'fn-call': {
      const head = `${span.name}/${span.args.length}`;
      if (span.result === null) return head;
      return `${head} \u{2192} "${escapeBytes(span.result)}"`;
    }
    case 'markers':
      return '';
    case 'marker-eval':
      return `${displayMarkerKind(span.marker_kind)} ${displayMarkerModifier(span.modifier)} \u2192 ${displayMarkerDecision(span.decision)}`;
  }
}

export function displayMarkerKind(k: 'skip' | 'run' | 'flaky'): string {
  return `#${k}`;
}

export function displayMarkerModifier(m: 'if' | 'unless'): string {
  return m;
}

export function displayMarkerDecision(d: 'pass' | 'mark'): string {
  return d;
}

// Per-pattern label, matching the surface syntax: `? <pattern>` (regex)
// or `= <pattern>` (literal). Used by the per-pattern table in
// SelectionCard and the failure-detail surface.
export function formatMultiMatchPatternLabel(p: MultiMatchPattern): string {
  const op = p.is_regex ? '?' : '=';
  return `${op} ${p.pattern}`;
}

// Human label for where a pure match ran (fn / test preamble / effect
// preamble / shell), used on the `pure-match-failed` detail row when that
// event is the test's recorded failure. Mirrors the Rust
// `MatchContext::Display` impl ("<kind> '<name>'") so the viewer and the
// CLI/TAP reporters agree on wording.
const MATCH_CONTEXT_KIND_LABELS: Record<MatchContext['type'], string> = {
  fn: 'fn',
  'test-preamble': 'test preamble',
  'effect-preamble': 'effect preamble',
  shell: 'shell',
};

export function formatMatchContext(mc: MatchContext): string {
  return `${MATCH_CONTEXT_KIND_LABELS[mc.type]} '${mc.name}'`;
}
