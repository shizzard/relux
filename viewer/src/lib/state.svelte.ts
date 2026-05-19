import type { Event } from '../types/Event';
import type { Span } from '../types/Span';
import type { StructuredLog } from '../types/StructuredLog';
import {
  ancestors,
  buildCallStack,
  buildCallStackForSpan,
  eventBySeq,
  liveShellsAtSeq,
  liveShellsAtSpan,
  replayBufferRegionsAtMarker,
  replayShellCtxAtSeq,
  spanBufferCutoffSeq,
  spanBufferKey,
  spanById,
  toNumber as n,
  type BufferRegions,
  type LiveShell,
  type ShellContextSnapshot,
  type SpanId,
} from './derive';
import { capturesAtSeq, capturesAtSpan, varsAtSeq, varsAtSpan } from './scope';
import {
  ALL_EVENT_TYPE_IDS,
  flattenRows,
  foldCloseIndex,
  foldEvents,
  foldedTypeId,
  leadEvent,
  type EventTypeId,
  type Row,
} from './flatten';
import { testTimeRange, type TimeRange } from './timeline';

export type OpenModal = 'env' | 'shells' | 'filter' | 'artifacts' | null;
export type EnvFilterScope = 'name' | 'value' | 'name-matches';

const ERROR_PATH_VISIBLE: ReadonlySet<EventTypeId> = new Set<EventTypeId>([
  'error',
  'fail-pattern-triggered',
  'match-timeout',
]);

const SEND_MATCH_VISIBLE: ReadonlySet<EventTypeId> = new Set<EventTypeId>([
  'send',
  'match',
  'match-timeout',
]);

const ERROR_PATH_HIDDEN: ReadonlySet<EventTypeId> = complementOf(ERROR_PATH_VISIBLE);
const SEND_MATCH_HIDDEN: ReadonlySet<EventTypeId> = complementOf(SEND_MATCH_VISIBLE);

function complementOf(visible: ReadonlySet<EventTypeId>): ReadonlySet<EventTypeId> {
  const out = new Set<EventTypeId>();
  for (const id of ALL_EVENT_TYPE_IDS) if (!visible.has(id)) out.add(id);
  return out;
}

function setEquals<T>(a: ReadonlySet<T>, b: ReadonlySet<T>): boolean {
  if (a.size !== b.size) return false;
  for (const v of a) if (!b.has(v)) return false;
  return true;
}

export class ViewerState {
  // Definite-assignment assertion (`!`): set by the constructor before any
  // `$derived` lambda below ever runs. The runes are lazy &mdash; their bodies
  // don't execute during field initialization &mdash; but `svelte-check` can't
  // see that, so we assert here to silence the false-positive
  // "used before initialization" diagnostic.
  readonly data!: StructuredLog;

  selectedEventSeq = $state<number | null>(null);
  selectedSpanId = $state<SpanId | null>(null);
  expandedSpans = $state<Set<SpanId>>(new Set());
  expandedValueRows = $state<Set<string>>(new Set());

  openModal = $state<OpenModal>(null);
  hiddenEventTypes = $state<Set<EventTypeId>>(new Set());

  // Timeline-bar transient state. `timelineHover` is set after the 500 ms
  // hover delay fires; holds the candidate set at the cursor's percent
  // position. `timelinePin` is the click-pinned variant for multi-
  // candidate zones &mdash; stays visible until the user picks a card or
  // clicks outside. `timelineCardFocus` tracks which card the cursor is
  // currently over (drives the secondary "intensified" preview style on
  // the bar slice).
  timelineHover = $state<{ percent: number; spans: Span[] } | null>(null);
  timelinePin = $state<{ percent: number; spans: Span[] } | null>(null);
  timelineCardFocus = $state<SpanId | null>(null);

  envFilter = $state<string>('');
  envFilterScope = $state<EnvFilterScope>('name-matches');

  artifactFilter = $state<string>('');

  errorPathSpanId = $derived<SpanId | null>(
    (this.data.outcome.kind === 'fail' || this.data.outcome.kind === 'cancelled') &&
      this.data.outcome.span !== null
      ? n(this.data.outcome.span)
      : null,
  );

  hasErrorPath = $derived<boolean>(this.errorPathSpanId !== null);

  aliveSpans = $derived<Set<SpanId>>(this.computeAliveSpans());

  isErrorPathPresetActive = $derived<boolean>(
    setEquals(this.hiddenEventTypes, ERROR_PATH_HIDDEN),
  );

  isSendMatchPresetActive = $derived<boolean>(
    setEquals(this.hiddenEventTypes, SEND_MATCH_HIDDEN),
  );

  rows = $derived<Row[]>(flattenRows(this.data, this.expandedSpans));

  visibleRows = $derived<Row[]>(this.computeVisibleRows());

  selected = $derived<Event | null>(
    this.selectedEventSeq === null ? null : eventBySeq(this.data, this.selectedEventSeq),
  );

  selectedSpan = $derived<Span | null>(
    this.selectedSpanId === null ? null : spanById(this.data, this.selectedSpanId),
  );

  timeRange = $derived<TimeRange>(testTimeRange(this.data));

  callStack = $derived(
    this.selected
      ? buildCallStack(this.data, this.selected)
      : this.selectedSpan
        ? buildCallStackForSpan(this.data, this.selectedSpan)
        : [],
  );

  bufferRegionsAt = $derived<Map<string, BufferRegions>>(this.computeBufferRegions());

  bufferKey = $derived<string | null>(this.computeBufferKey());

  // `null` = "not applicable" (no event/span selected, or selected
  // context has no surfaced outer scope). Components render an empty
  // hint in that case. Empty Map = "applicable but empty".
  varsAt = $derived<Map<string, string> | null>(this.computeVarsAt());
  capturesAt = $derived<Map<string, string> | null>(this.computeCapturesAt());

  shellContext = $derived<ShellContextSnapshot | null>(
    this.selected ? replayShellCtxAtSeq(this.data, n(this.selected.seq)) : null,
  );

  liveShells = $derived<LiveShell[]>(
    this.selected
      ? liveShellsAtSeq(this.data, this.selected)
      : this.selectedSpan
        ? liveShellsAtSpan(this.data, this.selectedSpan)
        : [],
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

    if (
      data.outcome.kind === 'fail' &&
      data.outcome.event_seq !== null &&
      data.outcome.span !== null
    ) {
      this.selectedEventSeq = n(data.outcome.event_seq);
      for (const ancestor of ancestors(this.data, n(data.outcome.span))) {
        initial.add(n(ancestor.id));
      }
    } else if (
      data.outcome.kind === 'cancelled' &&
      data.outcome.event_seq !== null &&
      data.outcome.span !== null
    ) {
      this.selectedEventSeq = n(data.outcome.event_seq);
      for (const ancestor of ancestors(this.data, n(data.outcome.span))) {
        initial.add(n(ancestor.id));
      }
    } else if (data.outcome.kind === 'skip') {
      this.selectedEventSeq = n(data.outcome.event_seq);
      for (const ancestor of ancestors(this.data, n(data.outcome.span))) {
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

  // Reveal a span deep in the tree: expand every ancestor in the
  // expansion set, then select the target. Used by the marker pill's
  // jump-to-partner click handler.
  revealAndSelect(targetId: SpanId): void {
    const next = new Set(this.expandedSpans);
    for (const a of ancestors(this.data, targetId)) {
      next.add(n(a.id));
    }
    this.expandedSpans = next;
    this.selectedSpanId = targetId;
    this.selectedEventSeq = null;
  }

  toggleErrorPath(): void {
    if (!this.hasErrorPath) return;
    this.hiddenEventTypes = this.isErrorPathPresetActive
      ? new Set()
      : new Set(ERROR_PATH_HIDDEN);
  }

  toggleSendMatch(): void {
    this.hiddenEventTypes = this.isSendMatchPresetActive
      ? new Set()
      : new Set(SEND_MATCH_HIDDEN);
  }

  toggleEventType(id: EventTypeId): void {
    const next = new Set(this.hiddenEventTypes);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.hiddenEventTypes = next;
  }

  showAllEventTypes(): void {
    this.hiddenEventTypes = new Set();
  }

  hideAllEventTypes(): void {
    this.hiddenEventTypes = new Set(ALL_EVENT_TYPE_IDS);
  }

  toggleSpan(id: SpanId): void {
    const next = new Set(this.expandedSpans);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.expandedSpans = next;
  }

  collapseAll(): void {
    this.expandedSpans = new Set();
  }

  expandAll(): void {
    const next = new Set<SpanId>();
    const map = this.data.spans as unknown as Record<string, { id: bigint } | undefined>;
    for (const key of Object.keys(map)) {
      const span = map[key];
      if (span) next.add(n(span.id));
    }
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

  openArtifacts(): void {
    this.openModal = 'artifacts';
  }

  closeArtifacts(): void {
    if (this.openModal === 'artifacts') this.openModal = null;
  }

  openFilter(): void {
    this.openModal = 'filter';
  }

  closeFilter(): void {
    if (this.openModal === 'filter') this.openModal = null;
  }

  toggleFilter(): void {
    if (this.openModal === 'filter') this.openModal = null;
    else this.openModal = 'filter';
  }

  closeModal(): void {
    this.openModal = null;
  }

  private computeAliveSpans(): Set<SpanId> {
    const out = new Set<SpanId>();
    if (this.hiddenEventTypes.size === 0) return out;
    const hidden = this.hiddenEventTypes;
    const folded = foldEvents(this.data.events);
    for (const fe of folded) {
      const id = foldedTypeId(fe);
      if (id === null) continue;
      if (hidden.has(id)) continue;
      const spanId = n(leadEvent(fe).span);
      for (const a of ancestors(this.data, spanId)) {
        out.add(n(a.id));
      }
    }
    return out;
  }

  private computeVisibleRows(): Row[] {
    if (this.hiddenEventTypes.size === 0) return this.rows;
    const hidden = this.hiddenEventTypes;
    const alive = this.aliveSpans;
    return this.rows.filter((r) => {
      if (r.kind === 'span-entry') return alive.has(n(r.span.id));
      if (r.kind === 'bif-row') return alive.has(n(r.span.id));
      if (r.kind === 'event') {
        const id = foldedTypeId(r.folded);
        return id !== null && !hidden.has(id);
      }
      if (r.kind === 'log-bar') return !hidden.has(r.level as EventTypeId);
      return false;
    });
  }

  private computeBufferRegions(): Map<string, BufferRegions> {
    const targetSeq = this.computeBufferTargetSeq();
    if (targetSeq === null) return new Map();

    const out = new Map<string, BufferRegions>();
    for (const marker of Object.keys(this.data.shells)) {
      out.set(marker, replayBufferRegionsAtMarker(this.data, targetSeq, marker));
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

  private computeBufferKey(): string | null {
    if (this.selected) return this.selected.shell_marker;
    if (this.selectedSpan) return spanBufferKey(this.data, this.selectedSpan);
    return null;
  }

  private computeVarsAt(): Map<string, string> | null {
    if (this.selected) return varsAtSeq(this.data, this.selected);
    if (this.selectedSpan) return varsAtSpan(this.data, this.selectedSpan);
    return null;
  }

  private computeCapturesAt(): Map<string, string> | null {
    if (this.selected) {
      return capturesAtSeq(this.data, this.selected, this.selected.shell);
    }
    if (this.selectedSpan) return capturesAtSpan(this.data, this.selectedSpan);
    return null;
  }
}
