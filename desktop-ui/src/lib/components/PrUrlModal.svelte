<script lang="ts" module>
  let openState = $state(false);

  export function openPrUrlModal() {
    openState = true;
  }

  export function closePrUrlModal() {
    openState = false;
  }
</script>

<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  let prUrl = $state("");
  let submitting = $state(false);
  let inputEl: HTMLInputElement | null = $state(null);

  $effect(() => {
    if (!openState) return;
    queueMicrotask(() => inputEl?.focus());
  });

  async function submit() {
    const url = prUrl.trim();
    if (!url || submitting) return;
    submitting = true;
    try {
      await app.cmd("open_pr_url", { url });
      prUrl = "";
      closePrUrlModal();
      app.showEmptyState = false;
    } finally {
      submitting = false;
    }
  }

  function onBackdropKey(e: KeyboardEvent) {
    if (e.key === "Escape") closePrUrlModal();
    if (e.key === "Enter") void submit();
  }
</script>

{#if openState}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-modal
    class="fixed inset-0 z-[220] bg-black/55 flex items-start justify-center pt-[16vh]"
    style="backdrop-filter: blur(2px);"
    onclick={closePrUrlModal}
    onkeydown={onBackdropKey}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[620px] rounded-xl bg-card border border-border shadow-2xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
        <span class="text-sm text-fg font-medium">Open PR by URL</span>
        <button
          onclick={closePrUrlModal}
          aria-label="Close PR URL modal"
          class="ml-auto text-muted hover:text-fg-2"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
        </button>
      </div>

      <div class="px-4 py-4 space-y-3">
        <p class="text-sm text-fg-3">Paste a GitHub PR URL and press Enter.</p>
        <div class="rounded-lg border border-border bg-surface px-3 py-2 flex items-center gap-2">
          <input
            bind:this={inputEl}
            bind:value={prUrl}
            onkeydown={(e) => e.key === "Enter" && submit()}
            class="w-full bg-transparent text-sm outline-none placeholder:text-muted mono"
            placeholder="https://github.com/owner/repo/pull/123"
          />
          <button
            onclick={submit}
            disabled={!prUrl.trim() || submitting}
            class="px-3 py-1.5 rounded-md bg-accent hover:bg-accent/90 disabled:opacity-40 disabled:cursor-not-allowed text-black text-xs font-medium"
          >
            {submitting ? "Opening…" : "Review"}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
