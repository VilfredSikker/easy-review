<script lang="ts">
  import { tick } from "svelte";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import { documentPreviewKind } from "$lib/documentPreview";
  import type { DocumentViewMode } from "$lib/documentPreviewCache";
  import { langForPath } from "$lib/extToLang";
  import { renameHeader } from "$lib/renameDiff";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
    previewPaths?: ReadonlySet<string>;
    sidePaths?: ReadonlySet<string>;
    onpreviewchange?: (path: string, mode: DocumentViewMode) => void;
  }
  const { row, previewPaths = new Set<string>(), sidePaths = new Set<string>(), onpreviewchange }: Props = $props();

  const documentKind = $derived(documentPreviewKind(row.filePath));
  const viewMode = $derived.by((): DocumentViewMode => {
    if (previewPaths.has(row.filePath)) return "preview";
    return sidePaths.has(row.filePath) ? "side" : "raw";
  });
  // Side by side only helps where rendering changes the text: markdown.
  const viewModes = $derived<[DocumentViewMode, string][]>(documentKind === "markdown"
    ? [["raw", "Raw"], ["preview", "Preview"], ["side", "Side by side"]]
    : [["raw", "Raw"], ["preview", "Preview"]]);

  // Read reviewed live from the snapshot, not from the baked-in row: the diff
  // render model intentionally ignores `reviewed` so toggling it is a cache hit.
  const reviewed = $derived(
    app.snapshot?.files.find((f) => f.path === row.filePath)?.reviewed ?? false,
  );

  const collapsed = $derived.by(() => {
    diffFileCollapse.revision;
    return diffFileCollapse.collapsed.has(row.filePath);
  });

  async function afterCollapse(collapsedPath: string) {
    await tick();
    await diffNav.scrollAfterCollapse(collapsedPath);
  }

  function toggleCollapse(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    // Snapshot the path: in the sticky overlay `row` is a reactive derived that
    // re-points to another file once collapsing shifts the layout (read below).
    const path = row.filePath;
    const wasCollapsed = diffFileCollapse.isCollapsed(path);
    diffFileCollapse.toggle(path);
    if (!wasCollapsed) void afterCollapse(path);
  }

  async function toggleReviewed(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    // Snapshot the clicked file once. In the sticky overlay, `row` is the reactive
    // `visibleFileHeaderRow` derived; collapsing the file shifts the layout, which
    // re-points it to a different file — so re-reading `row.filePath` after the
    // collapse would mark/scroll the wrong file.
    const path = row.filePath;
    const isReviewed = app.snapshot?.files.find((f) => f.path === path)?.reviewed ?? false;
    try {
      if (isReviewed) {
        await app.cmd("unmark_reviewed", { path });
      } else {
        diffFileCollapse.collapse(path);
        await app.cmd("mark_reviewed", { path });
        await afterCollapse(path);
      }
    } catch (err) {
      app.showToast("error", String(err));
    }
  }

  // Split filePath into directory and filename for breadcrumb emphasis
  const pathParts = $derived.by(() => {
    const i = row.filePath.lastIndexOf("/");
    if (i === -1) return { dir: "", name: row.filePath };
    return { dir: row.filePath.slice(0, i + 1), name: row.filePath.slice(i + 1) };
  });

  const lang = $derived(langForPath(row.filePath));
  const rename = $derived(row.oldPath ? renameHeader(row.oldPath, row.filePath) : null);
  const title = $derived(row.oldPath ? `${row.oldPath} → ${row.filePath}` : row.filePath);

  async function openSource(e: MouseEvent) {
    e.stopPropagation();
    try {
      const res = await invoke<{ kind: string; target: string }>("open_source");
      if (res.kind === "needs_checkout") {
        app.showToast("info", "Create editable worktree to open locally");
      }
    } catch (err) {
      app.showToast("error", `VS Code: ${err}`);
    }
  }
</script>

<!-- Collapse chevron (primary action, leftmost) -->
<button
  type="button"
  class="shrink-0 w-5 h-5 text-fg-2 hover:bg-hover hover:text-fg rounded flex items-center justify-center transition"
  title={collapsed ? "Expand file" : "Collapse file"}
  aria-label={collapsed ? "Expand file diff" : "Collapse file diff"}
  aria-expanded={!collapsed}
  onclick={toggleCollapse}
>
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2.5"
    class="transition-transform {collapsed ? '' : 'rotate-90'}"
  >
    <polyline points="9 18 15 12 9 6" />
  </svg>
</button>

<!-- Language chip, then filename first: the name is what a reviewer scans for,
     the directory trails dimmed and truncates from its start. -->
{#if lang}
  <span class="lang-chip mono shrink-0 {reviewed ? 'opacity-60' : ''}">{lang}</span>
{/if}
<!-- The name may shrink (to its truncation) but keeps priority over the
     directory, which gives up its width first. -->
<span class="mono text-xs font-medium text-fg min-w-[4ch] max-w-[60%] truncate {reviewed ? 'opacity-60' : ''}" style="flex-shrink:0.25" {title}>
  {#if rename}
    <span class="text-fg-3">{rename.old[0]}<mark class="mark-old">{rename.old[1]}</mark>{rename.old[2]}</span><span class="text-fg-3 mx-1.5">→</span>{rename.new[0]}<mark class="mark-new">{rename.new[1]}</mark>{rename.new[2]}
  {:else}
    {pathParts.name}
  {/if}
</span>
<div class="mono text-[11px] flex flex-1 min-w-0 items-center overflow-hidden" {title}>
  {#if pathParts.dir && !rename?.crossDir}
    <span class="text-muted truncate-start min-w-0 flex-1">
      <span class="truncate-start-inner">{pathParts.dir}</span>
    </span>
  {/if}
</div>
{#if row.kind === "test"}
  <span class="chip chip-test mono">TEST</span>
{/if}
{#if row.isNew}
  <span class="chip chip-new">new</span>
{/if}
<!-- No "local" chip yet: FileSnapshot carries no per-file unpushed signal, and
     CommitSummary.unpushed has no file list to derive one from. -->

<!-- +N/−N totals -->
{#if documentKind}
  <div class="doc-modes flex shrink-0 gap-1 text-xs" role="group" aria-label="Document view">
    {#each viewModes as [mode, label] (mode)}
      <button type="button" aria-pressed={viewMode === mode}
        class="rounded px-1.5 py-1 hover:bg-hover {viewMode === mode ? 'bg-hover text-fg' : 'text-fg-3'}"
        onclick={(event) => { event.stopPropagation(); onpreviewchange?.(row.filePath, mode); }}>
        {label}
      </button>
    {/each}
  </div>
{/if}
{#if row.oldPath && row.additions === 0 && row.deletions === 0}
  <span class="mono text-xs text-fg-3 shrink-0" title="Renamed">R</span>
{:else}
  <span class="mono text-xs text-add-fg shrink-0">+{row.additions}</span>
  <span class="mono text-xs text-del-fg shrink-0">−{row.deletions}</span>
{/if}

<!-- Reviewed toggle, labelled: an icon-only box was easy to miss. -->
<button
  type="button"
  role="checkbox"
  aria-checked={reviewed}
  aria-label="Mark as reviewed"
  onclick={toggleReviewed}
  title={reviewed ? "Marked reviewed — click to unmark" : "Mark file reviewed"}
  class="shrink-0 h-6 px-1.5 rounded flex items-center gap-1.5 text-xs transition hover:bg-hover group
    {reviewed ? 'text-periwinkle' : 'text-fg-3 hover:text-fg'}"
>
  <span
    class="w-3.5 h-3.5 rounded-[3px] flex items-center justify-center border
      {reviewed ? 'bg-periwinkle border-periwinkle text-on-accent' : 'border-muted text-transparent group-hover:border-fg-3'}"
  >
    <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  </span>
  <span class="rev-label">Reviewed</span>
</button>

<!-- Open source (icon only) -->
<button
  type="button"
  onclick={openSource}
  title="Open in editor"
  aria-label="Open in editor"
  class="shrink-0 w-6 h-6 rounded flex items-center justify-center text-fg-3 hover:bg-hover hover:text-fg transition"
>
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
    <polyline points="15 3 21 3 21 9" />
    <line x1="10" y1="14" x2="21" y2="3" />
  </svg>
</button>

<style>
  .lang-chip { font-size: 9.5px; font-weight: 500; letter-spacing: .02em; color: var(--color-fg-2); border: 1px solid var(--color-border); border-radius: 4px; padding: 0 4px; line-height: 15px; }
  .chip { display: inline-flex; align-items: center; height: 16px; padding: 0 5px; border-radius: 5px; font-size: 10px; font-weight: 500; flex: none; }
  .chip-test { color: var(--color-info); background: color-mix(in srgb, var(--color-info) 13%, transparent); }
  .chip-new { color: var(--color-fg-2); background: var(--color-hover); }
  .mark-old { background: color-mix(in srgb, var(--color-del-fg) 22%, transparent); color: inherit; border-radius: 2px; }
  .mark-new { background: color-mix(in srgb, var(--color-add-fg) 22%, transparent); color: inherit; border-radius: 2px; }
  /* Narrow widths keep the controls, not their labels: the checkbox stays named
     through aria-label, and the document view falls back to its current mode. */
  @media (max-width: 520px) { .rev-label { display: none; } }
  @media (max-width: 880px) { .doc-modes { display: none; } }
</style>
