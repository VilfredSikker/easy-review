<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { copyToClipboard } from "$lib/clipboard";
  import { app } from "$lib/stores/app.svelte";

  type ExportOpts = {
    includeComments: boolean;
    includeQuestions: boolean;
    includeFindings: boolean;
    includeAnnotations: boolean;
    onlyUnresolved: boolean;
  };
  type ExportOptionKey = keyof ExportOpts;

  let includeComments = $state(true);
  let includeQuestions = $state(true);
  let includeFindings = $state(true);
  let includeAnnotations = $state(true);
  let onlyUnresolved = $state(false);

  let preview = $state("");
  let loadingPreview = $state(false);
  let error = $state<string | null>(null);
  let savedPath = $state<string | null>(null);
  let savedAt = $state(0);
  let previewRequestId = 0;

  function currentExportOpts(): ExportOpts {
    return {
      includeComments,
      includeQuestions,
      includeFindings,
      includeAnnotations,
      onlyUnresolved,
    };
  }

  async function refreshPreview(optsSnapshot = currentExportOpts()) {
    const requestId = ++previewRequestId;
    loadingPreview = true;
    try {
      const nextPreview = await invoke<string>("export_review", { opts: optsSnapshot });
      if (requestId !== previewRequestId) return;
      preview = nextPreview;
      error = null;
    } catch (e) {
      if (requestId !== previewRequestId) return;
      error = String(e);
    } finally {
      if (requestId === previewRequestId) loadingPreview = false;
    }
  }

  function setExportOption(key: ExportOptionKey, checked: boolean) {
    const nextOpts = {
      ...currentExportOpts(),
      [key]: checked,
    };

    includeComments = nextOpts.includeComments;
    includeQuestions = nextOpts.includeQuestions;
    includeFindings = nextOpts.includeFindings;
    includeAnnotations = nextOpts.includeAnnotations;
    onlyUnresolved = nextOpts.onlyUnresolved;

    void refreshPreview(nextOpts);
  }

  function onExportOptionChange(key: ExportOptionKey, e: Event) {
    setExportOption(key, (e.currentTarget as HTMLInputElement).checked);
  }

  async function handleCopyToClipboard() {
    try {
      const body = await invoke<string>("export_review", { opts: currentExportOpts() });
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
      const target = await invoke<string>("export_review_to_file", { opts: currentExportOpts(), path: "" });
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
      const body = await invoke<string>("read_review_json", { revisionId: null });
      await copyToClipboard(body);
      app.showToast("success", `Copied review.json (${body.length} bytes)`);
    } catch {
      app.showToast("error", "No review.json found for this review tab");
    }
  }

  $effect(() => {
    if (app.mainView !== "export-review") return;
    app.snapshot?.active_tab;
    void refreshPreview();
  });
</script>

<div class="flex-1 min-w-0 min-h-0 overflow-hidden flex flex-col bg-ink-900">
  <div class="h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-2 text-sm">
    <span class="text-fg-2">Export Review</span>
    {#if loadingPreview}
      <span class="text-[11px] text-muted mono">Refreshing…</span>
    {/if}
    <button
      class="ml-auto px-2 py-1 text-xs border border-border rounded hover:bg-hover"
      onclick={handleCopyToClipboard}
      disabled={loadingPreview && !preview}
    >
      Copy to clipboard
    </button>
    <button
      class="px-2 py-1 text-xs border border-border rounded hover:bg-hover"
      onclick={saveToFile}
      disabled={loadingPreview && !preview}
    >
      Save to file
    </button>
    <button class="px-2 py-1 text-xs border border-border rounded hover:bg-hover" onclick={copyReviewJson}>
      Copy review.json
    </button>
    <button class="px-2 py-1 text-xs border border-border rounded hover:bg-hover" onclick={() => app.setMainView("diff")}>
      Back to diff
    </button>
  </div>

  <div class="px-4 py-3 border-b border-hairline bg-card flex flex-wrap items-center gap-x-5 gap-y-2">
    <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
      <input
        type="checkbox"
        checked={includeComments}
        onchange={(e) => onExportOptionChange("includeComments", e)}
      />
      <span>Comments</span>
    </label>
    <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
      <input
        type="checkbox"
        checked={includeQuestions}
        onchange={(e) => onExportOptionChange("includeQuestions", e)}
      />
      <span>Questions</span>
    </label>
    <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
      <input
        type="checkbox"
        checked={includeFindings}
        onchange={(e) => onExportOptionChange("includeFindings", e)}
      />
      <span>AI findings</span>
    </label>
    <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
      <input
        type="checkbox"
        checked={includeAnnotations}
        onchange={(e) => onExportOptionChange("includeAnnotations", e)}
      />
      <span>UI annotations</span>
    </label>
    <label class="flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
      <input
        type="checkbox"
        checked={onlyUnresolved}
        onchange={(e) => onExportOptionChange("onlyUnresolved", e)}
      />
      <span>Only unresolved</span>
    </label>
    {#if savedPath}
      <span class="text-[11px] text-add-fg mono">{savedPath}</span>
    {/if}
    {#if error}
      <span class="text-[11px] text-del-fg mono">{error}</span>
    {/if}
  </div>

  <div class="flex-1 min-h-0 overflow-y-auto p-4">
    <div class="text-[10px] uppercase tracking-wider text-muted mb-1">Preview</div>
    <pre
      class="min-h-full text-[12px] mono text-fg-2 bg-bg border border-hairline rounded p-3 whitespace-pre-wrap break-words"
    >{preview || (loadingPreview ? "(loading…)" : error ? `(error) ${error}` : "No preview.")}</pre>
  </div>
</div>
