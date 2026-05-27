<script lang="ts">
  import { diffSel } from "$lib/stores/diffSelection.svelte";

  interface Props {
    onGoBack: () => void;
  }
  const { onGoBack }: Props = $props();
</script>

<button
  type="button"
  class="composer-scroll-back px-3 py-1.5 rounded-full
         bg-card border border-hairline shadow-lg text-sm text-fg-2 hover:text-fg-1 hover:bg-hover
         flex items-center gap-2 pointer-events-auto"
  onclick={onGoBack}
>
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <path d="M12 19V5M5 12l7-7 7 7"/>
  </svg>
  <span>Go back to comment</span>
  <span class="text-fg-3 text-xs mono">{diffSel.rangeLabel()}</span>
</button>
