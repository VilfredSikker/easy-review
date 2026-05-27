<script lang="ts">
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
    scrollTopPx?: number;
  }
  const { row, scrollTopPx = 0 }: Props = $props();

  const collapsed = $derived.by(() => {
    diffFileCollapse.revision;
    return diffFileCollapse.collapsed.has(row.filePath);
  });

  function toggleCollapse(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    diffFileCollapse.toggle(row.filePath);
  }

  async function toggleReviewed(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    try {
      if (row.reviewed) {
        await app.cmd("unmark_reviewed", { fileIdx: row.fileIndex });
      } else {
        await app.cmd("mark_reviewed", { fileIdx: row.fileIndex });
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

  function scrollPrevHunk(e: MouseEvent) {
    e.stopPropagation();
    diffNav.scrollToAdjacentHunk(row.filePath, "prev", scrollTopPx);
  }

  function scrollNextHunk(e: MouseEvent) {
    e.stopPropagation();
    diffNav.scrollToAdjacentHunk(row.filePath, "next", scrollTopPx);
  }
</script>

<div
  class="h-10 px-3 border-t border-ink-650 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0"
  data-row-identity={row.identity}
>
  <!-- Reviewed checkbox (primary action, leftmost) -->
  <button
    type="button"
    onclick={toggleReviewed}
    title={row.reviewed ? "Marked reviewed — click to unmark" : "Mark file reviewed"}
    aria-label={row.reviewed ? "Unmark as reviewed" : "Mark as reviewed"}
    aria-pressed={row.reviewed}
    class="shrink-0 w-4 h-4 rounded flex items-center justify-center border transition
      {row.reviewed
      ? 'bg-periwinkle border-periwinkle text-white'
      : 'border-ink-500 bg-transparent text-transparent hover:border-periwinkle'}"
  >
    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  </button>

  <!-- Collapse caret -->
  <button
    type="button"
    class="shrink-0 p-0.5 text-fg-3 hover:bg-hover rounded flex items-center justify-center"
    title={collapsed ? "Expand file" : "Collapse file"}
    aria-label={collapsed ? "Expand file diff" : "Collapse file diff"}
    aria-expanded={!collapsed}
    onclick={toggleCollapse}
  >
    <svg
      width="10"
      height="10"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
      class="transition-transform {collapsed ? '' : 'rotate-90'}"
    >
      <polyline points="9 18 15 12 9 6" />
    </svg>
  </button>

  <!-- Breadcrumb path: dir muted, filename emphasized -->
  <div class="mono text-xs truncate flex-1 min-w-0">
    {#if pathParts.dir}
      <span class="text-muted">{pathParts.dir}</span>
    {/if}
    <span class="text-fg font-medium">{pathParts.name}</span>
  </div>

  <!-- +N/−N totals -->
  <span class="mono text-xs text-add-fg shrink-0">+{row.additions}</span>
  <span class="mono text-xs text-del-fg shrink-0">−{row.deletions}</span>

  <!-- Hunk nav arrows -->
  <div class="flex items-center gap-0.5 ml-1 shrink-0">
    <button
      type="button"
      title="Previous hunk (k)"
      aria-label="Previous hunk"
      onclick={scrollPrevHunk}
      class="w-5 h-5 rounded flex items-center justify-center text-fg-3 hover:bg-hover hover:text-fg transition"
    >
      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <polyline points="18 15 12 9 6 15" />
      </svg>
    </button>
    <button
      type="button"
      title="Next hunk (j)"
      aria-label="Next hunk"
      onclick={scrollNextHunk}
      class="w-5 h-5 rounded flex items-center justify-center text-fg-3 hover:bg-hover hover:text-fg transition"
    >
      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </button>
  </div>

  <!-- Open source button -->
  <button
    type="button"
    onclick={openSource}
    title="Open in editor"
    class="shrink-0 flex items-center gap-1 px-2 h-6 rounded text-xs text-fg-3 hover:bg-hover hover:text-fg transition"
  >
    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
      <polyline points="15 3 21 3 21 9" />
      <line x1="10" y1="14" x2="21" y2="3" />
    </svg>
    Open source
  </button>
</div>
