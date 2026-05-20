<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import CallStack from './sections/CallStack.svelte';
  import SourcePeek from './sections/SourcePeek.svelte';
  import Variables from './sections/Variables.svelte';
  import ShellBuffer from './sections/ShellBuffer.svelte';

  let { state }: { state: ViewerState } = $props();
</script>

<aside class="detail" aria-label="Details">
  <SourcePeek {state} />
  <ShellBuffer {state} />
  <Variables {state} />
  <CallStack {state} />
</aside>

<style>
  .detail {
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--gap-md);
    overflow: hidden;
  }
</style>
