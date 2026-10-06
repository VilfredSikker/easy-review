<script lang="ts">
  import type { DocumentViewMode } from "$lib/documentPreviewCache";
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }> | null;
    /** When true, hide the overlay (real file-header row is in viewport top band). */
    hidden?: boolean;
    /** Left inset in px. Guide mode passes the pillar rail width so the overlay
        aligns with the diff column instead of spanning across the rail lane. */
    offsetLeftPx?: number;
    previewPaths?: ReadonlySet<string>;
    sidePaths?: ReadonlySet<string>;
    onpreviewchange?: (path: string, mode: DocumentViewMode) => void;
  }
  const { row, hidden = false, offsetLeftPx = 0, previewPaths, sidePaths, onpreviewchange }: Props = $props();
</script>

<!-- Always in DOM so it doesn't shift .hscroll layout when toggling visibility. -->
<div
  class="sticky top-0 z-30 isolate max-w-full overflow-hidden h-10 px-3 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 {hidden || !row
    ? 'pointer-events-none invisible'
    : 'pointer-events-auto'}"
  style={offsetLeftPx > 0
    ? `margin-left:${offsetLeftPx}px;width:calc(100% - ${offsetLeftPx}px);`
    : "width:100%;"}
>
  {#if row}
    <FileHeaderContent {row} {previewPaths} {sidePaths} {onpreviewchange} />
  {/if}
</div>
