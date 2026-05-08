import type { Event } from '../types/Event';
import type { StructuredLog } from '../types/StructuredLog';
import {
  ancestors,
  buildCallStack,
  eventBySeq,
  replayBufferAtSeq,
  replayShellCtxAtSeq,
  replayVarsAtSeq,
  toNumber as n,
  type ShellContextSnapshot,
  type SpanId,
} from './derive';
import { flattenRows, type Row } from './flatten';

export class ViewerState {
  readonly data: StructuredLog;

  selectedEventSeq = $state<number | null>(null);
  expandedSpans = $state<Set<SpanId>>(new Set());
  envModalOpen = $state(false);

  rows = $derived<Row[]>(flattenRows(this.data, this.expandedSpans));

  selected = $derived<Event | null>(
    this.selectedEventSeq === null ? null : eventBySeq(this.data, this.selectedEventSeq),
  );

  callStack = $derived(this.selected ? buildCallStack(this.data, this.selected) : []);

  bufferAt = $derived<Map<string, string>>(
    this.selected ? replayBufferAtSeq(this.data, n(this.selected.seq)) : new Map(),
  );

  varsAt = $derived<Map<string, string>>(
    this.selected ? replayVarsAtSeq(this.data, n(this.selected.seq)) : new Map(),
  );

  shellContext = $derived<ShellContextSnapshot | null>(
    this.selected ? replayShellCtxAtSeq(this.data, n(this.selected.seq)) : null,
  );

  constructor(data: StructuredLog) {
    this.data = data;

    const initial = new Set<SpanId>();
    // Root test span is always expanded so its direct events are visible.
    for (const key of Object.keys(data.spans)) {
      const span = (data.spans as unknown as Record<string, { kind: string; id: bigint }>)[key];
      if (span && span.kind === 'test') {
        initial.add(n(span.id));
      }
    }

    if (data.failure && data.failure.type !== 'runtime' && data.failure.type !== 'cancelled') {
      this.selectedEventSeq = n(data.failure.event_seq);
      for (const ancestor of ancestors(this.data, n(data.failure.span))) {
        initial.add(n(ancestor.id));
      }
    } else if (data.failure && data.failure.event_seq !== null && data.failure.span !== null) {
      this.selectedEventSeq = n(data.failure.event_seq);
      for (const ancestor of ancestors(this.data, n(data.failure.span))) {
        initial.add(n(ancestor.id));
      }
    }

    this.expandedSpans = initial;
  }

  selectEvent(seq: number): void {
    this.selectedEventSeq = seq;
  }

  toggleSpan(id: SpanId): void {
    const next = new Set(this.expandedSpans);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.expandedSpans = next;
  }

  openEnv(): void {
    this.envModalOpen = true;
  }

  closeEnv(): void {
    this.envModalOpen = false;
  }
}
