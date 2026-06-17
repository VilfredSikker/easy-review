<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot, PillarSnapshot } from "$lib/types";

  interface Props {
    collapsed?: boolean;
  }
  const { collapsed = false }: Props = $props();

  const snapshot = $derived(app.snapshot);
  const tour = $derived(snapshot?.tour ?? null);
  const pillars = $derived(tour?.pillars ?? []);

  /** path → FileSnapshot for source_index lookup + +/- counts + reviewed state. */
  const fileByPath = $derived.by(() => {
    const m = new Map<string, FileSnapshot>();
    for (const f of snapshot?.files ?? []) m.set(f.path, f);
    return m;
  });

  const selectedPath = $derived(
    snapshot?.files?.[snapshot?.selected_file ?? 0]?.path ?? null,
  );

  function jumpToFile(path: string) {
    const f = fileByPath.get(path);
    if (f) void app.cmd("select_file", { idx: f.source_index });
  }

  function reviewAll(p: PillarSnapshot) {
    void app.cmd("bulk_review_pillar", { pillarId: p.id });
  }

  function unreviewAll(p: PillarSnapshot) {
    void app.cmd("unbulk_review_pillar", { pillarId: p.id });
  }
</script>

{#if !collapsed}
  <div class="flex flex-col h-full min-h-0 w-[var(--tree-w,18rem)] shrink-0 border-r border-hairline bg-surface">
    <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 shrink-0">
      <span class="text-[10px] uppercase tracking-[0.06em] font-semibold text-muted">Guide</span>
      <span class="px-[5px] py-0 rounded-full text-[9px] text-muted" style="background: color-mix(in srgb, var(--color-fg) 6%, transparent);">{pillars.length}</span>
      {#if tour && !tour.fresh}
        <span class="text-[9px] text-risk-med">stale</span>
      {/if}
      <div class="flex-1"></div>
      <button
        class="text-[10px] text-fg-3 hover:text-fg-2"
        title="Switch back to the plain diff"
        onclick={() => void app.cmd("set_mode", { mode: "branch" })}
      >Diff →</button>
    </div>

    <div class="flex-1 overflow-y-auto">
      {#each pillars as pillar (pillar.id)}
        {@const allReviewed = pillar.totalCount > 0 && pillar.reviewedCount === pillar.totalCount}
        <div class="border-b border-hairline">
          <div class="px-3 pt-2.5 pb-1.5">
            <div class="flex items-center gap-1.5">
              {#if pillar.foundation}
                <span class="text-[10px] text-accent" title="Foundation">◆</span>
              {/if}
              <span class="text-[12px] font-semibold text-fg flex-1 leading-tight">{pillar.title}</span>
              {#if allReviewed}
                <span class="text-[9px] px-[5px] py-[1px] rounded-full text-add-fg" style="background: var(--color-add-bg);">Reviewed</span>
              {:else}
                <span class="mono text-[10px] text-muted">{String(pillar.reviewedCount).padStart(2, "0")}/{String(pillar.totalCount).padStart(2, "0")}</span>
              {/if}
            </div>
            {#if pillar.descriptionMarkdown}
              <p class="text-[11px] text-fg-2 mt-1 leading-snug whitespace-pre-wrap">{pillar.descriptionMarkdown}</p>
            {/if}
            <button
              class="mt-1.5 text-[10px] px-1.5 py-[2px] rounded border border-hairline text-fg-2 hover:bg-card"
              onclick={() => (allReviewed ? unreviewAll(pillar) : reviewAll(pillar))}
            >{allReviewed ? "Unreview all" : `Review all ${pillar.totalCount} files`}</button>
          </div>

          <div class="pb-1.5">
            {#each pillar.files as tf (tf.path)}
              {@const f = fileByPath.get(tf.path)}
              {@const isSelected = tf.path === selectedPath}
              {@const isReviewed = f?.reviewed ?? false}
              <button
                class="w-full text-left px-3 py-[3px] flex items-center gap-1.5 relative {isSelected ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
                title={tf.reason || tf.path}
                disabled={!f}
                onclick={() => jumpToFile(tf.path)}
              >
                {#if isSelected}
                  <span class="absolute left-0 top-[3px] bottom-[3px] w-[2px] rounded-r bg-accent"></span>
                {/if}
                {#if isReviewed}
                  <span class="text-[10px] text-add-fg shrink-0">✓</span>
                {:else}
                  <span class="w-[10px] shrink-0"></span>
                {/if}
                <span class="text-[11px] truncate flex-1 {isReviewed ? 'text-fg-3' : ''} {f ? '' : 'opacity-50'}">{tf.path.split("/").pop()}</span>
                {#if f}
                  {#if f.additions > 0}<span class="mono text-[9px] text-add-fg">+{f.additions}</span>{/if}
                  {#if f.deletions > 0}<span class="mono text-[9px] text-del-fg">−{f.deletions}</span>{/if}
                {/if}
              </button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}
