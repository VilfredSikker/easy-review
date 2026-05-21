<script lang="ts">
  import { onMount } from "svelte";
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { closePrUrlModal, prUrlModal } from "$lib/stores/prUrlModal.svelte";

  let inputEl: HTMLInputElement | null = $state(null);

  const canSubmit = $derived(
    prUrlModal.url.trim().length > 0 && !prUrlModal.submitting,
  );

  $effect(() => {
    if (!prUrlModal.open) return;
    let cancelled = false;
    void tick().then(() => {
      if (!cancelled) inputEl?.focus({ preventScroll: true });
    });
    return () => {
      cancelled = true;
    };
  });

  function onUrlInput(e: Event) {
    prUrlModal.url = (e.currentTarget as HTMLInputElement).value;
  }

  async function submit() {
    if (inputEl) prUrlModal.url = inputEl.value;
    const url = prUrlModal.url.trim();
    if (!url || prUrlModal.submitting) return;
    prUrlModal.submitting = true;
    try {
      await app.cmd("open_pr_url", { url });
      closePrUrlModal();
      app.showEmptyState = false;
    } finally {
      prUrlModal.submitting = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!prUrlModal.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closePrUrlModal();
      return;
    }
    if (e.key === "Enter") {
      const target = e.target as HTMLElement | null;
      if (target?.tagName === "INPUT" || target?.tagName === "TEXTAREA") {
        e.preventDefault();
        void submit();
      }
    }
  }

  onMount(() => {
    window.addEventListener("keydown", handleKeydown, { capture: true });
    return () => window.removeEventListener("keydown", handleKeydown, { capture: true });
  });
</script>

{#if prUrlModal.open}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-modal="pr-url"
    class="fixed inset-0 z-[9998] bg-black/55 pointer-events-auto"
    style="backdrop-filter: blur(2px);"
    role="presentation"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) closePrUrlModal();
    }}
    onclick={(e) => {
      if (e.target === e.currentTarget) closePrUrlModal();
    }}
  ></div>

  <!-- Panel -->
  <div
    data-modal="pr-url"
    tabindex="-1"
    role="dialog"
    aria-modal="true"
    aria-label="Open PR by URL"
    class="fixed left-1/2 -translate-x-1/2 top-[16vh] z-[9999] w-[620px] max-w-[calc(100vw-2rem)] rounded-xl bg-card border border-border shadow-2xl overflow-hidden outline-none pointer-events-auto"
  >
    <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
      <span class="text-sm text-fg font-medium">Open PR by URL</span>
      <button
        type="button"
        onclick={() => closePrUrlModal()}
        aria-label="Close PR URL modal"
        class="ml-auto text-muted hover:text-fg-2 pointer-events-auto"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
      </button>
    </div>

    <div class="px-4 py-4 space-y-3">
      <p class="text-sm text-fg-3">Paste a GitHub PR URL and press Enter.</p>
      <div class="rounded-lg border border-border bg-surface px-3 py-2 flex items-center gap-2 min-w-0">
        <input
          bind:this={inputEl}
          value={prUrlModal.url}
          oninput={onUrlInput}
          onpaste={() => queueMicrotask(() => inputEl && (prUrlModal.url = inputEl.value))}
          class="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted mono pointer-events-auto"
          placeholder="https://github.com/owner/repo/pull/123"
        />
        <button
          type="button"
          onclick={() => void submit()}
          disabled={prUrlModal.submitting}
          class="shrink-0 px-3 py-1.5 rounded-md bg-accent hover:bg-accent/90 disabled:opacity-40 disabled:cursor-not-allowed text-black text-xs font-medium pointer-events-auto {canSubmit ? '' : 'opacity-60'}"
        >
          {prUrlModal.submitting ? "Opening…" : "Review"}
        </button>
      </div>
    </div>
  </div>
{/if}
