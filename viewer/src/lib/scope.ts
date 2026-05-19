import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import { isTransparentBif } from './bif';
import { spanById, toNumber as n, type SpanId } from './derive';

// Walk the ancestor chain of `spanId` and return:
//   ambientScope - innermost Test or EffectSetup ancestor (the runtime
//                  `Scope` that frames this event); null if none found.
//   innermostFn  - innermost FnCall ancestor encountered *before* the
//                  ambient scope (i.e. between the event and its
//                  scope); null if the event executes directly in the
//                  ambient scope.
//
// Walking starts at `spanId` itself, so when called on a Test or
// EffectSetup span, `ambientScope` returns that span's id.
//
// `effect-cleanup` ancestors are special: the runtime parents them
// directly under the test span (not the long-closed `effect-setup`),
// but the cleanup VM still runs with the *effect's* `Scope`. We honour
// that by hopping to `effect-cleanup.setup_span` — which by
// construction is an `effect-setup` id — and returning it as
// `ambientScope`.
export function scopeContext(
  data: StructuredLog,
  spanId: SpanId,
): { ambientScope: SpanId | null; innermostFn: SpanId | null } {
  let current: Span | null = spanById(data, spanId);
  let innermostFn: SpanId | null = null;
  while (current) {
    if (current.kind === 'test' || current.kind === 'effect-setup') {
      return { ambientScope: n(current.id), innermostFn };
    }
    if (current.kind === 'effect-cleanup') {
      return { ambientScope: n(current.setup_span), innermostFn };
    }
    if (current.kind === 'fn-call' && !isTransparentBif(current) && innermostFn === null) {
      innermostFn = n(current.id);
    }
    if (current.parent === null) break;
    current = spanById(data, n(current.parent));
  }
  return { ambientScope: null, innermostFn };
}

// Stateful forward replay. Walks events with `seq <= cutoffSeq`,
// maintaining per-scope, per-shell, and per-fn-frame maps, then
// projects the maintained state through the viewer's perspective
// (`viewerSpanId` + `viewerShell`). The runtime semantics are mirrored
// from `crates/relux-runtime/src/vm/context.rs:195-218`:
//   - Inside a fn-call frame: hard barrier. Visible vars = frame args
//     plus var-let / var-assign emitted in that frame.
//   - In shell context: ambient scope vars (test-level or effect-level)
//     unioned with shell-local lets; the shell-local entries shadow.
function varsAtCutoff(
  data: StructuredLog,
  cutoffSeq: number,
  viewerSpanId: SpanId,
  viewerShell: string | null,
): Map<string, string> {
  const scopeVars = new Map<SpanId, Map<string, string>>();
  const shellVars = new Map<string, Map<string, string>>();
  const frameVars = new Map<SpanId, Map<string, string>>();

  // Pre-seed each fn-call frame with its declared args. This way an
  // event selected at the very first statement inside a function
  // already shows the arguments.
  const spans = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(spans)) {
    const span = spans[key];
    if (!span || span.kind !== 'fn-call') continue;
    if (isTransparentBif(span)) continue;
    const seed = new Map<string, string>();
    for (const [k, v] of span.args) seed.set(k, v);
    frameVars.set(n(span.id), seed);
  }

  const ensureScope = (id: SpanId): Map<string, string> => {
    let m = scopeVars.get(id);
    if (!m) {
      m = new Map();
      scopeVars.set(id, m);
    }
    return m;
  };
  const ensureShell = (name: string): Map<string, string> => {
    let m = shellVars.get(name);
    if (!m) {
      m = new Map();
      shellVars.set(name, m);
    }
    return m;
  };

  for (const ev of data.events) {
    if (n(ev.seq) > cutoffSeq) break;
    const ctx = scopeContext(data, n(ev.span));

    switch (ev.kind) {
      case 'var-let': {
        if (ctx.innermostFn !== null) {
          const m = frameVars.get(ctx.innermostFn);
          if (m) m.set(ev.name, ev.value);
        } else if (ev.shell !== null) {
          ensureShell(ev.shell).set(ev.name, ev.value);
        } else if (ctx.ambientScope !== null) {
          ensureScope(ctx.ambientScope).set(ev.name, ev.value);
        }
        break;
      }
      case 'var-assign': {
        let landed = false;
        if (ctx.innermostFn !== null) {
          const m = frameVars.get(ctx.innermostFn);
          if (m && m.has(ev.name)) {
            m.set(ev.name, ev.value);
            landed = true;
          }
        }
        if (!landed && ev.shell !== null) {
          const m = shellVars.get(ev.shell);
          if (m && m.has(ev.name)) {
            m.set(ev.name, ev.value);
            landed = true;
          }
        }
        if (!landed && ctx.ambientScope !== null) {
          const m = scopeVars.get(ctx.ambientScope);
          if (m && m.has(ev.name)) {
            m.set(ev.name, ev.value);
          }
        }
        break;
      }
      case 'effect-expose-var': {
        // Emitted by the exposing effect's setup span. The runtime
        // injects this var into the *parent* scope under
        // `<parent's-alias-for-this-effect>.<exposed-name>`. The
        // emitting span's `alias` field (mirrors the parent's
        // `start ... as <alias>`) is the qualifier; without it the
        // runtime does not inject, so neither do we.
        const emitter = spanById(data, n(ev.span));
        if (!emitter || emitter.kind !== 'effect-setup') break;
        if (emitter.alias === null) break;
        if (emitter.parent === null) break;
        const parentCtx = scopeContext(data, n(emitter.parent));
        if (parentCtx.ambientScope === null) break;
        ensureScope(parentCtx.ambientScope).set(
          `${emitter.alias}.${ev.name}`,
          ev.value,
        );
        break;
      }
    }
  }

  // Project the maintained state through the viewer's perspective.
  const viewCtx = scopeContext(data, viewerSpanId);
  if (viewCtx.innermostFn !== null) {
    return new Map(frameVars.get(viewCtx.innermostFn) ?? []);
  }
  const out = new Map<string, string>();
  if (viewCtx.ambientScope !== null) {
    const m = scopeVars.get(viewCtx.ambientScope);
    if (m) for (const [k, v] of m) out.set(k, v);
  }
  if (viewerShell !== null) {
    const m = shellVars.get(viewerShell);
    if (m) for (const [k, v] of m) out.set(k, v);
  }
  return out;
}

// Variables visible at the time of the selected event.
export function varsAtSeq(
  data: StructuredLog,
  selected: Event,
): Map<string, string> {
  return varsAtCutoff(data, n(selected.seq), n(selected.span), selected.shell);
}

// Variables visible to a selected span — i.e. the *outer* scope the
// span sits in, sampled at the moment the span opens.
//
//   shell-block / cleanup-block -> the ambient test/effect scope.
//   fn-call called from a shell -> the caller shell's scope (ambient
//                                   vars plus shell-local lets).
//   fn-call called from another fn-call -> the outer fn's frame.
//   fn-call called directly from a test or effect-setup -> null (the
//     caller is a pure-init context outside any shell/fn; vars from
//     that context aren't reachable via the structured event stream).
//
// Returns `null` to mean "not applicable, render the empty hint";
// returns an empty map to mean "applicable but currently empty".
export function varsAtSpan(
  data: StructuredLog,
  span: Span,
): Map<string, string> | null {
  if (span.kind !== 'shell-block' && span.kind !== 'cleanup-block' && span.kind !== 'fn-call') {
    return null;
  }
  const ctx = outerContextForSpan(data, span);
  if (ctx === null) return null;
  return varsAtCutoff(data, ctx.cutoffSeq, ctx.viewerSpanId, ctx.viewerShell);
}

// Captures for the selected event. A `match-done` rewrites all
// captures at once on the active frame; subsequent matches that set
// fewer groups erase the prior ones. Frames isolate captures: a
// fn-call's match-dones are not visible after pop, and outer-shell
// captures are not visible while inside a fn-call.
export function capturesAtSeq(
  data: StructuredLog,
  selected: Event,
  shell: string | null,
): Map<string, string> {
  return capturesAtCutoff(data, n(selected.seq), n(selected.span), shell);
}

// Captures visible to a selected span. Mirrors `varsAtSpan`: returns
// null when "not applicable", otherwise the captures from the outer
// scope at the moment the span opens.
export function capturesAtSpan(
  data: StructuredLog,
  span: Span,
): Map<string, string> | null {
  if (span.kind !== 'shell-block' && span.kind !== 'cleanup-block' && span.kind !== 'fn-call') {
    return null;
  }
  const ctx = outerContextForSpan(data, span);
  if (ctx === null) return null;
  return capturesAtCutoff(data, ctx.cutoffSeq, ctx.viewerSpanId, ctx.viewerShell);
}

function capturesAtCutoff(
  data: StructuredLog,
  cutoffSeq: number,
  viewerSpanId: SpanId,
  viewerShell: string | null,
): Map<string, string> {
  if (viewerShell === null) return new Map();
  const viewCtx = scopeContext(data, viewerSpanId);
  for (let i = data.events.length - 1; i >= 0; i--) {
    const ev = data.events[i]!;
    if (n(ev.seq) > cutoffSeq) continue;
    if (ev.kind !== 'match-done' || !ev.captures) continue;
    if (ev.shell !== viewerShell) continue;
    const evCtx = scopeContext(data, n(ev.span));
    if (evCtx.innermostFn !== viewCtx.innermostFn) continue;
    const out = new Map<string, string>();
    for (const [k, v] of Object.entries(ev.captures)) {
      if (v !== undefined) out.set(k, v);
    }
    return out;
  }
  return new Map();
}

// Resolve the outer-scope view for a span: where to anchor the
// `scopeContext` projection and the seq cutoff. Returns null when the
// outer scope isn't a context the variables panel surfaces (e.g. a
// fn-call invoked directly from test-level let-init).
function outerContextForSpan(
  data: StructuredLog,
  span: Span,
): { viewerSpanId: SpanId; viewerShell: string | null; cutoffSeq: number } | null {
  const cutoffSeq = firstSeqInSubtree(data, n(span.id));

  if (span.kind === 'shell-block' || span.kind === 'cleanup-block') {
    // Outer = the test or effect span containing this block.
    if (span.parent === null) return null;
    const parentCtx = scopeContext(data, n(span.parent));
    if (parentCtx.ambientScope === null) return null;
    return {
      viewerSpanId: parentCtx.ambientScope,
      viewerShell: null,
      cutoffSeq,
    };
  }

  // fn-call: walk up until we find the caller context.
  let parent: Span | null = span.parent === null ? null : spanById(data, n(span.parent));
  while (parent) {
    if (parent.kind === 'shell-block') {
      return {
        viewerSpanId: n(parent.id),
        viewerShell: parent.shell,
        cutoffSeq,
      };
    }
    if (parent.kind === 'cleanup-block') {
      return {
        viewerSpanId: n(parent.id),
        viewerShell: null,
        cutoffSeq,
      };
    }
    if (parent.kind === 'fn-call' && !isTransparentBif(parent)) {
      return {
        viewerSpanId: n(parent.id),
        viewerShell: null,
        cutoffSeq,
      };
    }
    if (parent.kind === 'test' || parent.kind === 'effect-setup') {
      // Pure-init context, not reachable through the event stream.
      return null;
    }
    parent = parent.parent === null ? null : spanById(data, n(parent.parent));
  }
  return null;
}

// Seq of the last event that fired *before* `spanId` opens. Used by
// `outerContextForSpan` as the replay cutoff — variables/captures
// declared at or after the span are excluded.
//
// Two cases:
//   - Span has events in its subtree (typical fn-call): use the first
//     event's `seq - 1`. The event itself is the span's earliest visible
//     activity; everything strictly before it is the outer scope.
//   - Span has no events (a flattened pure BIF like `rand` or `trim`,
//     whose `FnCall` span exists but emits nothing): fall back to the
//     latest event whose `ts` predates the span's `start_ts`. Without
//     this fallback the cutoff defaults to "the entire log" and the
//     vars panel surfaces let-bindings declared *after* the BIF call.
//     Returns `-1` when no event predates the span at all, which makes
//     `varsAtCutoff` replay nothing (the right answer).
function firstSeqInSubtree(data: StructuredLog, spanId: SpanId): number {
  const subtree = subtreeIds(data, spanId);
  for (const ev of data.events) {
    if (subtree.has(n(ev.span))) {
      return n(ev.seq) - 1;
    }
  }
  const span = spanById(data, spanId);
  if (!span) return Number.MAX_SAFE_INTEGER;
  let last = -1;
  for (const ev of data.events) {
    if (ev.ts >= span.start_ts) break;
    last = n(ev.seq);
  }
  return last;
}

function subtreeIds(data: StructuredLog, root: SpanId): Set<SpanId> {
  const childrenByParent = new Map<SpanId, SpanId[]>();
  const map = data.spans as unknown as Record<string, Span | undefined>;
  for (const key of Object.keys(map)) {
    const s = map[key];
    if (!s || s.parent === null) continue;
    const p = n(s.parent);
    let bucket = childrenByParent.get(p);
    if (!bucket) {
      bucket = [];
      childrenByParent.set(p, bucket);
    }
    bucket.push(n(s.id));
  }
  const out = new Set<SpanId>([root]);
  const stack: SpanId[] = [root];
  while (stack.length > 0) {
    const id = stack.pop()!;
    const kids = childrenByParent.get(id);
    if (!kids) continue;
    for (const k of kids) {
      if (out.has(k)) continue;
      out.add(k);
      stack.push(k);
    }
  }
  return out;
}
