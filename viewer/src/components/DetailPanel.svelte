<script lang="ts">
  import type { ViewerState } from '../lib/state.svelte';
  import CallStack from './sections/CallStack.svelte';
  import SourcePeek from './sections/SourcePeek.svelte';
  import Variables from './sections/Variables.svelte';
  import Buffer from './sections/Buffer.svelte';
  import ShellContext from './sections/ShellContext.svelte';

  let { state }: { state: ViewerState } = $props();
</script>

<aside class="detail" aria-label="Details">
  <CallStack {state} />
  <SourcePeek {state} />
  <Variables {state} />
  <Buffer {state} />
  <ShellContext {state} />
</aside>

<style>
  .detail {
    padding: var(--gap-md) var(--gap-lg);
    overflow-y: auto;
    background: var(--sidebar);
    min-height: 50vh;
  }
</style>
