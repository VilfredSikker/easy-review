<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { copyToClipboard } from "$lib/clipboard";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { app } from "$lib/stores/app.svelte";

  type ExportFormat = "markdown" | "html";
  type PreviewView = "rendered" | "source";
  type ExportOpts = {
    includeComments: boolean;
    includeQuestions: boolean;
    includeFindings: boolean;
    includeAnnotations: boolean;
    onlyUnresolved: boolean;
    format: ExportFormat;
  };
  type ExportOptionKey = keyof Omit<ExportOpts, "format">;

  let includeComments = $state(true);
  let includeQuestions = $state(true);
  let includeFindings = $state(true);
  let includeAnnotations = $state(true);
  let onlyUnresolved = $state(false);
  let format = $state<ExportFormat>("markdown");
  let previewView = $state<PreviewView>("rendered");

  let preview = $state("");
  let loadingPreview = $state(false);
  let error = $state<string | null>(null);
  let savedPath = $state<string | null>(null);
  let savedAt = $state(0);
  let previewRequestId = 0;
  let lastPreviewTab = $state(-1);

  function currentExportOpts(): ExportOpts {
    return {
      includeComments,
      includeQuestions,
      includeFindings,
      includeAnnotations,
      onlyUnresolved,
      format,
    };
  }

  function setFormat(next: ExportFormat) {
    if (next === format) return;
    format = next;
    void refreshPreview();
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

  function applyExportOpts(nextOpts: ExportOpts) {
    includeComments = nextOpts.includeComments;
    includeQuestions = nextOpts.includeQuestions;
    includeFindings = nextOpts.includeFindings;
    includeAnnotations = nextOpts.includeAnnotations;
    onlyUnresolved = nextOpts.onlyUnresolved;
    format = nextOpts.format;
    void refreshPreview(nextOpts);
  }

  function setExportOption(key: ExportOptionKey, checked: boolean) {
    applyExportOpts({ ...currentExportOpts(), [key]: checked });
  }

  function includeAllOptions() {
    applyExportOpts({
      ...currentExportOpts(),
      includeComments: true,
      includeQuestions: true,
      includeFindings: true,
      includeAnnotations: true,
      onlyUnresolved: false,
    });
  }

  function excludeAllOptions() {
    applyExportOpts({
      ...currentExportOpts(),
      includeComments: false,
      includeQuestions: false,
      includeFindings: false,
      includeAnnotations: false,
      onlyUnresolved: false,
    });
  }

  function onExportOptionChange(key: ExportOptionKey, e: Event) {
    setExportOption(key, (e.currentTarget as HTMLInputElement).checked);
  }

  async function handleCopyToClipboard() {
    try {
      const body = await invoke<string>("export_review", { opts: currentExportOpts() });
      await copyToClipboard(body);
      app.pushLog("info", "clipboard", `Copied ${body.length} chars (${format})`);
      savedPath = `Copied ${format === "html" ? "HTML" : "markdown"} to clipboard`;
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
    if (app.mainView !== "export-review") {
      lastPreviewTab = -1;
      return;
    }
    const tab = app.snapshot?.active_tab ?? -1;
    if (tab === lastPreviewTab) return;
    lastPreviewTab = tab;
    void refreshPreview();
  });
</script>

<div class="flex-1 min-w-0 min-h-0 overflow-hidden flex flex-col bg-ink-900">
  <div class="h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-2 text-sm">
    <span class="text-fg-2">Export Review</span>
    {#if loadingPreview}
      <span class="text-[11px] text-muted mono">Refreshing…</span>
    {/if}
    <div
      class="ml-auto flex items-center rounded border border-border overflow-hidden"
      role="group"
      aria-label="Export format"
    >
      <button
        class={`px-2 py-1 text-xs ${format === "markdown" ? "bg-hover text-fg" : "text-muted hover:text-fg-2"}`}
        aria-pressed={format === "markdown"}
        onclick={() => setFormat("markdown")}
      >
        Markdown
      </button>
      <span class="w-px self-stretch bg-border" aria-hidden="true"></span>
      <button
        class={`px-2 py-1 text-xs ${format === "html" ? "bg-hover text-fg" : "text-muted hover:text-fg-2"}`}
        aria-pressed={format === "html"}
        onclick={() => setFormat("html")}
      >
        HTML
      </button>
    </div>
    <button
      class="px-2 py-1 text-xs border border-border rounded hover:bg-hover"
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
    <span class="w-px h-4 bg-hairline" aria-hidden="true"></span>
    <button
      type="button"
      class="px-2 py-0.5 text-xs border border-border rounded hover:bg-hover text-fg-2"
      onclick={includeAllOptions}
    >
      Include all
    </button>
    <button
      type="button"
      class="px-2 py-0.5 text-xs border border-border rounded hover:bg-hover text-fg-2"
      onclick={excludeAllOptions}
    >
      Exclude all
    </button>
    {#if savedPath}
      <span class="text-[11px] text-add-fg mono">{savedPath}</span>
    {/if}
    {#if error}
      <span class="text-[11px] text-del-fg mono">{error}</span>
    {/if}
  </div>

  <div class="flex-1 min-h-0 overflow-hidden p-4 flex flex-col">
    <div class="flex items-center gap-2 mb-1">
      <span class="text-[10px] uppercase tracking-wider text-muted">
        Preview — {format === "html" ? "export.html" : "export.md"}
      </span>
      <div class="ml-auto flex items-center gap-2 text-[10px] uppercase tracking-wider">
        <button
          class={previewView === "rendered" ? "text-fg-2" : "text-muted hover:text-fg-3"}
          aria-pressed={previewView === "rendered"}
          onclick={() => (previewView = "rendered")}
        >
          Rendered
        </button>
        <button
          class={previewView === "source" ? "text-fg-2" : "text-muted hover:text-fg-3"}
          aria-pressed={previewView === "source"}
          onclick={() => (previewView = "source")}
        >
          Source
        </button>
      </div>
    </div>
    {#if preview && previewView === "rendered" && format === "html"}
      <!-- WYSIWYG preview of the standalone HTML document. sandbox="" blocks
           scripts/navigation; the document itself escapes annotation HTML. -->
      <iframe
        class="flex-1 min-h-0 w-full bg-bg border border-hairline rounded"
        title="HTML export preview"
        sandbox=""
        srcdoc={preview}
      ></iframe>
    {:else}
      <div
        class="flex-1 min-h-0 overflow-y-auto text-[12px] text-fg-2 bg-bg border border-hairline rounded p-3 break-words export-preview"
      >
        {#if preview}
          {#if previewView === "source"}
            <pre class="mono text-[11px] leading-relaxed text-fg-3 whitespace-pre-wrap break-words">{preview}</pre>
          {:else}
            <MarkdownText text={preview} className="export-preview-markdown" />
          {/if}
        {:else if loadingPreview}
          <p class="text-muted mono">(loading…)</p>
        {:else if error}
          <p class="text-del-fg mono">(error) {error}</p>
        {:else}
          <p class="text-muted">No preview.</p>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .export-preview :global(.export-preview-markdown h1) {
    font-size: 1.15rem;
    color: var(--color-fg);
  }
  .export-preview :global(.export-preview-markdown h2) {
    font-size: 0.95rem;
    color: var(--color-fg-2);
    font-family: "JetBrains Mono", monospace;
  }
  .export-preview :global(.export-preview-markdown h3) {
    font-size: 0.85rem;
    color: var(--color-fg-2);
  }
  .export-preview :global(.export-preview-markdown blockquote) {
    color: var(--color-fg-3);
  }
  .export-preview :global(.export-preview-markdown code) {
    color: var(--color-add-fg);
    background: rgba(34, 197, 94, 0.08);
    padding: 0.05rem 0.25rem;
    border-radius: 3px;
  }
  .export-preview :global(.export-preview-markdown pre) {
    background: var(--color-ink-870);
    font-size: 0.9em;
  }
</style>
