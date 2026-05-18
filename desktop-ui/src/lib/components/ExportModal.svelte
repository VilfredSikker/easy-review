<script lang="ts" module>
  /** Global flag toggled from CommandPalette + keyboard. Imported by App.svelte
   * so the modal is mounted once at the root and any caller can flip it. */
  let openState = $state(false);
  export function openExportModal() {
    openState = true;
  }
  export function closeExportModal() {
    openState = false;
  }
  export function exportModalOpen(): boolean {
    return openState;
  }
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { app } from "$lib/stores/app.svelte";

  /** Optional preview override — used by Storybook to render without a Tauri host. */
  interface Props {
    /** When provided, replaces the Tauri-backed preview render. */
    previewOverride?: string | null;
  }
  const { previewOverride = null }: Props = $props();

  let includeComments = $state(true);
  let includeQuestions = $state(true);
  let includeFindings = $state(true);
  let includeAnnotations = $state(true);
  let onlyUnresolved = $state(false);
  let revisions = $state<{ revision_id: string; active: boolean }[]>([]);
  let selectedRevisionId = $state<string>("");

  let preview = $state<string>("");
  let savedPath = $state<string | null>(null);
  let savedAt = $state<number>(0);
  let error = $state<string | null>(null);

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const opts = $derived({
    includeComments,
    includeQuestions,
    includeFindings,
    includeAnnotations,
    onlyUnresolved,
  });

  async function refreshPreview() {
    if (previewOverride !== null) {
      preview = previewOverride;
      return;
    }
    try {
      preview = await invoke<string>("export_review", { opts });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  // Refresh whenever the modal is opened or opts change.
  $effect(() => {
    void opts;
    void openState;
    if (!openState) return;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(refreshPreview, 200);
  });

  onMount(() => {
    refreshPreview();
  });

  $effect(() => {
    if (!openState) return;
    invoke<{ revision_id: string; active: boolean }[]>("list_review_revisions")
      .then((r) => {
        revisions = r;
        selectedRevisionId = (r.find((x) => x.active) ?? r[0])?.revision_id ?? "";
      })
      .catch(() => {});
  });

  async function handleCopyToClipboard() {
    try {
      const body = previewOverride !== null
        ? previewOverride
        : await invoke<string>("export_review", { opts });
      await copyToClipboard(body);
      app.pushLog("info", "clipboard", `Copied ${body.length} chars`);
      savedPath = "Copied to clipboard";
      savedAt = Date.now();
      setTimeout(() => {
        if (Date.now() - savedAt >= 1900) savedPath = null;
      }, 2000);
    } catch (e) {
      error = String(e);
    }
  }

  async function saveToFile() {
    try {
      const target = await invoke<string>("export_review_to_file", { opts, path: "" });
      savedPath = `Saved to ${target}`;
      savedAt = Date.now();
      setTimeout(() => {
        if (Date.now() - savedAt >= 2900) savedPath = null;
      }, 3000);
    } catch (e) {
      error = String(e);
    }
  }

  async function copyReviewJson() {
    try {
      const body = await invoke<string>("read_review_json", { revisionId: selectedRevisionId || null });
      await copyToClipboard(body);
      app.showToast("success", `Copied review.json (${body.length} bytes)`);
    } catch {
      app.showToast("error", "No review.json found for selected revision");
    }
  }

  function onBackdropKey(e: KeyboardEvent) {
    if (e.key === "Escape") closeExportModal();
  }
</script>

{#if openState}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-modal
    class="fixed inset-0 z-[200] bg-black/55 flex items-start justify-center pt-[10vh]"
    style="backdrop-filter: blur(2px);"
    onclick={closeExportModal}
    onkeydown={onBackdropKey}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[640px] max-h-[80vh] rounded-xl bg-card border border-border shadow-2xl overflow-hidden flex flex-col"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
        <span class="text-sm text-fg font-medium">Export review to coding agent</span>
        <button
          onclick={closeExportModal}
          aria-label="Close export modal"
          class="ml-auto text-muted hover:text-fg-2"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
        </button>
      </div>

      <div class="px-4 py-3 border-b border-hairline space-y-2">
        {#if revisions.length > 1}
          <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
            <span>Revision</span>
            <select bind:value={selectedRevisionId} class="bg-bg border border-border rounded px-2 py-1 text-xs">
              {#each revisions as rev}
                <option value={rev.revision_id}>{rev.active ? "active · " : ""}{rev.revision_id}</option>
              {/each}
            </select>
          </label>
        {/if}
        <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input type="checkbox" bind:checked={includeComments} />
          <span>Comments</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input type="checkbox" bind:checked={includeQuestions} />
          <span>Questions</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input type="checkbox" bind:checked={includeFindings} />
          <span>AI findings</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input type="checkbox" bind:checked={includeAnnotations} />
          <span>UI annotations</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer pt-1 border-t border-hairline mt-2">
          <input type="checkbox" bind:checked={onlyUnresolved} />
          <span>Only unresolved</span>
        </label>
      </div>

      <div class="flex items-center gap-2 px-4 py-3 border-b border-hairline">
        <button
          onclick={handleCopyToClipboard}
          class="px-3 py-1.5 rounded-md bg-accent text-black text-xs font-medium hover:opacity-90"
        >
          Copy to clipboard
        </button>
        <button
          onclick={saveToFile}
          class="px-3 py-1.5 rounded-md border border-border text-xs text-fg-2 hover:bg-hover"
        >
          Save to file
        </button>
        <button
          onclick={copyReviewJson}
          class="px-3 py-1.5 rounded-md border border-border text-xs text-fg-2 hover:bg-hover"
        >
          Copy review.json
        </button>
        {#if savedPath}
          <span class="text-[11px] text-add-fg mono ml-1">{savedPath}</span>
        {/if}
        {#if error}
          <span class="text-[11px] text-del-fg mono ml-1">{error}</span>
        {/if}
      </div>

      <div class="flex-1 overflow-y-auto px-4 py-3 min-h-0">
        <div class="text-[10px] uppercase tracking-wider text-muted mb-1">Preview</div>
        <pre
          class="text-[12px] mono text-fg-2 bg-bg border border-hairline rounded p-3 whitespace-pre-wrap break-words"
        >{preview || "(loading…)"}</pre>
      </div>
    </div>
  </div>
{/if}
