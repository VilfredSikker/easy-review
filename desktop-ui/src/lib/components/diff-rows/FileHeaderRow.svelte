<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
  }
  const { row }: Props = $props();

  const collapsed = $derived(diffFileCollapse.collapsed.has(row.filePath));

  async function toggleReviewed() {
    const snap = app.snapshot;
    const idx = snap?.files.findIndex((f) => f.path === row.filePath) ?? -1;
    if (idx < 0) return;
    const cmd = row.reviewed ? "unmark_reviewed" : "mark_reviewed";
    await app.cmd(cmd, { fileIdx: idx });
    if (!row.reviewed) {
      diffFileCollapse.collapse(row.filePath);
    }
  }

  function onCollapseCheckboxClick(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    diffFileCollapse.toggle(row.filePath);
  }
</script>

<div
  class="h-10 px-4 border-t border-ink-650 border-b border-hairline bg-ink-800 flex items-center gap-3 shrink-0"
  data-row-identity={row.identity}
>
  <input
    type="checkbox"
    class="shrink-0 rounded border-ink-500 text-accent focus:ring-accent/40"
    checked={collapsed}
    title={collapsed ? "Expand file" : "Collapse file"}
    aria-label={collapsed ? "Expand file diff" : "Collapse file diff"}
    onclick={onCollapseCheckboxClick}
  />
  <span class="mono text-sm text-fg truncate">{row.filePath}</span>
  <span class="mono text-xs text-add-fg shrink-0">+{row.additions}</span>
  <span class="mono text-xs text-del-fg shrink-0">−{row.deletions}</span>
  <div class="ml-auto flex items-center gap-1">
    <button
      type="button"
      class="px-2 py-1 text-xs flex items-center gap-1 hover:bg-hover rounded {row.reviewed ? 'text-add-fg' : 'text-fg-3'}"
      onclick={toggleReviewed}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 13l4 4L19 7"/></svg>
      {row.reviewed ? "Unmark" : "Mark reviewed"}
    </button>
  </div>
</div>
