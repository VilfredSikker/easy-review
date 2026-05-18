<script lang="ts">
  import type { ToastMessage } from "$lib/stores/app.svelte";

  interface Props { toasts: ToastMessage[]; }
  const { toasts }: Props = $props();

  const borderColor: Record<ToastMessage["kind"], string> = {
    success: "border-l-4 border-l-add-fg",
    error: "border-l-4 border-l-del-fg",
    info: "border-l-4 border-l-accent",
  };
</script>

<div class="fixed right-6 bottom-6 flex flex-col gap-2 z-50 pointer-events-none">
  {#each toasts as toast (toast.id)}
    <div
      class="bg-ink-700 text-ink-100 text-xs font-mono w-80 px-4 py-3 rounded-none shadow-lg border border-ink-500/40 whitespace-pre-wrap break-words leading-relaxed {borderColor[toast.kind]}"
    >
      {toast.message}
    </div>
  {/each}
</div>
