import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import {
  ancestors,
  buildCallStack,
  eventBySeq,
  liveShellsAtSeq,
  replayBufferRegionsAtSeq,
  replayCapturesAtSeq,
  replayShellCtxAtSeq,
  replayVarsAtSeq,
  spanBufferCutoffSeq,
  spanBufferShell,
  spanById,
  toNumber as n,
  type BufferRegions,
  type LiveShell,
  type ShellContextSnapshot,
  type SpanId,
} from './derive';
import { flattenRows, foldCloseIndex, type Row } from './flatten';

export type OpenModal = 'env' | 'shells' | null;
export type TreeFilter = 'all' | 'errors' | 'send-match';
export type EnvFilterScope = 'name' | 'value' | 'name-matches';

export class ViewerState {
  readonly data: StructuredLog;

  selectedEventSeq = $state<number | null>(null);
  selectedSpanId = $state<SpanId | null>(null);
  expandedSpans = $state<Set<SpanId>>(new Set());
  expandedValueRows = $state<Set<string>>(new Set());

  openModal = $state<OpenModal>(null);
  filter = $state<TreeFilter>('all');

  envFilter = $state<string>('');
  envFilterScope = $state<EnvFilterScope>('name-matches');

  rows = $derived<Row[]>(flattenRows(this.data, this.expandedSpans));

  selected = $derived<Event | null>(
    this.selectedEventSeq === null ? null : eventBySeq(this.data, this.selectedEventSeq),
  );

  selectedSpan = $derived<Span | null>(
    this.selectedSpanId === null ? null : spanById(this.data, this.selectedSpanId),
  );

  callStack = $derived(this.selected ? buildCallStack(this.data, this.selected) : []);

  bufferRegionsAt = $derived<Map<string, BufferRegions>>(this.computeBufferRegions());

  bufferShell = $derived<string | null>(this.computeBufferShell());

  varsAt = $derived<Map<string, string>>(
    this.selected ? replayVarsAtSeq(this.data, n(this.selected.seq)) : new Map(),
  );

  capturesAt = $derived<Map<string, string>>(
    this.selected
      ? replayCapturesAtSeq(this.data, n(this.selected.seq), this.selected.shell)
      : new Map(),
  );

  shellContext = $derived<ShellContextSnapshot | null>(
    this.selected ? replayShellCtxAtSeq(this.data, n(this.selected.seq)) : null,
  );

  liveShells = $derived<LiveShell[]>(
    this.selected ? liveShellsAtSeq(this.data, this.selected) : [],
  );

  constructor(data: StructuredLog) {
    this.data = data;

    const initial = new Set<SpanId>();
    for (const key of Object.keys(data.spans)) {
      const span = (data.spans as unknown as Record<string, { kind: string; id: bigint }>)[key];
      if (span && span.kind === 'test') {
        initial.add(n(span.id));
      }
    }

    if (data.failure && data.failure.event_seq !== null && data.failure.span !== null) {
      this.selectedEventSeq = n(data.failure.event_seq);
      for (const ancestor of ancestors(this.data, n(data.failure.span))) {
        initial.add(n(ancestor.id));
      }
    }

    this.expandedSpans = initial;
  }

  selectEvent(seq: number): void {
    if (this.selectedEventSeq === seq) {
      this.selectedEventSeq = null;
    } else {
      this.selectedEventSeq = seq;
      this.selectedSpanId = null;
    }
  }

  selectSpan(id: SpanId): void {
    if (this.selectedSpanId === id) {
      this.selectedSpanId = null;
    } else {
      this.selectedSpanId = id;
      this.selectedEventSeq = null;
    }
  }

  toggleSpan(id: SpanId): void {
    const next = new Set(this.expandedSpans);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.expandedSpans = next;
  }

  toggleExpandedValueRow(key: string): void {
    const next = new Set(this.expandedValueRows);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    this.expandedValueRows = next;
  }


  openEnv(): void {
    this.openModal = 'env';
  }

  closeEnv(): void {
    if (this.openModal === 'env') this.openModal = null;
  }

  openShells(): void {
    this.openModal = 'shells';
  }

  closeShells(): void {
    if (this.openModal === 'shells') this.openModal = null;
  }

  closeModal(): void {
    this.openModal = null;
  }

  private computeBufferRegions(): Map<string, BufferRegions> {
    const targetSeq = this.computeBufferTargetSeq();
    if (targetSeq === null) return new Map();

    const out = new Map<string, BufferRegions>();
    for (const name of Object.keys(this.data.shells)) {
      out.set(name, replayBufferRegionsAtSeq(this.data, targetSeq, name));
    }
    return out;
  }

  private computeBufferTargetSeq(): number | null {
    if (this.selected) {
      // The selected lead may open a fold (match-start, sleep-start,
      // shell-spawn). Walk to the close of that fold so the cutoff
      // reflects "after the match completed / sleep returned / shell
      // came up", not the moment the operation began. From there, peek
      // one more event in the same shell so bytes that arrived between
      // the close and the next operation are also visible.
      const events = this.data.events;
      const selectedSeq = n(this.selected.seq);
      let idx = -1;
      for (let i = 0; i < events.length; i++) {
        if (n(events[i]!.seq) === selectedSeq) {
          idx = i;
          break;
        }
      }
      if (idx < 0) return null;
      idx = foldCloseIndex(events, idx);
      const close = events[idx]!;
      const next = events[idx + 1];
      if (next && next.shell === close.shell) return n(next.seq);
      return n(close.seq);
    }

    if (this.selectedSpan) {
      return spanBufferCutoffSeq(this.data, this.selectedSpan);
    }

    return null;
  }

  private computeBufferShell(): string | null {
    if (this.selected) return this.selected.shell;
    if (this.selectedSpan) return spanBufferShell(this.data, this.selectedSpan);
    return null;
  }
}
