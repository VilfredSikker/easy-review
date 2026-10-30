<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import { tourEmptyCopy } from "$lib/tourEmpty";

  /** The Tour page for a view with no tour: say so, and offer the ways out. */
  const snapshot = $derived(app.snapshot);
  const copy = $derived(tourEmptyCopy(snapshot?.other_bucket));
  let generating = $state(false);

  async function generate() {
    generating = true;
    try {
      await app.cmd("generate_tour");
    } finally {
      generating = false;
    }
  }

  // The tour lives on the other view of this tab, so land on that view's Tour
  // rather than its Diff: leaving Tour picks the source, entering it again
  // reads that source's tour.
  async function switchViews(to: "pr" | "branch") {
    const prNumber = resolveActivePrNumber(snapshot);
    await app.cmd("set_mode", to === "pr" ? { mode: "pr_diff", prNumber } : { mode: "branch" });
    await app.cmd("set_mode", { mode: "tour" });
  }

  const btn =
    "inline-flex h-7 items-center rounded-[7px] border px-3 text-[12.5px] font-medium disabled:cursor-default disabled:opacity-60";
</script>

<div class="flex-1 min-w-0 min-h-0 grid place-items-center bg-bg p-6 text-center" data-testid="tour-empty">
  <div class="grid justify-items-center gap-3.5">
    <div>
      <b class="text-[14px] font-semibold text-fg">{copy.title}</b>
      <p class="mt-1 max-w-[60ch] text-[13px] leading-relaxed text-fg-3">{copy.body}</p>
    </div>
    <div class="flex flex-wrap justify-center gap-2">
      <button
        type="button"
        class="{btn} border-transparent bg-accent text-on-accent hover:bg-accent-hover"
        disabled={generating}
        onclick={() => void generate()}
      >
        {generating ? "Generating…" : "Generate tour"}
      </button>
      {#if copy.switchTo}
        {@const to = copy.switchTo}
        <button type="button" class="{btn} border-transparent text-fg-2 hover:bg-hover hover:text-fg" onclick={() => void switchViews(to)}>
          {copy.switchLabel}
        </button>
      {:else}
        <button type="button" class="{btn} border-transparent text-fg-2 hover:bg-hover hover:text-fg" onclick={() => reviewPage.show("diff")}>
          Go to Diff
        </button>
      {/if}
    </div>
  </div>
</div>
