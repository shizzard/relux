<script lang="ts">
  import type { Event } from '../types/Event';
  import type { Span } from '../types/Span';
  import type { TimeoutValue } from '../types/TimeoutValue';
  import type { ViewerState } from '../lib/state.svelte';
  import { effectSetupProps, shellBlockProps, toNumber as n } from '../lib/derive';
  import {
    formatDuration,
    kindFamily,
  } from '../lib/format';
  import ValueCell from './ValueCell.svelte';

  type Mode = { kind: 'event'; event: Event } | { kind: 'span'; span: Span };

  let {
    state,
    mode,
  }: {
    state: ViewerState;
    mode: Mode;
  } = $props();

  type Row =
    | { type: 'kv'; key: string; value: string; mono?: boolean; accent?: boolean }
    | { type: 'subhead'; text: string };

  function timeoutValueRows(tv: TimeoutValue): Row[] {
    const out: Row[] = [{ type: 'kv', key: 'type', value: tv.type }];
    out.push({ type: 'kv', key: 'duration', value: tv.duration });
    if (tv.type === 'tolerance') {
      out.push({ type: 'kv', key: 'multiplier', value: tv.multiplier });
      out.push({ type: 'kv', key: 'total', value: tv.total_duration });
    }
    if (tv.source !== null) {
      out.push({
        type: 'kv',
        key: 'from',
        value: `${tv.source.file}:${tv.source.line}`,
        mono: true,
      });
    }
    return out;
  }

  const family = $derived(mode.kind === 'event' ? kindFamily(mode.event.kind) : 'event');
  const head = $derived(buildHead());
  const rows = $derived<Row[]>(mode.kind === 'event' ? eventRows(mode.event) : spanRows(mode.span));

  function buildHead(): { title: string; subtitle: string } {
    if (mode.kind === 'event') {
      const ev = mode.event;
      return { title: ev.kind, subtitle: buildEventSubtitle(ev) };
    }
    const span = mode.span;
    return { title: span.kind, subtitle: buildSpanSubtitle(span) };
  }

  function buildEventSubtitle(ev: Event): string {
    const parts: string[] = [];
    if (ev.shell !== null) parts.push(`shell ${ev.shell}`);
    parts.push(`t = ${formatDuration(ev.ts)}`);
    if (ev.kind === 'match-done') parts.push(`${formatDuration(ev.elapsed)} wait`);
    if (ev.kind === 'sleep-start') parts.push(`${formatDuration(ev.duration)}`);
    return parts.join(' \u00b7 ');
  }

  function buildSpanSubtitle(span: Span): string {
    const dur =
      span.end_ts !== null ? formatDuration(span.end_ts - span.start_ts) : '\u2014';
    const loc =
      span.location !== null ? ` \u00b7 ${span.location.file}:${span.location.line}` : '';
    return `${dur}${loc}`;
  }

  function eventRows(ev: Event): Row[] {
    switch (ev.kind) {
      case 'send':
      case 'recv':
        return [{ type: 'kv', key: 'data', value: ev.data, mono: true }];
      case 'match-start': {
        const out: Row[] = [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true, accent: true },
          { type: 'kv', key: 'is_regex', value: ev.is_regex ? 'regex' : 'literal' },
        ];
        out.push({ type: 'subhead', text: '\u2014 timeout' });
        out.push(...timeoutValueRows(ev.effective));
        return out;
      }
      case 'match-done': {
        const out: Row[] = [
          { type: 'kv', key: 'matched', value: ev.matched, mono: true, accent: true },
          { type: 'kv', key: 'elapsed', value: formatDuration(ev.elapsed) },
          { type: 'kv', key: 'buffer_seq', value: String(n(ev.buffer_seq)) },
        ];
        if (ev.captures) {
          out.push({ type: 'subhead', text: '\u2014 captures' });
          for (const [name, value] of Object.entries(ev.captures)) {
            if (value === undefined) continue;
            out.push({ type: 'kv', key: `$${name}`, value, mono: true, accent: true });
          }
        }
        return out;
      }
      case 'timeout': {
        const out: Row[] = [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true },
          { type: 'kv', key: 'buffer_seq', value: ev.buffer_seq === null ? '\u2014' : String(n(ev.buffer_seq)) },
        ];
        out.push({ type: 'subhead', text: '\u2014 timeout' });
        out.push(...timeoutValueRows(ev.effective));
        return out;
      }
      case 'fail-pattern-set':
        return [{ type: 'kv', key: 'pattern', value: ev.pattern, mono: true }];
      case 'fail-pattern-cleared':
        return [{ type: 'kv', key: 'fail patterns', value: 'cleared' }];
      case 'fail-pattern-triggered':
        return [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true },
          { type: 'kv', key: 'matched line', value: ev.matched_line, mono: true },
        ];
      case 'sleep-start':
        return [{ type: 'kv', key: 'duration', value: formatDuration(ev.duration) }];
      case 'sleep-done':
        return [];
      case 'timeout-set': {
        const out: Row[] = timeoutValueRows(ev.timeout);
        out.push({ type: 'subhead', text: '\u2014 previous' });
        out.push(...timeoutValueRows(ev.previous));
        return out;
      }
      case 'var-let':
      case 'var-assign':
        return [
          { type: 'kv', key: 'name', value: ev.name, mono: true },
          { type: 'kv', key: 'value', value: ev.value, mono: true },
        ];
      case 'string-eval':
        return [{ type: 'kv', key: 'result', value: ev.result, mono: true }];
      case 'interpolation': {
        const out: Row[] = [
          { type: 'kv', key: 'template', value: ev.template, mono: true },
          { type: 'kv', key: 'result', value: ev.result, mono: true, accent: true },
        ];
        if (ev.bindings.length > 0) {
          out.push({ type: 'subhead', text: '\u2014 bindings' });
          for (const [k, v] of ev.bindings) {
            out.push({ type: 'kv', key: k, value: v, mono: true });
          }
        }
        return out;
      }
      case 'annotate':
        return [{ type: 'kv', key: 'text', value: ev.text }];
      case 'log':
      case 'warning':
      case 'error':
        return [{ type: 'kv', key: 'message', value: ev.message }];
      case 'shell-spawn':
        return [
          { type: 'kv', key: 'name', value: ev.name, mono: true },
          { type: 'kv', key: 'command', value: ev.command, mono: true },
        ];
      case 'shell-ready':
      case 'shell-switch':
      case 'shell-terminate':
        return [{ type: 'kv', key: 'shell', value: ev.name, mono: true }];
      case 'effect-expose-shell':
        return [
          { type: 'kv', key: 'name', value: ev.name, mono: true },
          { type: 'kv', key: 'target', value: ev.target, mono: true },
          { type: 'kv', key: 'qualifier', value: ev.qualifier ?? '\u2014' },
        ];
      case 'effect-expose-var':
        return [
          { type: 'kv', key: 'name', value: ev.name, mono: true },
          { type: 'kv', key: 'value', value: ev.value, mono: true },
          { type: 'kv', key: 'target', value: ev.target, mono: true },
          { type: 'kv', key: 'qualifier', value: ev.qualifier ?? '\u2014' },
        ];
    }
  }

  function spanRows(span: Span): Row[] {
    if (span.kind === 'shell-block') {
      const props = shellBlockProps(state.data, n(span.id));
      const out: Row[] = [{ type: 'kv', key: 'shell', value: span.shell, mono: true }];
      if (props !== null) {
        out.push({ type: 'kv', key: 'command', value: props.command, mono: true });
        if (props.startupMs !== null) {
          out.push({ type: 'kv', key: 'startup', value: formatDuration(props.startupMs) });
        }
      }
      if (span.location !== null) {
        out.push({
          type: 'kv',
          key: 'location',
          value: `${span.location.file}:${span.location.line}`,
          mono: true,
        });
      }
      return out;
    }
    if (span.kind === 'effect-setup') {
      const props = effectSetupProps(state.data, n(span.id));
      const out: Row[] = [
        { type: 'kv', key: 'effect', value: span.effect, mono: true },
      ];
      if (span.alias !== null) out.push({ type: 'kv', key: 'alias', value: span.alias, mono: true });
      if (props !== null) {
        if (props.overlay.length > 0) {
          out.push({ type: 'subhead', text: '\u2014 expects' });
          for (const [k, v] of props.overlay) {
            out.push({ type: 'kv', key: k, value: v, mono: true });
          }
        }
        if (props.shellExposes.length > 0) {
          out.push({ type: 'subhead', text: '\u2014 exposes shells' });
          for (const e of props.shellExposes) {
            const target =
              e.qualifier !== null ? `${e.qualifier}.${e.target}` : e.target;
            out.push({
              type: 'kv',
              key: e.name,
              value: target === e.name ? target : `${target}`,
              mono: true,
            });
          }
        }
        if (props.varExposes.length > 0) {
          out.push({ type: 'subhead', text: '\u2014 exposes vars' });
          for (const v of props.varExposes) {
            out.push({ type: 'kv', key: v.name, value: v.value, mono: true });
          }
        }
      }
      return out;
    }
    if (span.kind === 'fn-call') {
      const out: Row[] = [{ type: 'kv', key: 'name', value: span.name, mono: true }];
      if (span.result !== null) {
        out.push({ type: 'kv', key: 'result', value: span.result, mono: true, accent: true });
      }
      if (span.args.length > 0) {
        out.push({ type: 'subhead', text: '\u2014 args' });
        for (const [k, v] of span.args) {
          out.push({ type: 'kv', key: k, value: v, mono: true });
        }
      }
      return out;
    }
    if (span.kind === 'test') {
      return [
        { type: 'kv', key: 'name', value: span.name, mono: true },
        { type: 'kv', key: 'path', value: state.data.test.path, mono: true },
        { type: 'kv', key: 'outcome', value: state.data.test.outcome },
      ];
    }
    if (span.kind === 'effect-cleanup') {
      return [{ type: 'kv', key: 'effect', value: span.effect, mono: true }];
    }
    return [];
  }

  function rowKey(row: Row, i: number, mode: Mode): string {
    const prefix =
      mode.kind === 'event' ? `e${n(mode.event.seq)}` : `s${n(mode.span.id)}`;
    if (row.type === 'subhead') return `${prefix}:sub:${i}`;
    return `${prefix}:${row.key}:${i}`;
  }

  function expandedKey(row: Row & { type: 'kv' }, i: number): string {
    return rowKey(row, i, mode);
  }
</script>

<div class="card {family}">
  <header class="head">
    <b>{head.title}</b>
    <span class="sub">{head.subtitle}</span>
  </header>
  <div class="grid">
    {#each rows as row, i (rowKey(row, i, mode))}
      {#if row.type === 'subhead'}
        <div class="subhead">{row.text}</div>
      {:else}
        {@const key = expandedKey(row, i)}
        <div class="kv-row">
          <div class="k">{row.key}</div>
          <div class="v" class:accent={row.accent}>
            {#if row.mono}
              <ValueCell value={row.value} {state} expandKey={key} accent={row.accent} />
            {:else}
              <span class="plain">{row.value}</span>
            {/if}
          </div>
        </div>
      {/if}
    {/each}
    {#if rows.length === 0}
      <p class="muted">no additional details.</p>
    {/if}
  </div>
</div>

<style>
  .card {
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius);
    background: var(--paper-2);
    padding: var(--gap-sm) var(--gap-md);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    margin: var(--gap-xs) 0 var(--gap-sm) var(--gap-md);
  }
  .card.ok {
    border-left-color: var(--accent-2);
  }
  .card.danger {
    border-left-color: var(--danger);
  }
  .card.info {
    border-left-color: var(--ink-dim);
  }
  .head {
    display: flex;
    gap: var(--gap-sm);
    align-items: baseline;
    color: var(--ink-faint);
    font-size: 0.72rem;
    border-bottom: 1px dashed var(--border);
    padding-bottom: 4px;
    margin-bottom: 6px;
  }
  .head b {
    color: var(--ink);
    font-weight: 600;
    font-size: 0.82rem;
  }
  .sub {
    color: var(--ink-faint);
  }
  .grid {
    display: grid;
    grid-template-columns: 110px minmax(0, 1fr);
    gap: 3px var(--gap-sm);
    align-items: baseline;
  }
  .subhead {
    grid-column: 1 / -1;
    color: var(--ink-faint);
    font-size: 0.72rem;
    padding-top: 4px;
  }
  .kv-row {
    display: contents;
  }
  .kv-row .k {
    color: var(--ink-faint);
    padding: 1px 0;
  }
  .kv-row .v {
    color: var(--ink);
    min-width: 0;
    padding: 1px 0;
  }
  .kv-row .v.accent .plain {
    color: var(--accent);
  }
  .kv-row .v .plain {
    display: inline-block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .muted {
    grid-column: 1 / -1;
    color: var(--ink-faint);
    font-style: italic;
    margin: 0;
  }
</style>
