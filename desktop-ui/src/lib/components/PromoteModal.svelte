<script lang="ts">
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    open: boolean;
    kind: "question" | "finding";
    sourceId: string;
    initialBody: string;
    targetLineLabel: string;
    onSubmit: (body: string) => void | Promise<void>;
    onClose: () => void;
  }

  const {
    open,
    kind,
    sourceId,
    initialBody,
    targetLineLabel,
    onSubmit,
    onClose,
  }: Props = $props();

  let body = $state(initialBody);
  let submitting = $state(false);
  let textareaEl: HTMLTextAreaElement | null = $state(null);
  let lastSourceId = $state("");

  // Re-sync body when the modal is reopened with a new source.
  $effect(() => {
    if (sourceId !== lastSourceId) {
      body = initialBody;
      lastSourceId = sourceId;
    }
  });

  // Autofocus when opened.
  $effect(() => {
    if (open && textareaEl) {
      // Defer so the DOM is in place.
      queueMicrotask(() => textareaEl?.focus());
    }
  });

  const title = $derived(
    kind === "question" ? "Promote question to comment" : "Promote AI finding to comment",
  );

  async function submit() {
    if (!body.trim() || submitting) return;
    submitting = true;
    try {
      await onSubmit(body);
    } finally {
      submitting = false;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions a11y_interactive_supports_focus -->
  <div
    data-modal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
    onclick={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      class="w-full max-w-xl rounded-lg border border-border bg-surface shadow-xl"
      onkeydown={handleKey}
    >
      <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-comment"
          ><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
        <span class="text-sm font-medium text-fg-1">{title}</span>
        <button
          aria-label="Close"
          class="ml-auto text-muted hover:text-fg-2 px-2"
          onclick={onClose}
        >×</button>
      </div>

      <div class="px-4 py-3 space-y-3">
        <div class="text-[11px] font-mono text-muted">
          Target: <span class="text-fg-3">{targetLineLabel}</span>
        </div>

        <label class="block">
          <span class="block text-[11px] font-mono text-muted mb-1">Comment body</span>
          <textarea
            bind:this={textareaEl}
            bind:value={body}
            rows="8"
            class="w-full rounded-md border border-border bg-bg px-2.5 py-2 text-sm text-fg-2 font-mono outline-none focus:border-accent resize-y"
          ></textarea>
        </label>

        <div class="text-[10px] text-muted font-mono">
          ⌘+Enter to promote · Esc to cancel
        </div>
      </div>

      <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
        <Button variant="ghost" onclick={onClose}>Cancel</Button>
        <Button variant="primary" disabled={!body.trim() || submitting} onclick={submit}>
          {submitting ? "Promoting…" : "Promote"}
        </Button>
      </div>
    </div>
  </div>
{/if}
