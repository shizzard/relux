<script lang="ts">
  import { onMount } from 'svelte';
  import { ViewerState } from './lib/state.svelte';
  import AppBar from './components/AppBar.svelte';
  import TimelineBar from './components/TimelineBar.svelte';
  import EventsList from './components/EventsList.svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import EnvModal from './components/EnvModal.svelte';
  import ShellsModal from './components/ShellsModal.svelte';
  import ArtifactsModal from './components/ArtifactsModal.svelte';
  import type { StructuredLog } from './types/StructuredLog';

  let { data }: { data: StructuredLog | null } = $props();
  const state = $derived(data ? new ViewerState(data) : null);

  function isTextInputTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
    return target.isContentEditable;
  }

  function cycleSearchInputs(): void {
    const scope: ParentNode =
      state && state.openModal !== null
        ? (document.querySelector('[role="dialog"]') ?? document)
        : document;
    const inputs = Array.from(
      scope.querySelectorAll<HTMLInputElement>('input[data-search-input]'),
    ).filter((el) => el.offsetParent !== null || el === document.activeElement);
    if (inputs.length === 0) return;
    const active = document.activeElement;
    const idx = active instanceof HTMLInputElement ? inputs.indexOf(active) : -1;
    const next = idx === -1 ? 0 : (idx + 1) % inputs.length;
    inputs[next]!.focus();
    inputs[next]!.select();
  }

  function handleKey(event: KeyboardEvent): void {
    if (!state) return;
    // Cmd/Ctrl+S: focus/cycle search inputs in current scope. Runs BEFORE the
    // text-input guard so it can advance from one focused input to the next.
    if (
      (event.metaKey || event.ctrlKey) &&
      !event.altKey &&
      !event.shiftKey &&
      event.key.toLowerCase() === 's'
    ) {
      event.preventDefault();
      cycleSearchInputs();
      return;
    }
    if (event.key === 'Escape' && state.openModal !== null) {
      event.preventDefault();
      state.closeModal();
      return;
    }
    if (isTextInputTarget(event.target)) return;
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === 'e') {
      event.preventDefault();
      if (state.openModal === 'env') state.closeEnv();
      else state.openEnv();
    } else if (key === 's') {
      event.preventDefault();
      if (state.openModal === 'shells') state.closeShells();
      else state.openShells();
    } else if (key === 'a') {
      event.preventDefault();
      if (state.data.artifacts.length === 0) return;
      if (state.openModal === 'artifacts') state.closeArtifacts();
      else state.openArtifacts();
    } else if (key === 't') {
      event.preventDefault();
      state.toggleErrorPath();
    } else if (key === 'm') {
      event.preventDefault();
      state.toggleSendMatch();
    } else if (key === 'f') {
      event.preventDefault();
      state.toggleFilter();
    } else if (key === 'c') {
      event.preventDefault();
      state.collapseAll();
    } else if (key === 'x') {
      event.preventDefault();
      state.expandAll();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  });
</script>

{#if state}
  <AppBar {state} />
  <TimelineBar {state} />
  <div class="layout">
    <EventsList {state} />
    <DetailPanel {state} />
  </div>
  <EnvModal {state} />
  <ShellsModal {state} />
  <ArtifactsModal {state} />
{:else}
  <p class="no-data">No data loaded. Set <code>window.RELUX_DATA</code> before this script runs.</p>
{/if}

<style>
  .layout {
    flex: 1 1 0;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 40fr) minmax(0, 60fr);
    gap: var(--gap-md);
    padding: var(--gap-md);
  }
  .no-data {
    margin: var(--gap-xl);
    color: var(--ink-dim);
  }
</style>
