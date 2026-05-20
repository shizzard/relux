<script lang="ts">
  import type { ViewerState } from '../../lib/state.svelte';
  import { selectionSourceRange } from '../../lib/derive';
  import Panel from '../Panel.svelte';
  import SourceView from './SourceView.svelte';

  let { state }: { state: ViewerState } = $props();

  const range = $derived(
    selectionSourceRange(state.data, state.selectedSpanId, state.selectedEventSeq),
  );
  const hint = $derived(range ? `${range.file}:${range.line}` : 'no location');
</script>

<Panel title="source" {hint}>
  <SourceView {state} />
</Panel>
