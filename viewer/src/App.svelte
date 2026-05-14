<script lang="ts">
  import { onMount } from 'svelte';
  import { ViewerState } from './lib/state.svelte';
  import AppBar from './components/AppBar.svelte';
  import EventsList from './components/EventsList.svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import EnvModal from './components/EnvModal.svelte';
  import ShellsModal from './components/ShellsModal.svelte';
  import type { StructuredLog } from './types/StructuredLog';

  let { data }: { data: StructuredLog | null } = $props();
  const state = $derived(data ? new ViewerState(data) : null);

  function handleKey(event: KeyboardEvent): void {
    if (!state) return;
    const mod = event.metaKey || event.ctrlKey;
    if (event.key === 'Escape' && state.openModal !== null) {
      event.preventDefault();
      state.closeModal();
      return;
    }
    if (!mod) return;
    const key = event.key.toLowerCase();
    if (key === 'e') {
      event.preventDefault();
      if (state.openModal === 'env') state.closeEnv();
      else state.openEnv();
    } else if (key === '\\') {
      event.preventDefault();
      if (state.openModal === 'shells') state.closeShells();
      else state.openShells();
    } else if (key === 'k') {
      event.preventDefault();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  });
</script>

{#if state}
  <AppBar {state} />
  <div class="layout">
    <EventsList {state} />
    <DetailPanel {state} />
  </div>
  <EnvModal {state} />
  <ShellsModal {state} />
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
