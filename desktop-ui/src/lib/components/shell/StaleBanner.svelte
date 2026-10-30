<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { oidSegments, staleBannerCopy } from "$lib/shellBanners";
  import { diffSync } from "./diffSync.svelte";

  /** Under the view bar while the open diff is behind origin (`diff_stale`). */
  const stale = $derived(app.snapshot?.diff_stale ?? null);
  const copy = $derived(stale ? staleBannerCopy(stale, app.snapshot?.base ?? null) : null);
</script>

{#if copy}
  <div
    class="flex shrink-0 flex-wrap items-center gap-2.5 border-b border-line bg-[color-mix(in_srgb,var(--color-warning)_9%,var(--color-bg))] px-3.5 py-2 text-[12.5px] text-fg-2"
    role="status"
    data-testid="stale-banner"
  >
    <span class="font-semibold text-warning">Stale</span>
    <span class="min-w-0 flex-1">
      {#each oidSegments(copy.message, [copy.latestShort, copy.builtShort]) as seg, i (i)}
        {#if seg.code}<code class="rounded bg-card px-1 font-mono text-[11.5px] text-fg">{seg.text}</code>{:else}{seg.text}{/if}
      {/each}
    </span>
    <button
      type="button"
      class="ml-auto inline-flex h-6 shrink-0 items-center gap-1.5 rounded-[7px] border border-border bg-surface px-2.5 text-[12px] font-medium text-fg hover:border-[color-mix(in_srgb,var(--color-fg)_25%,transparent)] disabled:cursor-default disabled:opacity-60"
      disabled={diffSync.syncing}
      onclick={() => void diffSync.sync()}
    >
      {diffSync.syncing ? "Syncing…" : `Sync to ${copy.latestShort}`}
    </button>
  </div>
{/if}
