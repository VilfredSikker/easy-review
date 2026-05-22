<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
  }
  const { row }: Props = $props();

  function toggleReviewed() {
    const cmd = row.reviewed ? "unmark_reviewed" : "mark_reviewed";
    app.cmd(cmd, { fileIdx: row.fileIndex });
  }
</script>

<div
  class="h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-3 shrink-0 text-muted"
  data-row-identity={row.identity}
>
  <span class="mono text-sm text-fg-2 truncate">{row.filePath}</span>
  <span class="mono text-xs text-add-fg shrink-0">+{row.additions}</span>
  <span class="mono text-xs text-del-fg shrink-0">−{row.deletions}</span>
  <div class="ml-auto flex items-center gap-1">
    <button
      class="px-2 py-1 text-xs flex items-center gap-1 hover:bg-hover rounded {row.reviewed ? 'text-add-fg' : 'text-fg-3'}"
      onclick={toggleReviewed}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 13l4 4L19 7"/></svg>
      {row.reviewed ? "Unmark" : "Mark reviewed"}
    </button>
  </div>
</div>
