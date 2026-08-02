import type { BufferEvent } from '../types/BufferEvent';
import type { Event } from '../types/Event';
import type { MultiMatchPattern } from '../types/MultiMatchPattern';
import type { SourceLocation } from '../types/SourceLocation';
import type { Span } from '../types/Span';
import type { StackFrame } from '../types/StackFrame';
import type { StructuredLog } from '../types/StructuredLog';
import type { TimeoutValue } from '../types/TimeoutValue';
import { isTransparentBif } from './bif';
import { foldCloseIndex } from './flatten';

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
  return ancestors(data, n(event.span))
    .filter((span) => !isTransparentBif(span))
    .map(toStackFrame);
}

// Selecting a span shows the **outer** scope (vars/captures at the
// moment the span opened), so the call stack should also be the chain
// of callers - i.e. ancestors *excluding* the span itself.
export function buildCallStackForSpan(data: StructuredLog, span: Span): StackFrame[] {
  if (span.parent === null) return [];
  return ancestors(data, n(span.parent))
    .filter((s) => !isTransparentBif(s))
    .map(toStackFrame);
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
  // When `tailSplit` is omitted, the rendered layout is:
  //   [consumed gray] [matched highlight if any] [tail plain]
  // When `tailSplit` is set (per-pattern-done selection), the matched
  // highlight sits inside the tail at the split point; the layout is:
  //   [consumed gray] [tailSplit.tailBefore plain]
  //   [matched highlight] [tailSplit.tailAfter plain]
  // In the split case, `tail === tailSplit.tailBefore + matched.bytes
  // + tailSplit.tailAfter` so consumers that ignore the split (e.g.
  // the search-buffer text join) still see coherent contents.
  tail: string;
  tailSplit?: { tailBefore: string; tailAfter: string };
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
// Returns the shell *marker* (stable identity) to use for buffer-pane
// lookup for a span. Display name comes from `data.shells[marker].name`.
//
//   shell-block   -> marker from the first inner event
//   cleanup-block -> the marker of the first event inside the span;
//                    the runtime always spins a fresh implicit shell
//                    named `__cleanup`, but its marker is unique per
//                    owning effect (or per test for top-level cleanup)
//   fn-call       -> marker from the first event inside the subtree
//                    (the function executes in its caller's shell);
//                    falls back to the shell active at the call's
//                    start_ts when the body is purely pure and emits
//                    no shell-tagged events.
//   effect-setup / effect-cleanup -> marker from the first event in
//                    the subtree, when present
// Returns null when no event inside the subtree carries a shell.
export function spanBufferKey(data: StructuredLog, span: Span): string | null {
  if (
    span.kind !== 'shell-block' &&
    span.kind !== 'cleanup-block' &&
    span.kind !== 'fn-call' &&
    span.kind !== 'effect-setup' &&
    span.kind !== 'effect-cleanup'
  ) {
    return null;
  }
  const subtree = new Set<SpanId>([n(span.id), ...descendants(data, n(span.id))]);
  for (const ev of data.events) {
    if (subtree.has(n(ev.span)) && ev.shell_marker !== null) return ev.shell_marker;
  }
  if (span.kind === 'fn-call') {
    return activeShellMarkerAtTs(data, span.start_ts);
  }
  return null;
}

function activeShellMarkerAtTs(
  data: StructuredLog,
  ts: number,
): string | null {
  let marker: string | null = null;
  for (const ev of data.events) {
    if (ev.ts > ts) break;
    if (ev.shell_marker !== null) marker = ev.shell_marker;
  }
  return marker;
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

  if (
    span.kind === 'shell-block' ||
    span.kind === 'cleanup-block' ||
    span.kind === 'effect-setup' ||
    span.kind === 'effect-cleanup'
  ) {
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

/// Index over a StructuredLog's events and buffer events that captures
/// the multimatch observation-vs-drain rule:
///
///   - `observationSeqs`: every `Matched` buffer-event seq that was
///     emitted inside a `multi-match` span. Replay treats these as
///     observations (no cursor advance on their own).
///   - `endBySeq`: keyed by every multi-match block-end event seq
///     (`multi-match-done` and `multi-match-timeout`). On a done, the
///     value carries the bytes-to-drop count and the `Matched` whose
///     pattern ended farthest. On a timeout, the value is `null` -
///     no drain, but the loop still uses the entry to clear any active
///     observation highlight (whose bytes are still in `tail` and would
///     otherwise render twice).
///
/// Built once per StructuredLog in `ViewerState`'s constructor; reused
/// across every per-marker buffer replay.
export interface MultiMatchIndex {
  readonly observationSeqs: ReadonlySet<number>;
  readonly endBySeq: ReadonlyMap<
    number,
    { advanceBytes: number; matchedSeq: number } | null
  >;
}

export function buildMultiMatchIndex(data: StructuredLog): MultiMatchIndex {
  const multiMatchSpanIds = new Set<SpanId>();
  const spans = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(spans)) {
    const span = spans[key];
    if (span && span.kind === 'multi-match') multiMatchSpanIds.add(n(span.id));
  }
  if (multiMatchSpanIds.size === 0) {
    return { observationSeqs: new Set(), endBySeq: new Map() };
  }

  // Per-pattern Matched buffer events: the multi-match-pattern-done
  // structured event points at the Matched buffer event by seq.
  const observationSeqs = new Set<number>();
  for (const ev of data.events) {
    if (ev.kind !== 'multi-match-pattern-done') continue;
    if (!multiMatchSpanIds.has(n(ev.span))) continue;
    observationSeqs.add(n(ev.buffer_seq));
  }

  // Look up Matched buffer events by seq so the drain pass can resolve
  // `len(before) + len(matched)` in O(1).
  const matchedBySeq = new Map<number, { before: string; matched: string }>();
  for (const bev of data.buffer_events) {
    if (bev.kind !== 'matched') continue;
    matchedBySeq.set(n(bev.seq), { before: bev.before, matched: bev.matched });
  }

  const endBySeq = new Map<
    number,
    { advanceBytes: number; matchedSeq: number } | null
  >();
  for (const ev of data.events) {
    if (!multiMatchSpanIds.has(n(ev.span))) continue;
    if (ev.kind === 'multi-match-done') {
      const advanceMatched = matchedBySeq.get(n(ev.advance_to));
      if (advanceMatched === undefined) continue;
      endBySeq.set(n(ev.seq), {
        advanceBytes: advanceMatched.before.length + advanceMatched.matched.length,
        matchedSeq: n(ev.advance_to),
      });
    } else if (ev.kind === 'multi-match-timeout') {
      endBySeq.set(n(ev.seq), null);
    }
  }

  return { observationSeqs, endBySeq };
}

type ReplayStep =
  | { kind: 'buffer'; event: BufferEvent }
  | { kind: 'multi-end'; seq: number; advance: { advanceBytes: number; matchedSeq: number } | null };

function buildReplaySteps(
  data: StructuredLog,
  multiMatchIndex: MultiMatchIndex,
): ReplayStep[] {
  const steps: ReplayStep[] = [];
  const buffer = data.buffer_events;
  const ends = Array.from(multiMatchIndex.endBySeq.entries())
    .map(([seq, advance]) => ({ seq, advance }))
    .sort((a, b) => a.seq - b.seq);
  let bi = 0;
  let ai = 0;
  while (bi < buffer.length || ai < ends.length) {
    const bSeq = bi < buffer.length ? n(buffer[bi]!.seq) : Infinity;
    const aSeq = ai < ends.length ? ends[ai]!.seq : Infinity;
    if (bSeq <= aSeq) {
      steps.push({ kind: 'buffer', event: buffer[bi]! });
      bi++;
    } else {
      steps.push({ kind: 'multi-end', seq: aSeq, advance: ends[ai]!.advance });
      ai++;
    }
  }
  return steps;
}

export function replayBufferRegionsAtMarker(
  data: StructuredLog,
  seq: number,
  marker: string,
  multiMatchIndex?: MultiMatchIndex,
): BufferRegions {
  const idx: MultiMatchIndex =
    multiMatchIndex ?? { observationSeqs: new Set(), endBySeq: new Map() };
  const steps = buildReplaySteps(data, idx);

  let consumed = '';
  let matched: { bytes: string; seq: number } | null = null;
  let tail = '';

  for (const step of steps) {
    if (step.kind === 'multi-end') {
      if (step.seq > seq) break;
      // An observation highlight never moved bytes out of `tail`. On
      // both block-end paths the right move is to drop it (folding
      // would double-count - the bytes are either still in `tail` on
      // timeout or about to be drained on done). A non-observation
      // highlight (a regular match active before the block) represents
      // bytes already removed from `tail`, so fold it into consumed.
      if (matched !== null) {
        if (!idx.observationSeqs.has(matched.seq)) {
          consumed += matched.bytes;
        }
        matched = null;
      }
      if (step.advance !== null) {
        const toDrop = step.advance.advanceBytes;
        if (toDrop <= tail.length) {
          consumed += tail.slice(0, toDrop);
          tail = tail.slice(toDrop);
        } else {
          consumed += tail;
          tail = '';
        }
      }
      continue;
    }
    const ev = step.event;
    if (n(ev.seq) > seq) break;
    if (ev.shell_marker !== marker) continue;
    switch (ev.kind) {
      case 'grew':
        tail += ev.data;
        break;
      case 'matched': {
        if (idx.observationSeqs.has(n(ev.seq))) {
          // Observation-only: replace the highlight but do not advance.
          // For the previous highlight (if any):
          //   - if it was itself an observation, its bytes are still
          //     in tail; drop without folding (folding double-counts).
          //   - if it was a regular match-done, those bytes were
          //     drained out of tail at that time; fold into consumed
          //     so they don't vanish from the rendered history.
          if (matched !== null && !idx.observationSeqs.has(matched.seq)) {
            consumed += matched.bytes;
          }
          matched = { bytes: ev.matched, seq: n(ev.seq) };
          break;
        }
        if (matched !== null) consumed += matched.bytes;
        consumed += ev.before;
        matched = { bytes: ev.matched, seq: n(ev.seq) };
        tail = ev.after;
        break;
      }
      case 'reset': {
        if (matched !== null) consumed += matched.bytes;
        consumed += ev.consumed;
        matched = null;
        tail = tail.length > ev.consumed.length ? tail.slice(ev.consumed.length) : '';
        break;
      }
    }
  }
  return { consumed, matched, tail };
}

/// Buffer-state view for a `multi-match-pattern-done` event selection.
/// Same shape as the regular replay, but populates `tailSplit` so the
/// renderer can place the matched highlight inside the still-undrained
/// tail at the observation's split point. The `before` bytes (which
/// regular `match-done` would fold into `consumed`) stay in the tail
/// region - they were never drained.
///
/// For shells other than the event's own, falls back to the regular
/// replay at the event's seq.
export function replayBufferRegionsAtPerPatternDone(
  data: StructuredLog,
  ev: Event,
  marker: string,
  multiMatchIndex?: MultiMatchIndex,
): BufferRegions {
  if (ev.kind !== 'multi-match-pattern-done' || ev.shell_marker !== marker) {
    return replayBufferRegionsAtMarker(data, n(ev.seq), marker, multiMatchIndex);
  }
  const targetBufferSeq = n(ev.buffer_seq);
  let targetBev: BufferEvent | null = null;
  for (const bev of data.buffer_events) {
    if (n(bev.seq) === targetBufferSeq) {
      targetBev = bev;
      break;
    }
  }
  if (targetBev === null || targetBev.kind !== 'matched') {
    return replayBufferRegionsAtMarker(data, n(ev.seq), marker, multiMatchIndex);
  }

  // Reconstruct the buffer state at the instant just before the
  // observation's Matched buffer event. Drop any previously-active
  // highlight - the renderer can only show one at a time, and the
  // per-pattern's match is what we want to display.
  //
  // Crucially: an observation highlight (an earlier per-pattern-done
  // in this same block) did NOT drain bytes out of `tail`, so folding
  // its bytes into `consumed` would double-count - the same bytes are
  // about to reappear inside the new tail. Only fold non-observation
  // highlights (a regular match-done before the block), whose bytes
  // really were removed from `tail`.
  const base = replayBufferRegionsAtMarker(
    data,
    targetBufferSeq - 1,
    marker,
    multiMatchIndex,
  );
  let consumed = base.consumed;
  if (base.matched !== null) {
    const isObservation =
      multiMatchIndex !== undefined &&
      multiMatchIndex.observationSeqs.has(base.matched.seq);
    if (!isObservation) consumed += base.matched.bytes;
  }

  const fullTail = targetBev.before + targetBev.matched + targetBev.after;
  return {
    consumed,
    matched: { bytes: targetBev.matched, seq: targetBufferSeq },
    tail: fullTail,
    tailSplit: {
      tailBefore: targetBev.before,
      tailAfter: targetBev.after,
    },
  };
}

/// Resolve the matched substring on the `Matched` buffer event referenced
/// by a `multi-match-pattern-done` event's `buffer_seq`. Returns null if
/// the event isn't a pattern-done or the referenced buffer event is
/// missing (defensive only; the runtime always pairs them).
export function patternMatchedTextFor(
  data: StructuredLog,
  ev: Event,
): string | null {
  if (ev.kind !== 'multi-match-pattern-done') return null;
  const targetSeq = n(ev.buffer_seq);
  for (const bev of data.buffer_events) {
    if (n(bev.seq) !== targetSeq) continue;
    if (bev.kind !== 'matched') return null;
    return bev.matched;
  }
  return null;
}

export type MultiMatchTerminal = 'done' | 'timeout' | 'pending';

export interface MultiMatchPatternRow {
  index: number;
  status: 'matched' | 'not-seen';
  matched: string | null;
  elapsed: number | null;
}

export interface MultiMatchOutcome {
  patterns: MultiMatchPattern[];
  terminal: MultiMatchTerminal;
  rows: MultiMatchPatternRow[];
}

/// Aggregate the per-pattern status for a `multi-match` span: walk its
/// inner events, collect the pattern list from `MultiMatchStart`,
/// resolve matched text for every `MultiMatchPatternDone`, and label the
/// terminal state. Returns null when the span id is not a multi-match
/// span.
export function multiMatchOutcomeFor(
  data: StructuredLog,
  spanId: SpanId,
): MultiMatchOutcome | null {
  const span = spanById(data, spanId);
  if (!span || span.kind !== 'multi-match') return null;

  let patterns: MultiMatchPattern[] = [];
  const matchedByIndex = new Map<number, { matched: string | null; elapsed: number }>();
  let terminal: MultiMatchTerminal = 'pending';
  for (const ev of data.events) {
    if (n(ev.span) !== spanId) continue;
    if (ev.kind === 'multi-match-start') {
      patterns = ev.patterns;
    } else if (ev.kind === 'multi-match-pattern-done') {
      matchedByIndex.set(ev.index, {
        matched: patternMatchedTextFor(data, ev),
        elapsed: ev.elapsed,
      });
    } else if (ev.kind === 'multi-match-done') {
      terminal = 'done';
    } else if (ev.kind === 'multi-match-timeout') {
      terminal = 'timeout';
    }
  }

  const rows: MultiMatchPatternRow[] = patterns.map((_, i) => {
    const hit = matchedByIndex.get(i);
    if (hit !== undefined) {
      return { index: i, status: 'matched', matched: hit.matched, elapsed: hit.elapsed };
    }
    return { index: i, status: 'not-seen', matched: null, elapsed: null };
  });

  return { patterns, terminal, rows };
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

export interface EffectOverlayRow {
  key: string;
  value: string;
  // Sibling aliases this value was implicitly sourced from (R015), e.g.
  // `["Db"]` for a `DB_PORT` overlay value read from `Db`'s exposed var.
  // Empty when the overlay value was supplied directly (no implicit dep).
  sources: string[];
}

export interface EffectSetupProps {
  overlay: EffectOverlayRow[];
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
  const sourcesByKey = new Map<string, string[]>();
  for (const [key, alias] of span.dep_sources) {
    let bucket = sourcesByKey.get(key);
    if (!bucket) {
      bucket = [];
      sourcesByKey.set(key, bucket);
    }
    bucket.push(alias);
  }
  const overlay: EffectOverlayRow[] = span.overlay.map(([key, value]) => ({
    key,
    value,
    sources: sourcesByKey.get(key) ?? [],
  }));
  return { overlay, shellExposes, varExposes };
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

export type LiveShellState = 'pending' | 'ready' | 'busy' | 'terminated' | 'error';

export interface LiveShell {
  marker: string;
  name: string;
  command: string;
  state: LiveShellState;
}

// Approximate per-shell state by replaying every shell-lifecycle and
// match event up to `seq`.
//
//   - "pending": no `shell-spawn` event for the shell has fired by
//                `seq` yet - the shell will exist later in the log but
//                hasn't started at the cursor moment. Without this the
//                default `ready` (rendered as "running") leaked for
//                shells that hadn't been spawned yet, surfacing
//                spawn-in-the-future shells as live.
//   - "terminated": shell had a `shell-terminate` event at-or-before `seq`.
//   - "busy"   : most recent `match-start` for the shell at-or-before seq
//                has no corresponding `match-done` yet.
//   - "ready"  : otherwise (post-`shell-ready`, idle prompt).
//
// Returns one entry per shell in `data.shells`, keyed by marker.
function liveShellsAtSeqNumber(data: StructuredLog, seq: number): LiveShell[] {
  const out: LiveShell[] = [];
  const records = data.shells as unknown as Record<
    string,
    | { marker: string; name: string; command: string; spawn_ts: number; terminate_ts: number | null }
    | undefined
  >;
  for (const marker of Object.keys(records)) {
    const rec = records[marker];
    if (!rec) continue;
    let state: LiveShellState = 'ready';
    let spawned = false;
    let busy = false;
    let ended = false;
    for (const ev of data.events) {
      if (n(ev.seq) > seq) break;
      if (ev.shell_marker !== marker) continue;
      if (ev.kind === 'shell-spawn') spawned = true;
      else if (ev.kind === 'shell-terminate') ended = true;
      else if (ev.kind === 'match-start') busy = true;
      else if (ev.kind === 'match-done' || ev.kind === 'timeout') busy = false;
    }
    if (!spawned) state = 'pending';
    else if (ended) state = 'terminated';
    else if (busy) state = 'busy';
    out.push({ marker, name: rec.name, command: rec.command, state });
  }
  return out;
}

// Per-shell state at the moment of `event` (i.e. as of `event.seq`).
export function liveShellsAtSeq(data: StructuredLog, event: Event): LiveShell[] {
  return liveShellsAtSeqNumber(data, n(event.seq));
}

// Per-shell state for a span selection: anchor the replay on the latest
// event whose `ts` falls inside the span's lifetime. Used when the user
// has a span (not an event) selected - `liveShellsAtSeq` needs a seq,
// and a span's natural anchor is "the latest activity within it". For
// in-flight spans (`end_ts === null`) the anchor walks to the very end
// of the log. Returns `[]` when no event qualifies (a span before any
// events fired - unusual but well-defined).
export function liveShellsAtSpan(data: StructuredLog, span: Span): LiveShell[] {
  const endTs = span.end_ts ?? Number.POSITIVE_INFINITY;
  let anchorSeq: number | null = null;
  for (const ev of data.events) {
    if (ev.ts > endTs) break;
    anchorSeq = n(ev.seq);
  }
  if (anchorSeq === null) return [];
  return liveShellsAtSeqNumber(data, anchorSeq);
}

/// Return the bootstrap setup span's id for a given marker, or null if
/// no such span exists. By construction at most one bootstrap span
/// exists per marker per test (the setup body runs at most once).
export function bootstrapForReuse(data: StructuredLog, marker: string): SpanId | null {
  const map = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(map)) {
    const span = map[key];
    if (
      span &&
      span.kind === 'effect-setup' &&
      span.marker === marker &&
      span.is_reuse === false
    ) {
      return n(span.id);
    }
  }
  return null;
}

/// Return the final cleanup span's id for a given marker, or null if
/// no such span exists. By construction at most one final cleanup span
/// exists per marker per test (refcount hits zero at most once).
export function finalCleanupForDeferred(
  data: StructuredLog,
  marker: string,
): SpanId | null {
  const map = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(map)) {
    const span = map[key];
    if (
      span &&
      span.kind === 'effect-cleanup' &&
      span.marker === marker &&
      span.is_deferred === false
    ) {
      return n(span.id);
    }
  }
  return null;
}

/// Return the `shell-block` span whose first inner event is
/// `shell-spawn` for the given marker, or null if none exists. By
/// construction at most one first-use block exists per marker (one
/// PTY = one spawn).
export function firstUseShellBlockForMarker(
  data: StructuredLog,
  marker: string,
): SpanId | null {
  const map = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(map)) {
    const span = map[key];
    if (!span || span.kind !== 'shell-block') continue;
    for (const ev of data.events) {
      if (n(ev.span) !== n(span.id)) continue;
      if (ev.kind === 'shell-spawn' && ev.shell_marker === marker) {
        return n(span.id);
      }
      // First event in span - if not a marker-matching shell-spawn,
      // move on to the next shell-block.
      break;
    }
  }
  return null;
}

/**
 * Returns the source byte range to highlight for the current selection.
 *
 * - Selected span -> span.location.
 * - Selected event -> event.source if present, else its parent span's
 *   location.
 * - Folded event row -> union of the two halves' ranges
 *   (`file = lead.file`, `start = min`, `end = max`).
 * - null otherwise.
 *
 * Cross-file folded halves can't be merged into a single range; if
 * that invariant ever breaks (it shouldn't by construction), the
 * lead's source is returned and the wider range is silently dropped.
 */
export function selectionSourceRange(
  data: StructuredLog,
  selectedSpanId: SpanId | null,
  selectedEventSeq: number | null,
): SourceLocation | null {
  if (selectedSpanId !== null) {
    const span = spanById(data, selectedSpanId);
    return span?.location ?? null;
  }
  if (selectedEventSeq !== null) {
    const idx = data.events.findIndex((e) => n(e.seq) === selectedEventSeq);
    if (idx < 0) return null;
    const lead = data.events[idx]!;
    const closeIdx = foldCloseIndex(data.events, idx);
    if (closeIdx > idx) {
      const close = data.events[closeIdx]!;
      const a = lead.source;
      const b = close.source;
      if (a && b && a.file === b.file) {
        return {
          file: a.file,
          line: a.line,
          start: Math.min(a.start, b.start),
          end: Math.max(a.end, b.end),
        };
      }
      if (a) return a;
      if (b) return b;
    }
    if (lead.source) return lead.source;
    const parent = spanById(data, n(lead.span));
    return parent?.location ?? null;
  }
  return null;
}

export { n as toNumber };
