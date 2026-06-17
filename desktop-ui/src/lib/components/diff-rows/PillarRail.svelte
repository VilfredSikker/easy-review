<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot } from "$lib/types";
  import type { PillarHeaderInfo } from "$lib/diffRenderModel";

  interface Props {
    info: PillarHeaderInfo;
    fileRows: FileSnapshot[];
    selectedPath: string | null;
  }
  const { info, fileRows, selectedPath }: Props = $props();

  const allReviewed = $derived(info.totalCount > 0 && info.reviewedCount === info.totalCount);

  function reviewAll() {
    if (allReviewed) void app.cmd("unbulk_review_pillar", { pillarId: info.pillarId });
    else void app.cmd("bulk_review_pillar", { pillarId: info.pillarId });
  }

  function jumpToFile(f: FileSnapshot) {
    void app.cmd("select_file", { idx: f.source_index });
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
      {@const isSelected = f.path === selectedPath}
      <button
        class="w-full text-left px-1.5 py-[3px] rounded flex items-center gap-1.5 relative {isSelected ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
        title={f.path}
        onclick={() => jumpToFile(f)}
      >
        {#if isSelected}
          <span class="absolute left-0 top-[3px] bottom-[3px] w-[2px] rounded-r bg-accent"></span>
        {/if}
        {#if f.reviewed}
          <span class="text-[10px] text-add-fg shrink-0">✓</span>
        {:else}
          <span class="w-[10px] shrink-0"></span>
        {/if}
        <span class="text-[11px] truncate flex-1 {f.reviewed ? 'text-fg-3' : ''}">{basename(f.path)}</span>
        {#if f.additions > 0}<span class="mono text-[9px] text-add-fg">+{f.additions}</span>{/if}
        {#if f.deletions > 0}<span class="mono text-[9px] text-del-fg">−{f.deletions}</span>{/if}
      </button>
    {/each}
  </div>
</div>
