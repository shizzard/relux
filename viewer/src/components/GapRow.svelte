<script lang="ts">
  import { formatDuration } from '../lib/format';

  let { ms }: { ms: number } = $props();
</script>

<li class="gap" aria-hidden="true">
  <span class="dots"></span>
  <span class="elapsed">{formatDuration(ms)} elapsed</span>
  <span class="dots"></span>
</li>

<style>
  .gap {
    list-style: none;
    margin: 0;
    padding: var(--gap-xs) var(--gap-sm);
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
  }
  .dots {
    flex: 1 1 auto;
    border-top: 1px dashed var(--border);
    height: 0;
  }
  .elapsed {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--muted);
    flex: 0 0 auto;
  }
</style>
