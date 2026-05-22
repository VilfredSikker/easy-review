<script lang="ts">
  interface Props {
    filePath: string | null;
    /** When true, hide the overlay (real file-header row is in viewport top band). */
    hidden?: boolean;
  }
  const { filePath, hidden = false }: Props = $props();
</script>

<!-- Always in DOM so it doesn't shift .hscroll layout when toggling visibility. -->
<div
  class="sticky top-0 z-20 h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-3 shrink-0 text-muted pointer-events-none"
  style={hidden || !filePath ? "visibility:hidden" : ""}
>
  <span class="mono text-sm text-fg-2 truncate">{filePath ?? ""}</span>
</div>
