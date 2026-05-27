<script lang="ts">
  import type { ToastMessage } from "$lib/stores/app.svelte";
  import ToastCard from "$lib/components/ToastCard.svelte";

  interface Props {
    toasts: ToastMessage[];
  }
  const { toasts }: Props = $props();
</script>

<!-- Stacked bottom-right; newest on top (flex-col-reverse + DOM order = oldest first). -->
<div class="fixed right-4 bottom-4 flex flex-col-reverse gap-2 z-50 pointer-events-none items-end">
  {#each toasts as toast (toast.id)}
    <ToastCard {toast} />
  {/each}
</div>
