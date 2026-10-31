<script lang="ts">
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import OverviewView from "$lib/components/overview/OverviewView.svelte";
  import { reviewLayers, type ReviewLayers } from "$lib/stores/reviewPageModel";
  import type { AppSnapshot } from "$lib/types";
  import { applyTheme } from "$lib/themes";

  interface Props {
    snapshot: AppSnapshot;
    /** True draws the shell's layering; false remounts each page, as before F1. */
    keepMounted: boolean;
  }
  const { snapshot, keepMounted }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
    applyTheme(snapshot.theme);
  });

  let view = $state<"diff" | "overview">("diff");
  const layers: ReviewLayers = $derived(
    keepMounted
      ? reviewLayers(view, true)
      : {
          diff: view === "diff" ? "shown" : "absent",
          overview: view === "overview" ? "shown" : "absent",
        },
  );

  let result = $state("");

  const frame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

  /** Median time from asking for the diff to the second frame after it, over 7 returns. */
  async function measure() {
    result = "";
    const samples: number[] = [];
    for (let i = 0; i < 7; i++) {
      view = "overview";
      await tick();
      await frame();
      await frame();
      const t = performance.now();
      view = "diff";
      await tick();
      await frame();
      await frame();
      samples.push(performance.now() - t);
    }
    samples.sort((a, b) => a - b);
    result = `${samples[3].toFixed(1)} ms`;
  }
</script>

<div class="h-screen flex flex-col bg-bg text-fg">
  <div class="flex items-center gap-3 border-b border-hairline px-3 py-2 text-[12px] text-fg-2">
    <button type="button" class="rounded border border-hairline px-2 py-0.5" onclick={() => (view = view === "diff" ? "overview" : "diff")}>
      Show {view === "diff" ? "Overview" : "Diff"}
    </button>
    <button type="button" class="rounded border border-hairline px-2 py-0.5" data-testid="measure" onclick={measure}>
      Measure Overview → Diff
    </button>
    <span data-testid="result">{result}</span>
  </div>
  <div class="grid flex-1 min-w-0 min-h-0 grid-cols-[minmax(0,1fr)] grid-rows-[minmax(0,1fr)]">
    {#if layers.diff !== "absent"}
      {@const hidden = layers.diff === "hidden"}
      <div class="[grid-area:1/1] flex min-w-0 min-h-0" class:invisible={hidden} inert={hidden}>
        <DiffView viewModeOverride="unified" />
      </div>
    {/if}
    {#if layers.overview !== "absent"}
      {@const hidden = layers.overview === "hidden"}
      <div class="[grid-area:1/1] flex min-w-0 min-h-0" class:invisible={hidden} inert={hidden}>
        <OverviewView {snapshot} />
      </div>
    {/if}
  </div>
</div>
