<script lang="ts">
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    open: boolean;
    messageId: string;
    initialBody: string;
    onSubmit: (body: string) => void | Promise<void>;
    onClose: () => void;
  }

  const { open, messageId, initialBody, onSubmit, onClose }: Props = $props();

  let body = $state(initialBody);
  let submitting = $state(false);
  let textareaEl: HTMLTextAreaElement | null = $state(null);
  let lastMessageId = $state("");

  $effect(() => {
    if (messageId !== lastMessageId) {
      body = initialBody;
      lastMessageId = messageId;
    }
  });

  $effect(() => {
    if (open && textareaEl) {
      queueMicrotask(() => textareaEl?.focus());
    }
  });

  async function submit() {
    if (!body.trim() || submitting) return;
    submitting = true;
    try {
      await onSubmit(body.trim());
    } finally {
      submitting = false;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      submit();
    }
  }
</script>

<ModalShell
  {open}
  ariaLabel="Edit comment"
  onClose={onClose}
  onKeydown={handleKey}
  focusSelector="textarea"
  backdropClass="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
  panelClass="fixed left-1/2 top-1/2 z-[51] w-full max-w-xl -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-surface shadow-xl outline-none"
>
  <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
    <span class="text-sm font-medium text-fg-1">Edit comment</span>
    <button
      aria-label="Close"
      class="ml-auto text-muted hover:text-fg-2 px-2"
      onclick={onClose}
    >×</button>
  </div>

  <div class="px-4 py-3">
    <label class="block">
      <span class="block text-[11px] font-mono text-muted mb-1">Message</span>
      <textarea
        bind:this={textareaEl}
        bind:value={body}
        rows="6"
        class="w-full rounded-md border border-border bg-bg px-2.5 py-2 text-sm text-fg-2 font-mono outline-none focus:border-accent resize-y"
      ></textarea>
    </label>
    <div class="mt-2 text-[10px] text-muted font-mono">⌘+Enter to save · Esc to cancel</div>
  </div>

  <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
    <Button variant="primary" disabled={!body.trim() || submitting} onclick={submit}>
      {submitting ? "Saving…" : "Save"}
    </Button>
  </div>
</ModalShell>
