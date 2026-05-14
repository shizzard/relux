<script lang="ts">
  import type { Event } from '../types/Event';
  import type { LogLevel } from '../lib/flatten';
  import { formatTimestamp } from '../lib/format';

  let {
    level,
    event,
    depth,
  }: { level: LogLevel; event: Event; depth: number } = $props();

  const message = $derived(messageOf(event));
  const ts = $derived(formatTimestamp(event.ts));
  const rails = $derived(Array.from({ length: depth }, (_, i) => i));

  // log/warning/error are the only kinds routed through this component; the
  // type system narrows but a defensive fall-through keeps stray kinds
  // visible rather than silently empty.
  function messageOf(ev: Event): string {
    if (ev.kind === 'log' || ev.kind === 'warning' || ev.kind === 'error') {
      return ev.message;
    }
    return '';
  }

  const PICTOGRAMS: Record<LogLevel, string> = {
    log: '\u{25CB}',
    warning: '\u{25B2}',
    error: '\u{25CF}',
  };
</script>

<li class="log-bar {level}">
  {#each rails as i (i)}<span class="rail" aria-hidden="true"></span>{/each}
  <span class="pictogram" aria-hidden="true">{PICTOGRAMS[level]}</span>
  <span class="message">{message}</span>
  <span class="ts">{ts}</span>
</li>

<style>
  .log-bar {
    list-style: none;
    display: flex;
    align-items: stretch;
    margin: 0;
    padding: 0;
    min-height: 24px;
    border-left: 2px solid var(--ink-faint);
    background: color-mix(in srgb, var(--ink-faint) 4%, transparent);
  }
  .log-bar.warning {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .log-bar.error {
    border-left-color: var(--danger);
    background: color-mix(in srgb, var(--danger) 6%, transparent);
  }
  .rail {
    width: 24px;
    flex: 0 0 auto;
    border-right: 1px solid var(--border);
  }
  .pictogram {
    width: 20px;
    text-align: center;
    flex: 0 0 auto;
    align-self: center;
    color: var(--ink-faint);
    font-family: var(--font-mono);
  }
  .log-bar.warning .pictogram {
    color: var(--accent);
  }
  .log-bar.error .pictogram {
    color: var(--danger);
  }
  .message {
    font-family: var(--font-mono);
    font-size: 0.82rem;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    align-self: center;
    color: var(--ink);
    padding: 0 var(--gap-sm);
  }
  .ts {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--ink-faint);
    padding: 0 var(--gap-sm);
    flex: 0 0 auto;
    align-self: center;
  }
</style>
