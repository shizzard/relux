<script lang="ts">
  import { ViewerState } from './lib/state.svelte';
  import Header from './components/Header.svelte';
  import FailureBanner from './components/FailureBanner.svelte';
  import EventsList from './components/EventsList.svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import JumpToFailure from './components/JumpToFailure.svelte';
  import EnvModal from './components/EnvModal.svelte';
  import type { StructuredLog } from './types/StructuredLog';

  let { data }: { data: StructuredLog | null } = $props();
  const state = $derived(data ? new ViewerState(data) : null);
</script>

{#if state}
  <Header {state} />
  <FailureBanner {state} />
  <div class="layout">
    <EventsList {state} />
    <DetailPanel {state} />
  </div>
  <JumpToFailure {state} />
  <EnvModal {state} />
{:else}
  <p class="no-data">No data loaded. Set <code>window.RELUX_DATA</code> before this script runs.</p>
{/if}

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 62%) minmax(0, 38%);
  }
  .no-data {
    margin: var(--gap-xl);
    color: var(--muted);
  }
</style>
