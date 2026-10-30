<script lang="ts">
  import { tick } from "svelte";
  import type { PillarSnapshot, TourSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { shortAge } from "$lib/relativeTime";

  interface Props {
    /** Defaults to the live snapshot's tour; stories pass one. */
    tour?: TourSnapshot | null;
  }

  const { tour: tourOverride }: Props = $props();
  const tour = $derived(tourOverride !== undefined ? tourOverride : (app.snapshot?.tour ?? null));
  const pillars = $derived(tour?.available ? tour.pillars : []);
  const age = $derived(tour?.createdAt ? shortAge(tour.createdAt) : "");

  /** One line for the list: the pillar's summary, or the first line of its
   *  description on a tour written before pillars had one, with the inline
   *  markdown marks dropped since the row truncates rather than renders. */
  function summaryLine(p: PillarSnapshot): string {
    const source = p.summaryMarkdown?.trim() || p.descriptionMarkdown;
    const first = source.split(/\n\s*\n/u)[0] ?? "";
    return first
      .replace(/\[([^\]]+)\]\([^)]*\)/gu, "$1")
      .replace(/[`*_]/gu, "")
      .replace(/\s+/gu, " ")
      .trim();
  }

  function progress(p: PillarSnapshot): number {
    return p.totalCount > 0 ? Math.min(1, p.reviewedCount / p.totalCount) : 0;
  }

  async function openPillar(p: PillarSnapshot) {
    reviewPage.show("tour");
    const first = p.files[0]?.path;
    if (!first) return;
    await tick();
    await diffNav.scrollToFile(first);
  }
</script>

<section aria-label="Tour" data-testid="overview-tour">
  <div class="mb-2.5 flex items-center gap-2.5">
    <h2 class="m-0 text-[11px] font-medium uppercase tracking-[.06em] text-fg-3">Tour</h2>
    {#if tour?.available && !tour.fresh}
      <span
        class="inline-flex h-5 items-center rounded-[5px] bg-warning/14 px-[7px] text-[11px] font-medium text-warning"
        title="The tour was generated for an older diff"
      >stale</span>
    {:else if tour?.available && age}
      <span class="mono text-[11px] text-fg-3">fresh · {age}</span>
    {/if}
    <span class="flex-1"></span>
    {#if tour?.available}
      <button
        type="button"
        class="inline-flex h-7 items-center gap-1.5 rounded-[7px] border border-border bg-surface px-2.5 text-[12.5px] font-medium text-fg hover:border-fg-3"
        disabled={!reviewPage.available("tour")}
        onclick={() => reviewPage.show("tour")}
      >Start tour <kbd class="mono text-[10px] text-fg-3">2</kbd></button>
    {/if}
  </div>

  {#if pillars.length === 0}
    <div class="flex flex-wrap items-center gap-3 rounded-[10px] border border-line bg-surface px-4 py-3.5 text-[12.5px] text-fg-3">
      <span class="flex-1">No tour for this diff yet.</span>
      <button
        type="button"
        class="inline-flex h-7 items-center rounded-[7px] border border-border bg-surface px-2.5 text-[12.5px] font-medium text-fg hover:border-fg-3"
        onclick={() => void app.cmd("generate_tour")}
      >Generate tour</button>
    </div>
  {:else}
    <div class="grid overflow-hidden rounded-[10px] border border-line bg-surface {tour?.fresh ? '' : 'opacity-[.72]'}">
      {#each pillars as pillar, i (pillar.id)}
        <button
          type="button"
          class="grid grid-cols-[34px_minmax(0,1fr)_auto] items-center gap-3 border-t border-line px-4 py-3 text-left first:border-t-0 hover:bg-hover"
          data-testid="overview-tour-row"
          onclick={() => void openPillar(pillar)}
        >
          <span class="mono text-[11px] text-fg-3">{i + 1} / {pillars.length}</span>
          <span class="min-w-0">
            <span class="block font-medium text-fg">{pillar.title}</span>
            <span class="block truncate text-[12px] text-fg-2">{summaryLine(pillar)}</span>
          </span>
          <span class="flex items-center gap-2">
            <span class="mono text-[11px] text-fg-3">{pillar.reviewedCount}/{pillar.totalCount}</span>
            <span class="block h-1 w-16 overflow-hidden rounded-sm bg-hairline" aria-hidden="true">
              <i class="block h-full rounded-[inherit] bg-add-fg transition-[width] duration-200" style:width="{progress(pillar) * 100}%"></i>
            </span>
          </span>
        </button>
      {/each}
    </div>
  {/if}
</section>
