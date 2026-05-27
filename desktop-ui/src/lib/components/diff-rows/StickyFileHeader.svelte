<script lang="ts">
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }> | null;
    /** When true, hide the overlay (real file-header row is in viewport top band). */
    hidden?: boolean;
  }
  const { row, hidden = false }: Props = $props();

  const collapsed = $derived(row ? diffFileCollapse.collapsed.has(row.filePath) : false);

  function onCollapseCheckboxClick(e: MouseEvent) {
    if (!row) return;
    e.stopPropagation();
    e.preventDefault();
    diffFileCollapse.toggle(row.filePath);
  }
</script>

<!-- Always in DOM so it doesn't shift .hscroll layout when toggling visibility. -->
<div
  class="sticky top-0 z-30 isolate h-10 px-4 border-b border-hairline bg-ink-800 flex items-center gap-3 shrink-0 pointer-events-auto"
  style={hidden || !row ? "visibility:hidden" : ""}
>
  {#if row}
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
  {/if}
</div>
