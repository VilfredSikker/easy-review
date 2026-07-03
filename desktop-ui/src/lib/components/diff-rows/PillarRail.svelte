<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot } from "$lib/types";
  import type { PillarHeaderInfo } from "$lib/diffRenderModel";

  interface Props {
    info: PillarHeaderInfo;
    fileRows: FileSnapshot[];
    /** Primary file path → its co-located related rows (test/style/…). */
    relatedRows?: Map<string, { file: FileSnapshot; kind: string }[]>;
    selectedPath: string | null;
  }
  const { info, fileRows, relatedRows, selectedPath }: Props = $props();

  function kindLabel(kind: string): string {
    switch (kind) {
      case "test":
        return "test";
      case "style":
        return "style";
      case "story":
        return "story";
      case "snapshot":
        return "snap";
      default:
        return "";
    }
  }

  const allReviewed = $derived(info.totalCount > 0 && info.reviewedCount === info.totalCount);

  function reviewAll() {
    if (allReviewed) void app.cmd("unbulk_review_pillar", { pillarId: info.pillarId });
    else void app.cmd("bulk_review_pillar", { pillarId: info.pillarId });
  }

  function jumpToFile(f: FileSnapshot) {
    void app.cmd("select_file", { idx: f.source_index });
  }

  function toggleReviewed(e: MouseEvent, f: FileSnapshot) {
    e.stopPropagation();
    e.preventDefault();
    // Guide mode auto-collapses a file on its reviewed false→true transition
    // (see FlatDiffView), so no explicit collapse is needed here.
    void app.cmd(f.reviewed ? "unmark_reviewed" : "mark_reviewed", { path: f.path });
  }

  function basename(path: string): string {
    const i = path.lastIndexOf("/");
    return i >= 0 ? path.slice(i + 1) : path;
  }
</script>

<div class="px-4 py-3 max-h-[80vh] overflow-y-auto flex flex-col gap-2 bg-bg">
  <div class="flex items-center gap-1.5">
    {#if info.foundation}
      <span class="text-[11px] text-accent shrink-0" title="Foundation">◆</span>
    {/if}
    <h3 class="text-[13px] font-semibold text-fg leading-tight flex-1 min-w-0">{info.title}</h3>
  </div>

  <div class="flex items-center gap-2">
    {#if allReviewed}
      <span class="text-[10px] px-1.5 py-[1px] rounded-full text-add-fg" style="background: var(--color-add-bg);">Reviewed</span>
    {:else}
      <span class="mono text-[11px] text-muted">{String(info.reviewedCount).padStart(2, "0")}/{String(info.totalCount).padStart(2, "0")} reviewed</span>
    {/if}
    <button
      class="ml-auto text-[10px] px-1.5 py-[2px] rounded border border-hairline text-fg-2 hover:bg-card"
      onclick={reviewAll}
    >{allReviewed ? "Unreview all" : "Review all"}</button>
  </div>

  {#if info.descriptionMarkdown}
    <p class="text-[12px] text-fg-2 leading-snug whitespace-pre-wrap">{info.descriptionMarkdown}</p>
  {/if}

  <div class="flex flex-col gap-0.5 mt-1">
    {#each fileRows as f (f.path)}
      {@render fileRow(f, false, "")}
      {#each relatedRows?.get(f.path) ?? [] as child (child.file.path)}
        {@render fileRow(child.file, true, child.kind)}
      {/each}
    {/each}
  </div>
</div>

<!-- Row is a plain container holding two sibling buttons (reviewed-toggle +
     jump-to-file). Mirrors FileHeaderContent: no role=button wrapping an
     interactive control, so each button keeps native keyboard activation.
     `nested` rows are co-located related files (tests/styles/…) indented under
     their primary file. -->
{#snippet fileRow(f: FileSnapshot, nested: boolean, kind: string)}
  {@const isSelected = f.path === selectedPath}
  <div
    class="w-full px-1.5 py-[3px] rounded flex items-center gap-1.5 relative {nested
      ? 'pl-5'
      : ''} {isSelected ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
  >
    {#if isSelected}
      <span class="absolute left-0 top-[3px] bottom-[3px] w-[2px] rounded-r bg-accent"></span>
    {/if}
    {#if nested}
      <span class="text-fg-3 text-[10px] shrink-0 -mr-0.5" aria-hidden="true">↳</span>
    {/if}
    <button
      type="button"
      class="shrink-0 w-[14px] h-[14px] rounded-[3px] flex items-center justify-center border transition
        {f.reviewed ? 'bg-periwinkle border-periwinkle text-on-accent' : 'border-ink-500 text-transparent hover:border-fg-3'}"
      title={f.reviewed ? "Marked reviewed — click to unmark" : "Mark file reviewed"}
      aria-label={f.reviewed ? "Unmark as reviewed" : "Mark as reviewed"}
      aria-pressed={f.reviewed}
      onclick={(e) => toggleReviewed(e, f)}
    >
      <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
        <polyline points="20 6 9 17 4 12" />
      </svg>
    </button>
    <button
      type="button"
      class="flex items-center gap-1.5 flex-1 min-w-0 text-left"
      title={f.path}
      onclick={() => jumpToFile(f)}
    >
      <span class="text-[11px] truncate flex-1 {f.reviewed ? 'text-fg-3 line-through' : ''}">{basename(f.path)}</span>
      {#if nested && kindLabel(kind)}
        <span class="mono text-[8px] uppercase tracking-wide text-fg-3 px-1 rounded bg-card shrink-0">{kindLabel(kind)}</span>
      {/if}
      {#if f.additions > 0}<span class="mono text-[9px] text-add-fg">+{f.additions}</span>{/if}
      {#if f.deletions > 0}<span class="mono text-[9px] text-del-fg">−{f.deletions}</span>{/if}
    </button>
  </div>
{/snippet}
