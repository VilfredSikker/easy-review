<script lang="ts">
  import { tick } from "svelte";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
  }
  const { row }: Props = $props();

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

<!-- Breadcrumb path: dir muted (truncate start), filename always visible -->
<div class="mono text-xs flex flex-1 min-w-0 items-center overflow-hidden" title={row.filePath}>
  {#if pathParts.dir}
    <span class="text-muted truncate-start min-w-0 flex-1">
      <span class="truncate-start-inner">{pathParts.dir}</span>
    </span>
  {/if}
  <span class="text-fg font-medium shrink-0">{pathParts.name}</span>
</div>

<!-- +N/−N totals -->
<span class="mono text-xs text-add-fg shrink-0">+{row.additions}</span>
<span class="mono text-xs text-del-fg shrink-0">−{row.deletions}</span>

<!-- Reviewed toggle (icon only) -->
<button
  type="button"
  onclick={toggleReviewed}
  title={reviewed ? "Marked reviewed — click to unmark" : "Mark file reviewed"}
  aria-label={reviewed ? "Unmark as reviewed" : "Mark as reviewed"}
  aria-pressed={reviewed}
  class="shrink-0 w-6 h-6 rounded flex items-center justify-center transition hover:bg-hover
    {reviewed ? 'text-periwinkle' : 'text-fg-3 hover:text-fg'}"
>
  <span
    class="w-3.5 h-3.5 rounded-[3px] flex items-center justify-center border
      {reviewed ? 'bg-periwinkle border-periwinkle text-white' : 'border-ink-500 text-transparent'}"
  >
    <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  </span>
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
