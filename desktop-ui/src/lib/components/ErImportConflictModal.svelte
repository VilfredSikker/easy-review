<script lang="ts">
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import type { ErImportItemResult } from "$lib/types";

  interface Props {
    open: boolean;
    conflicts: ErImportItemResult[];
    onConfirm: () => void | Promise<void>;
    onClose: () => void;
  }

  const { open, conflicts, onConfirm, onClose }: Props = $props();

  let submitting = $state(false);

  function newerLabel(item: ErImportItemResult): string {
    return item.er_newer
      ? "your .er/ copy is newer"
      : "storage copy is newer (yours would overwrite it)";
  }

  async function confirm() {
    if (submitting) return;
    submitting = true;
    try {
      await onConfirm();
    } finally {
      submitting = false;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      confirm();
    }
  }
</script>

<ModalShell
  {open}
  ariaLabel="Overwrite review files"
  {onClose}
  onKeydown={handleKey}
  backdropClass="fixed inset-0 z-50 flex items-center justify-center bg-bg/60 p-6"
  panelClass="fixed left-1/2 top-1/2 z-[51] w-full max-w-xl -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-surface shadow-xl outline-none"
>
  <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-warning"
      ><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" /><line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" /></svg>
    <span class="text-sm font-medium text-fg-1">Overwrite existing review files?</span>
    <button
      aria-label="Close"
      class="ml-auto text-muted hover:text-fg-2 px-2"
      onclick={onClose}
    >×</button>
  </div>

  <div class="px-4 py-3 space-y-3">
    <p class="text-xs text-fg-3">
      These files already exist in shared storage with different content. Importing will
      overwrite the stored copies:
    </p>
    <ul class="space-y-1.5">
      {#each conflicts as item (item.name)}
        <li class="text-[12px] font-mono flex items-baseline gap-2">
          <span class="text-fg-2">{item.name}</span>
          <span class={item.er_newer ? "text-muted" : "text-warning"}>
            — {newerLabel(item)}
          </span>
        </li>
      {/each}
    </ul>
    <div class="text-[10px] text-muted font-mono">
      ⌘+Enter to overwrite · Esc to cancel
    </div>
  </div>

  <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
    <Button variant="primary" disabled={submitting} onclick={confirm}>
      {submitting ? "Overwriting…" : "Overwrite"}
    </Button>
  </div>
</ModalShell>
