import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StackFrame } from '../types/StackFrame';
import type { StructuredLog } from '../types/StructuredLog';

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

export function replayBufferAtSeq(
  data: StructuredLog,
  seq: number,
): Map<string, string> {
  const buffers = new Map<string, string>();
  for (const ev of data.buffer_events) {
    if (n(ev.seq) > seq) break;
    const current = buffers.get(ev.shell) ?? '';
    switch (ev.kind) {
      case 'grew':
        buffers.set(ev.shell, current + ev.data);
        break;
      case 'matched':
        buffers.set(ev.shell, ev.after);
        break;
      case 'reset':
        buffers.set(ev.shell, '');
        break;
    }
  }
  return buffers;
}

export function replayVarsAtSeq(
  data: StructuredLog,
  seq: number,
): Map<string, string> {
  const vars = new Map<string, string>();
  for (const ev of data.events) {
    if (n(ev.seq) > seq) break;
    if (ev.kind === 'var-let' || ev.kind === 'var-assign') {
      vars.set(ev.name, ev.value);
    }
  }
  return vars;
}

export interface ShellContextSnapshot {
  failPatterns: string[];
  timeout: string | null;
  activeShell: string | null;
}

export function replayShellCtxAtSeq(
  data: StructuredLog,
  seq: number,
): ShellContextSnapshot {
  const failPatterns: string[] = [];
  let timeout: string | null = null;
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

export { n as toNumber };
