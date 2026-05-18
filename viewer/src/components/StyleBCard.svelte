<script lang="ts">
  import type { Event } from '../types/Event';
  import type { Span } from '../types/Span';
  import type { ViewerState } from '../lib/state.svelte';
  import type { FoldedEvent } from '../lib/flatten';
  import { leadEvent } from '../lib/flatten';
  import {
    bootstrapForReuse,
    bufferBytesGrewBetween,
    effectSetupProps,
    finalCleanupForDeferred,
    firstEventInSpan,
    lastEventInShell,
    matchingShellSpawn,
    shellBlockLifecycle,
    toNumber as n,
  } from '../lib/derive';
  import type { SpanId } from '../lib/derive';
  import MarkerPill from './MarkerPill.svelte';
  import {
    displaySpanCallKind,
    displaySpanKind,
    foldedFamily,
    foldedKindLabel,
    formatBytes,
    formatDuration,
    formatTimeoutLine,
  } from '../lib/format';
  import NameCell from './NameCell.svelte';
  import ValueCell from './ValueCell.svelte';

  type Mode =
    | { kind: 'event'; folded: FoldedEvent }
    | { kind: 'span'; span: Span };

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

  const family = $derived(mode.kind === 'event' ? foldedFamily(mode.folded) : 'event');
  const head = $derived(buildHead());

  type PillProps = {
    marker: string;
    prefix: 'reused' | 'deferred' | null;
    jumpTo: SpanId | null;
  };
  const pillProps = $derived.by<PillProps | null>(() => {
    if (mode.kind !== 'span') return null;
    const span = mode.span;
    if (span.kind === 'effect-setup') {
      return {
        marker: span.marker,
        prefix: span.is_reuse ? 'reused' : null,
        jumpTo: span.is_reuse ? bootstrapForReuse(state.data, span.marker) : null,
      };
    }
    if (span.kind === 'effect-cleanup') {
      return {
        marker: span.marker,
        prefix: span.is_deferred ? 'deferred' : null,
        jumpTo: span.is_deferred ? finalCleanupForDeferred(state.data, span.marker) : null,
      };
    }
    return null;
  });
  const rows = $derived<Row[]>(
    mode.kind === 'event' ? foldedRows(mode.folded) : spanRows(mode.span),
  );

  function buildHead(): { title: string; subtitle: string } {
    if (mode.kind === 'event') {
      return {
        title: buildFoldedTitle(mode.folded),
        subtitle: buildFoldedSubtitle(mode.folded),
      };
    }
    const span = mode.span;
    return { title: buildSpanTitle(span), subtitle: buildSpanSubtitle(span) };
  }

  function buildFoldedTitle(f: FoldedEvent): string {
    if (f.kind === 'match' && f.start.kind === 'match-start') {
      return `match \u00b7 ${f.start.is_regex ? 'regex' : 'literal'}`;
    }
    if (f.kind === 'sleep' && f.start.kind === 'sleep-start') {
      return `sleep \u00b7 ${formatDuration(f.start.duration)}`;
    }
    if (f.kind === 'single') {
      const ev = f.event;
      if (ev.kind === 'fail-pattern-set' || ev.kind === 'fail-pattern-triggered') {
        return `${ev.kind} \u00b7 ${ev.is_regex ? 'regex' : 'literal'}`;
      }
      if (ev.kind === 'var-let' || ev.kind === 'var-assign') {
        return `${ev.kind} \u00b7 ${ev.name}`;
      }
    }
    return foldedKindLabel(f);
  }

  function buildFoldedSubtitle(f: FoldedEvent): string {
    const lead = leadEvent(f);
    const parts: string[] = [];
    if (lead.shell !== null) parts.push(`shell ${lead.shell}`);
    parts.push(`t = ${formatDuration(lead.ts)}`);
    if (f.kind === 'match') {
      if (f.outcome.kind === 'match-done') parts.push(`${formatDuration(f.outcome.elapsed)} wait`);
      else if (f.outcome.kind === 'timeout') parts.push('timed out');
    }
    return parts.join(' \u00b7 ');
  }

  function foldedRows(f: FoldedEvent): Row[] {
    switch (f.kind) {
      case 'single':
        return eventRows(f.event);
      case 'sleep':
        return sleepFoldRows(f.start, f.done);
      case 'match':
        return matchFoldRows(f.start, f.outcome);
    }
  }

  function sleepFoldRows(start: Event, done: Event): Row[] {
    if (start.kind !== 'sleep-start') return [];
    return [{ type: 'kv', key: 'actual', value: formatDuration(done.ts - start.ts) }];
  }

  function matchFoldRows(start: Event, outcome: Event): Row[] {
    if (start.kind !== 'match-start') return [];
    const out: Row[] = [
      { type: 'kv', key: 'pattern', value: start.pattern, mono: true, accent: true },
    ];
    if (outcome.kind === 'match-done') {
      out.push({ type: 'kv', key: 'elapsed', value: formatDuration(outcome.elapsed) });
      if (start.is_regex) {
        out.push({ type: 'kv', key: 'matched', value: outcome.matched, mono: true, accent: true });
      }
      const caps = outcome.captures
        ? Object.entries(outcome.captures).filter(
            (entry): entry is [string, string] => entry[1] !== undefined,
          )
        : [];
      if (caps.length > 0) {
        out.push({ type: 'subhead', text: 'captures' });
        for (const [name, value] of caps) {
          out.push({ type: 'kv', key: `$${name}`, value, mono: true, accent: true });
        }
      }
    } else if (outcome.kind === 'timeout') {
      out.push({
        type: 'kv',
        key: 'timeout',
        value: formatTimeoutLine(start.effective),
        mono: true,
      });
    }
    return out;
  }

  function buildSpanTitle(span: Span): string {
    const label = displaySpanKind(span.kind);
    switch (span.kind) {
      case 'shell-block':
        return `${label} \u00b7 ${span.shell}`;
      case 'cleanup-block': {
        const shell = lifecycleShellName(span);
        return shell !== null ? `${label} \u00b7 ${shell}` : label;
      }
      case 'fn-call': {
        const callLabel = displaySpanCallKind(span);
        const head =
          span.callee_kind === 'bif' ? `${span.name}/${span.args.length}` : span.name;
        return `${callLabel} \u00b7 ${head}`;
      }
      default:
        return label;
    }
  }

  function buildSpanSubtitle(span: Span): string {
    return `t = ${formatDuration(span.start_ts)}`;
  }

  function eventRows(ev: Event): Row[] {
    switch (ev.kind) {
      case 'send':
      case 'recv':
        return [{ type: 'kv', key: 'data', value: ev.data, mono: true }];
      case 'match-start':
        return [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true, accent: true },
          { type: 'kv', key: 'timeout', value: formatTimeoutLine(ev.effective), mono: true },
        ];
      case 'match-done': {
        const out: Row[] = [
          { type: 'kv', key: 'matched', value: ev.matched, mono: true, accent: true },
          { type: 'kv', key: 'elapsed', value: formatDuration(ev.elapsed) },
        ];
        const caps = ev.captures
          ? Object.entries(ev.captures).filter(
              (entry): entry is [string, string] => entry[1] !== undefined,
            )
          : [];
        if (caps.length > 0) {
          out.push({ type: 'subhead', text: 'captures' });
          for (const [name, value] of caps) {
            out.push({ type: 'kv', key: `$${name}`, value, mono: true, accent: true });
          }
        }
        return out;
      }
      case 'timeout':
        return [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true },
          { type: 'kv', key: 'timeout', value: formatTimeoutLine(ev.effective), mono: true },
        ];
      case 'fail-pattern-set':
        return [{ type: 'kv', key: 'pattern', value: ev.pattern, mono: true, accent: true }];
      case 'fail-pattern-cleared':
        return [{ type: 'kv', key: 'fail patterns', value: 'cleared' }];
      case 'fail-pattern-triggered': {
        const out: Row[] = [
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true, accent: true },
        ];
        if (ev.is_regex) {
          out.push({ type: 'kv', key: 'matched', value: ev.matched_line, mono: true, accent: true });
        }
        return out;
      }
      case 'sleep-start':
        return [{ type: 'kv', key: 'duration', value: formatDuration(ev.duration) }];
      case 'sleep-done':
        return [];
      case 'timeout-set':
        return [
          { type: 'kv', key: 'new', value: formatTimeoutLine(ev.timeout), mono: true },
          { type: 'kv', key: 'previous', value: formatTimeoutLine(ev.previous), mono: true },
        ];
      case 'var-let':
        return [{ type: 'kv', key: 'value', value: ev.value, mono: true }];
      case 'var-assign':
        return [
          { type: 'kv', key: 'value', value: ev.value, mono: true },
          { type: 'kv', key: 'previous', value: ev.previous, mono: true },
        ];
      case 'string-eval':
        return [{ type: 'kv', key: 'result', value: ev.result, mono: true }];
      case 'interpolation': {
        const out: Row[] = [
          { type: 'kv', key: 'template', value: ev.template, mono: true },
          { type: 'kv', key: 'result', value: ev.result, mono: true, accent: true },
        ];
        if (ev.bindings.length > 0) {
          out.push({ type: 'subhead', text: 'bindings' });
          for (const [k, v] of ev.bindings) {
            out.push({ type: 'kv', key: `\${${k}}`, value: v, mono: true });
          }
        }
        return out;
      }
      case 'var-read':
        return [{ type: 'kv', key: 'name', value: ev.name, mono: true }, { type: 'kv', key: 'value', value: ev.value, mono: true, accent: true }];
      case 'bool-check': {
        const out: Row[] = [];
        const e = ev.evaluation;
        switch (e.shape) {
          case 'unconditional':
            out.push({ type: 'kv', key: 'condition', value: 'unconditional' });
            break;
          case 'bare':
            out.push({ type: 'kv', key: 'value', value: e.value, mono: true });
            out.push({ type: 'kv', key: 'met', value: String(e.met), accent: true });
            break;
          case 'eq':
            out.push({ type: 'kv', key: 'lhs', value: e.lhs, mono: true });
            out.push({ type: 'kv', key: 'rhs', value: e.rhs, mono: true });
            out.push({ type: 'kv', key: 'met', value: String(e.met), accent: true });
            break;
          case 'regex':
            out.push({ type: 'kv', key: 'value', value: e.value, mono: true });
            out.push({ type: 'kv', key: 'pattern', value: e.pattern, mono: true });
            out.push({ type: 'kv', key: 'met', value: String(e.met), accent: true });
            break;
        }
        return out;
      }
      case 'pure-match': {
        const out: Row[] = [
          { type: 'kv', key: 'value', value: ev.value, mono: true },
          { type: 'kv', key: 'pattern', value: ev.pattern, mono: true, accent: true },
        ];
        if (ev.result !== '') {
          out.push({ type: 'kv', key: 'matched', value: ev.result, mono: true, accent: true });
        } else {
          out.push({ type: 'kv', key: 'matched', value: '(none)' });
        }
        const caps = Object.entries(ev.captures).filter(
          (entry): entry is [string, string] =>
            entry[0] !== '0' && entry[1] !== undefined,
        );
        if (caps.length > 0) {
          out.push({ type: 'subhead', text: 'captures' });
          for (const [k, v] of caps) {
            out.push({ type: 'kv', key: `$${k}`, value: v, mono: true, accent: true });
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
        return [{ type: 'kv', key: 'shell', value: ev.name, mono: true }];
      case 'shell-terminate': {
        const out: Row[] = [];
        const seq = n(ev.seq);
        const spawn = matchingShellSpawn(state.data, ev.name, seq);
        if (spawn !== null) {
          out.push({ type: 'kv', key: 'lifetime', value: formatDuration(ev.ts - spawn.ts) });
          const spawnSeq = n(spawn.seq);
          const bytes = bufferBytesGrewBetween(state.data, ev.name, spawnSeq, seq);
          out.push({ type: 'kv', key: 'bytes received', value: formatBytes(bytes) });
        }
        return out;
      }
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
      case 'cancelled': {
        const out: Row[] = [
          { type: 'kv', key: 'reason', value: ev.reason.type, mono: true, accent: true },
        ];
        switch (ev.reason.type) {
          case 'test-timeout':
          case 'suite-timeout':
            out.push({
              type: 'kv',
              key: 'duration',
              value: `${ev.reason.duration_ms}ms`,
              mono: true,
            });
            break;
          case 'fail-fast':
            out.push({
              type: 'kv',
              key: 'trigger',
              value: ev.reason.trigger_test,
              mono: true,
            });
            break;
          case 'sigint':
            break;
        }
        return out;
      }
    }
  }

  // Cleanup-block spans don't carry a shell name on the span itself; the
  // runtime spins a fresh implicit shell named `__cleanup`. Discover it by
  // peeking at the first event with a shell inside the span's subtree.
  function lifecycleShellName(span: Span): string | null {
    if (span.kind === 'shell-block') return span.shell;
    if (span.kind !== 'cleanup-block') return null;
    const spanId = n(span.id);
    for (const ev of state.data.events) {
      if (n(ev.span) === spanId && ev.shell !== null) return ev.shell;
    }
    return null;
  }

  function spanRows(span: Span): Row[] {
    if (span.kind === 'shell-block' || span.kind === 'cleanup-block') {
      return shellLikeBlockRows(span);
    }
    if (span.kind === 'effect-setup') {
      const props = effectSetupProps(state.data, n(span.id));
      const out: Row[] = [];
      out.push({ type: 'kv', key: 'elapsed', value: spanElapsed(span) });
      out.push({ type: 'kv', key: 'effect', value: span.effect, mono: true });
      if (span.alias !== null) out.push({ type: 'kv', key: 'alias', value: span.alias, mono: true });
      if (props !== null) {
        if (props.overlay.length > 0) {
          out.push({ type: 'subhead', text: 'expects' });
          for (const [k, v] of props.overlay) {
            out.push({ type: 'kv', key: k, value: v, mono: true });
          }
        }
        if (props.shellExposes.length > 0) {
          out.push({ type: 'subhead', text: 'exposes shells' });
          for (const e of props.shellExposes) {
            const target =
              e.qualifier !== null ? `${e.qualifier}.${e.target}` : e.target;
            out.push({
              type: 'kv',
              key: e.name,
              value: target,
              mono: true,
            });
          }
        }
        if (props.varExposes.length > 0) {
          out.push({ type: 'subhead', text: 'exposes vars' });
          for (const v of props.varExposes) {
            out.push({ type: 'kv', key: v.name, value: v.value, mono: true });
          }
        }
      }
      return out;
    }
    if (span.kind === 'fn-call') {
      const out: Row[] = [{ type: 'kv', key: 'elapsed', value: spanElapsed(span) }];
      if (span.result !== null) {
        out.push({ type: 'kv', key: 'result', value: span.result, mono: true, accent: true });
      }
      if (span.args.length > 0) {
        out.push({ type: 'subhead', text: 'arguments' });
        for (const [k, v] of span.args) {
          out.push({ type: 'kv', key: k, value: v, mono: true });
        }
      }
      return out;
    }
    if (span.kind === 'effect-cleanup') {
      const out: Row[] = [
        { type: 'kv', key: 'elapsed', value: spanElapsed(span) },
        { type: 'kv', key: 'effect', value: span.effect, mono: true },
      ];
      if (span.alias !== null) {
        out.push({ type: 'kv', key: 'alias', value: span.alias, mono: true });
      }
      return out;
    }
    if (span.kind === 'marker-eval') {
      return [
        { type: 'kv', key: 'marker', value: `@${span.marker_kind}`, mono: true },
        { type: 'kv', key: 'modifier', value: span.modifier, mono: true },
        { type: 'kv', key: 'decision', value: span.decision, accent: true },
      ];
    }
    return [];
  }

  function spanElapsed(span: Span): string {
    return span.end_ts !== null ? formatDuration(span.end_ts - span.start_ts) : '\u2014';
  }

  function shellLikeBlockRows(span: Span): Row[] {
    const shell = lifecycleShellName(span);
    const out: Row[] = [{ type: 'kv', key: 'elapsed', value: spanElapsed(span) }];

    const lifecycle = shellBlockLifecycle(state.data, n(span.id));
    if (
      lifecycle.firstUse &&
      lifecycle.spawn !== null &&
      lifecycle.spawn.kind === 'shell-spawn'
    ) {
      out.push({ type: 'kv', key: 'command', value: lifecycle.spawn.command, mono: true });
      if (lifecycle.ready !== null) {
        out.push({
          type: 'kv',
          key: 'startup',
          value: formatDuration(lifecycle.ready.ts - lifecycle.spawn.ts),
        });
      }
      return out;
    }

    // Reuse block: surface how stale the shell was on entry and how many
    // bytes accumulated in its buffer since the previous activity.
    if (shell !== null) {
      const firstInside = firstEventInSpan(state.data, n(span.id));
      const cutoffSeq = firstInside !== null ? n(firstInside.seq) : null;
      if (cutoffSeq !== null) {
        const prev = lastEventInShell(state.data, shell, cutoffSeq);
        if (prev !== null) {
          out.push({
            type: 'kv',
            key: 'last active',
            value: `${formatDuration(span.start_ts - prev.ts)} ago`,
          });
          const bytes = bufferBytesGrewBetween(state.data, shell, n(prev.seq), cutoffSeq);
          out.push({ type: 'kv', key: 'bytes since', value: formatBytes(bytes) });
        }
      }
    }
    return out;
  }

  function rowKey(row: Row, i: number, mode: Mode): string {
    const prefix =
      mode.kind === 'event'
        ? `e${n(leadEvent(mode.folded).seq)}`
        : `s${n(mode.span.id)}`;
    if (row.type === 'subhead') return `${prefix}:sub:${i}`;
    return `${prefix}:${row.key}:${i}`;
  }

  function expandedKey(row: Row & { type: 'kv' }, i: number): string {
    return rowKey(row, i, mode);
  }
</script>

<div class="card {family}">
  <header class="head">
    <span class="title">{head.title}</span>
    {#if pillProps}
      <MarkerPill
        {state}
        marker={pillProps.marker}
        prefix={pillProps.prefix}
        jumpTo={pillProps.jumpTo}
      />
    {/if}
    <span class="sub">{head.subtitle}</span>
  </header>
  <div class="grid">
    {#each rows as row, i (rowKey(row, i, mode))}
      {#if row.type === 'subhead'}
        <div class="subhead">{row.text}</div>
      {:else}
        {@const key = expandedKey(row, i)}
        <div class="kv-row">
          <div class="k"><NameCell name={row.key} /></div>
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
    border-left: 3px solid var(--ink-dim);
    border-radius: var(--radius);
    background: var(--paper-2);
    padding: var(--gap-sm) var(--gap-md);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    margin: var(--gap-xs) 0 var(--gap-sm) 0;
  }
  .card.ok {
    border-left-color: var(--accent-2);
  }
  .card.danger {
    border-left-color: var(--danger);
  }
  .card.info {
    border-left-color: var(--info);
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
  .head .title {
    color: var(--ink);
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
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    color: var(--ink-dim);
    font-size: 0.72rem;
    padding-top: 6px;
  }
  .subhead::before,
  .subhead::after {
    content: '';
    border-top: 1px dashed var(--border);
    height: 0;
  }
  .subhead::before {
    flex: 0 0 16px;
  }
  .subhead::after {
    flex: 1 1 auto;
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
