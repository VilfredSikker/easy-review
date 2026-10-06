<script lang="ts">
  import type { DocumentViewMode } from "$lib/documentPreviewCache";
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
    /** When the global sticky header overlay is active, ignore in-flow header clicks. */
    pointerEventsNone?: boolean;
    previewPaths?: ReadonlySet<string>;
    sidePaths?: ReadonlySet<string>;
    onpreviewchange?: (path: string, mode: DocumentViewMode) => void;
  }
  const { row, pointerEventsNone = false, previewPaths, sidePaths, onpreviewchange }: Props = $props();
</script>

<div
  class="file-header-viewport-row diff-viewport-row h-10 px-3 border-t border-ink-650 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 {pointerEventsNone
    ? 'pointer-events-none'
    : ''}"
  data-row-identity={row.identity}
>
  <FileHeaderContent {row} {previewPaths} {sidePaths} {onpreviewchange} />
</div>
