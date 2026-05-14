import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StackFrame } from '../types/StackFrame';
import type { StructuredLog } from '../types/StructuredLog';
import type { TimeoutValue } from '../types/TimeoutValue';

// ts-rs annotates `seq`, `span.id`, `parent` as `bigint`, but at runtime
// they arrive via `JSON.parse(window.RELUX_DATA)` and are plain `number`s
// (Rust u64 fits inside JS's 53-bit safe-integer range for these counts).
// We use `number` for the runtime keys throughout; lookups into
// `data.spans` use `String(id)` because object literal keys are strings.

export type SpanId = number;

function n(value: bigint | number): number {
  return Number(value);
}

export function eventBySeq(data: StructuredLog, seq: number): Event | null {
  for (const ev of data.events) {
    if (n(ev.seq) === seq) return ev;
  }
  return null;
}

export function spanById(data: StructuredLog, id: SpanId): Span | null {
  const map = data.spans as unknown as Record<string, Span | undefined>;
  return map[String(id)] ?? null;
}

export function ancestors(data: StructuredLog, spanId: SpanId): Span[] {
  const chain: Span[] = [];
  let current: Span | null = spanById(data, spanId);
  while (current) {
    chain.push(current);
    if (current.parent === null) break;
    current = spanById(data, n(current.parent));
  }
  return chain.reverse();
}

export function descendants(data: StructuredLog, spanId: SpanId): Set<SpanId> {
  const childrenByParent = new Map<SpanId, SpanId[]>();
  const map = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(map)) {
    const span = map[key];
    if (!span || span.parent === null) continue;
    const parent = n(span.parent);
    let bucket = childrenByParent.get(parent);
    if (!bucket) {
      bucket = [];
      childrenByParent.set(parent, bucket);
    }
    bucket.push(n(span.id));
  }
  const out = new Set<SpanId>();
  const stack: SpanId[] = [spanId];
  while (stack.length > 0) {
    const id = stack.pop()!;
    const kids = childrenByParent.get(id);
    if (!kids) continue;
    for (const kid of kids) {
      if (out.has(kid)) continue;
      out.add(kid);
      stack.push(kid);
    }
  }
  return out;
}

export function buildCallStack(data: StructuredLog, event: Event): StackFrame[] {
  return ancestors(data, n(event.span)).map(toStackFrame);
}

function toStackFrame(span: Span): StackFrame {
  let name: string | null = null;
  let args: Array<[string, string]> = [];
  let alias: string | null = null;
  switch (span.kind) {
    case 'test':
      name = span.name;
      break;
    case 'effect-setup':
      name = span.effect;
      args = span.overlay;
      alias = span.alias;
      break;
    case 'effect-cleanup':
      name = span.effect;
      break;
    case 'shell-block':
      name = span.shell;
      break;
    case 'cleanup-block':
      name = null;
      break;
    case 'fn-call':
      name = span.name;
      args = span.args;
      break;
  }
  return {
    span: span.id,
    kind: span.kind,
    name,
    args,
    alias,
    location: span.location,
  };
}

export interface BufferRegions {
  consumed: string;
  matched: { bytes: string; seq: number } | null;
  tail: string;
}

// Per-shell buffer reconstruction up to a given event seq.
//
// The buffer is append-only from the viewer's perspective: `grew` adds
// bytes to the tail; `matched` does not remove bytes, it just re-colors
// them (previously highlighted bytes fold into consumed, the freshly
// matched bytes become the new highlight, bytes after the match stay in
// the tail).
//
// Invariant: at the moment of a `matched` buffer event, the runtime's
// unmatched tail equals `before + matched + after`. The runtime emits
// `before` and `after` untruncated for exactly this reason, so the viewer
// can validate the invariant and rebuild lossless history. When the
// invariant fails (it shouldn't if grew/matched events are consistent),
// we still fall back to `before + matched + after` for the new tail
// segment so the user sees coherent regions.
// Returns the shell name to render in the buffer pane for a span.
//   shell-block   -> span.shell (carried on the span)
//   cleanup-block -> the shell of the first event inside the span; the
//                    runtime always spins a fresh implicit shell named
//                    `__cleanup`, but we discover it via events rather
//                    than hardcoding, since the span itself has no
//                    `shell` field
//   fn-call       -> first event inside the span subtree (the function
//                    executes in its caller's shell, whichever that is)
// Returns null for kinds without a shell context (test, effect-setup,
// effect-cleanup) or when no event inside the subtree carries a shell.
export function spanBufferShell(data: StructuredLog, span: Span): string | null {
  if (span.kind === 'shell-block') return span.shell;
  if (span.kind !== 'cleanup-block' && span.kind !== 'fn-call') return null;
  const subtree = new Set<SpanId>([n(span.id), ...descendants(data, n(span.id))]);
  for (const ev of data.events) {
    if (subtree.has(n(ev.span)) && ev.shell !== null) return ev.shell;
  }
  return null;
}

// Buffer-replay cutoff seq for span selection.
//   shell-block, cleanup-block -> seq of the next `shell-switch` event
//                                 after the span closes (where the shell
//                                 hands off; cleanup-block has its own
//                                 implicit shell that terminates rather
//                                 than switching, so the fallback below
//                                 carries it).
//   fn-call                    -> seq of the next event of any kind
//                                 after the call returns.
// Falls back to the last event's seq when no matching event exists (the
// span was the last thing in the test). Returns null for unclosed spans
// or kinds without buffer relevance.
export function spanBufferCutoffSeq(data: StructuredLog, span: Span): number | null {
  if (span.end_ts === null) return null;
  const endTs = span.end_ts;

  if (span.kind === 'shell-block' || span.kind === 'cleanup-block') {
    for (const ev of data.events) {
      if (ev.ts >= endTs && ev.kind === 'shell-switch') return n(ev.seq);
    }
    const last = data.events[data.events.length - 1];
    return last ? n(last.seq) : null;
  }

  if (span.kind === 'fn-call') {
    for (const ev of data.events) {
      if (ev.ts >= endTs) return n(ev.seq);
    }
    const last = data.events[data.events.length - 1];
    return last ? n(last.seq) : null;
  }

  return null;
}

export function replayBufferRegionsAtSeq(
  data: StructuredLog,
  seq: number,
  shell: string,
): BufferRegions {
  let consumed = '';
  let matched: { bytes: string; seq: number } | null = null;
  let tail = '';
  for (const ev of data.buffer_events) {
    if (n(ev.seq) > seq) break;
    if (ev.shell !== shell) continue;
    switch (ev.kind) {
      case 'grew':
        tail += ev.data;
        break;
      case 'matched': {
        const reconstructed = ev.before + ev.matched + ev.after;
        if (reconstructed !== tail) {
          console.warn(
            `[viewer] buffer reconstruction: tail mismatch at seq=${n(ev.seq)} shell=${shell}; ` +
              `tail.length=${tail.length} before+matched+after.length=${reconstructed.length}`,
          );
        }
        if (matched !== null) consumed += matched.bytes;
        consumed += ev.before;
        matched = { bytes: ev.matched, seq: n(ev.seq) };
        tail = ev.after;
        break;
      }
      case 'reset':
        consumed = '';
        matched = null;
        tail = '';
        break;
    }
  }
  return { consumed, matched, tail };
}

export { capturesAtSeq, scopeContext, varsAtSeq } from './scope';

export interface ShellContextSnapshot {
  failPatterns: string[];
  timeout: TimeoutValue | null;
  activeShell: string | null;
}

export function replayShellCtxAtSeq(
  data: StructuredLog,
  seq: number,
): ShellContextSnapshot {
  const failPatterns: string[] = [];
  let timeout: TimeoutValue | null = null;
  let activeShell: string | null = null;
  for (const ev of data.events) {
    if (n(ev.seq) > seq) break;
    switch (ev.kind) {
      case 'fail-pattern-set':
        failPatterns.push(ev.pattern);
        break;
      case 'fail-pattern-cleared':
        failPatterns.length = 0;
        break;
      case 'timeout-set':
        timeout = ev.timeout;
        break;
    }
    if (ev.shell !== null) activeShell = ev.shell;
  }
  return { failPatterns, timeout, activeShell };
}

export interface EffectShellExpose {
  name: string;
  target: string;
  qualifier: string | null;
}

export interface EffectVarExpose {
  name: string;
  target: string;
  qualifier: string | null;
  value: string;
}

export interface EffectSetupProps {
  overlay: Array<[string, string]>;
  shellExposes: EffectShellExpose[];
  varExposes: EffectVarExpose[];
}

export function effectSetupProps(
  data: StructuredLog,
  spanId: SpanId,
): EffectSetupProps | null {
  const span = spanById(data, spanId);
  if (!span || span.kind !== 'effect-setup') return null;
  const shellExposes: EffectShellExpose[] = [];
  const varExposes: EffectVarExpose[] = [];
  for (const ev of data.events) {
    if (n(ev.span) !== spanId) continue;
    if (ev.kind === 'effect-expose-shell') {
      shellExposes.push({
        name: ev.name,
        target: ev.target,
        qualifier: ev.qualifier,
      });
    } else if (ev.kind === 'effect-expose-var') {
      varExposes.push({
        name: ev.name,
        target: ev.target,
        qualifier: ev.qualifier,
        value: ev.value,
      });
    }
  }
  return { overlay: span.overlay, shellExposes, varExposes };
}

export interface ShellBlockProps {
  command: string;
  startupMs: number | null;
}

export function shellBlockProps(
  data: StructuredLog,
  spanId: SpanId,
): ShellBlockProps | null {
  let command: string | null = null;
  let spawnTs: number | null = null;
  let readyTs: number | null = null;
  for (const ev of data.events) {
    if (n(ev.span) !== spanId) continue;
    if (ev.kind === 'shell-spawn') {
      command = ev.command;
      spawnTs = ev.ts;
    } else if (ev.kind === 'shell-ready') {
      readyTs = ev.ts;
    }
  }
  if (command === null || spawnTs === null) return null;
  return {
    command,
    startupMs: readyTs !== null ? readyTs - spawnTs : null,
  };
}

// Find the matching `shell-spawn` event for a given shell name; the latest
// one before `beforeSeq` (exclusive). Used by `shell-terminate` to compute
// lifetime and accumulated bytes received.
export function matchingShellSpawn(
  data: StructuredLog,
  shell: string,
  beforeSeq: number,
): Event | null {
  let match: Event | null = null;
  for (const ev of data.events) {
    const seq = n(ev.seq);
    if (seq >= beforeSeq) break;
    if (ev.kind === 'shell-spawn' && ev.name === shell) match = ev;
  }
  return match;
}

// Most recent event in a given shell strictly before `beforeSeq`. Used by
// `shell-block` reuse cards to compute "last active" (gap between previous
// activity in this shell and this block's start).
export function lastEventInShell(
  data: StructuredLog,
  shell: string,
  beforeSeq: number,
): Event | null {
  let last: Event | null = null;
  for (const ev of data.events) {
    const seq = n(ev.seq);
    if (seq >= beforeSeq) break;
    if (ev.shell === shell) last = ev;
  }
  return last;
}

// First event inside a span (matched by `ev.span === spanId`), or null if
// the span has no inner events. Used by `shell-block` cards to anchor the
// "bytes since" range at the block's first event.
export function firstEventInSpan(
  data: StructuredLog,
  spanId: SpanId,
): Event | null {
  for (const ev of data.events) {
    if (n(ev.span) === spanId) return ev;
  }
  return null;
}

// Sum byte lengths of `grew` buffer events for the given shell in the
// half-open seq range `[fromSeq, toSeq)`. The buffer event stream is the
// authoritative source for shell output bytes; `Recv` structured events
// aren't emitted today.
export function bufferBytesGrewBetween(
  data: StructuredLog,
  shell: string,
  fromSeq: number,
  toSeq: number,
): number {
  let total = 0;
  for (const ev of data.buffer_events) {
    const seq = n(ev.seq);
    if (seq < fromSeq) continue;
    if (seq >= toSeq) break;
    if (ev.shell !== shell) continue;
    if (ev.kind === 'grew') total += ev.data.length;
  }
  return total;
}

// First-use vs. reuse for a shell-block / cleanup-block span. A first-use
// block is the one that spawns its shell (contains a `shell-spawn` event
// in its own span). Reuse blocks just switch into an already-spawned
// shell.
export interface ShellBlockLifecycle {
  firstUse: boolean;
  spawn: Event | null; // present iff firstUse
  ready: Event | null; // present iff firstUse (and the shell came up)
}

export function shellBlockLifecycle(
  data: StructuredLog,
  spanId: SpanId,
): ShellBlockLifecycle {
  let spawn: Event | null = null;
  let ready: Event | null = null;
  for (const ev of data.events) {
    if (n(ev.span) !== spanId) continue;
    if (ev.kind === 'shell-spawn') spawn = ev;
    else if (ev.kind === 'shell-ready') ready = ev;
  }
  return { firstUse: spawn !== null, spawn, ready };
}

export type LiveShellState = 'ready' | 'busy' | 'ended' | 'error';

export interface LiveShell {
  name: string;
  command: string;
  state: LiveShellState;
}

// Approximate per-shell state at the moment of `event`.
//
//   - "ended"  : shell had a `shell-terminate` event at-or-before `event.seq`.
//   - "busy"   : most recent `match-start` for the shell at-or-before seq
//                has no corresponding `match-done` yet.
//   - "ready"  : otherwise (post-`shell-ready`, idle prompt).
//
// Returns one entry per shell in `data.shells`, in declaration order.
export function liveShellsAtSeq(data: StructuredLog, event: Event): LiveShell[] {
  const seq = n(event.seq);
  const out: LiveShell[] = [];
  const records = data.shells as unknown as Record<
    string,
    { command: string; spawn_ts: number; terminate_ts: number | null } | undefined
  >;
  for (const name of Object.keys(records)) {
    const rec = records[name];
    if (!rec) continue;
    let state: LiveShellState = 'ready';
    let busy = false;
    let ended = false;
    for (const ev of data.events) {
      if (n(ev.seq) > seq) break;
      if (ev.shell !== name && ev.kind !== 'shell-terminate') continue;
      if (ev.kind === 'shell-terminate' && ev.name === name) ended = true;
      else if (ev.kind === 'match-start') busy = true;
      else if (ev.kind === 'match-done' || ev.kind === 'timeout') busy = false;
    }
    if (ended) state = 'ended';
    else if (busy) state = 'busy';
    out.push({ name, command: rec.command, state });
  }
  return out;
}

export { n as toNumber };
