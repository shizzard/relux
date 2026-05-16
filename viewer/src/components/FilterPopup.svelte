<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import { ALL_EVENT_TYPE_IDS, type EventTypeId } from '../lib/flatten';
  import { clickOutside } from '../lib/actions';
  import Chip from './Chip.svelte';

  let { state }: { state: ViewerState } = $props();

  const visibleCount = $derived(
    ALL_EVENT_TYPE_IDS.length - state.hiddenEventTypes.size,
  );

  function isChecked(id: EventTypeId): boolean {
    return !state.hiddenEventTypes.has(id);
  }
</script>

<div class="popup" use:clickOutside={() => state.closeFilter()}>
  <header class="head">
    <span class="title">filter events</span>
    <span class="count">{visibleCount} / {ALL_EVENT_TYPE_IDS.length} visible</span>
  </header>
  <ul class="list">
    {#each ALL_EVENT_TYPE_IDS as id (id)}
      <li>
        <label>
          <input
            type="checkbox"
            checked={isChecked(id)}
            onchange={() => state.toggleEventType(id)}
          />
          <span class="label">{id}</span>
        </label>
      </li>
    {/each}
  </ul>
  <footer class="actions">
    <Chip onclick={() => state.showAllEventTypes()}>show all</Chip>
    <Chip onclick={() => state.hideAllEventTypes()}>hide all</Chip>
  </footer>
</div>

<style>
  .popup {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 20;
    background: var(--paper);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    box-shadow: 4px 6px 0 rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    min-width: 220px;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--gap-sm);
    padding: 6px var(--gap-md);
    border-bottom: 1px dashed var(--border);
  }
  .title {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--ink);
  }
  .count {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--ink-faint);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 320px;
    overflow-y: auto;
  }
  .list li {
    margin: 0;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: 3px 8px;
    border-radius: 3px;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--ink);
  }
  label:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  input[type='checkbox'] {
    accent-color: var(--accent);
    cursor: pointer;
    margin: 0;
  }
  .label {
    user-select: none;
  }
  .actions {
    display: flex;
    gap: var(--gap-xs);
    padding: 6px var(--gap-sm);
    border-top: 1px dashed var(--border);
  }
</style>
