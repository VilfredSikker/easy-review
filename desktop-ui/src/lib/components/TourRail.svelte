<script lang="ts">
  import { baseName, shortFileName } from "$lib/shortFileName";
  import { pad2, type RailFile, type RailPillar } from "$lib/tourModel";
  import type { FileSnapshot, PillarSnapshot } from "$lib/types";
  import TourRuler from "./TourRuler.svelte";

  interface Props {
    pillars: RailPillar[];
    /** Expanded pillar: the one under the reading line. */
    activePillarId: string | null;
    /** File under the reading line; gets the accent bar. */
    currentPath: string | null;
    onpillar: (pillarId: string) => void;
    onjump: (path: string) => void;
    ontogglefile: (file: FileSnapshot) => void;
    /** `allReviewed` true clears the pillar, false reviews every file in it. */
    onreviewpillar: (pillarId: string, allReviewed: boolean) => void;
    /** The tour and the files the column draws, for the ruler beside the
     *  rail; without them the rail stands alone (stories). */
    rulerPillars?: readonly PillarSnapshot[];
    rulerFiles?: readonly FileSnapshot[];
  }
  const {
    pillars,
    activePillarId,
    currentPath,
    onpillar,
    onjump,
    ontogglefile,
    onreviewpillar,
    rulerPillars,
    rulerFiles,
  }: Props = $props();

  let railEl: HTMLElement | null = $state(null);

  // Scroll-spy moves the active pillar as the column scrolls; keep it in view
  // when the accordion is taller than the rail.
  $effect(() => {
    const id = activePillarId;
    if (!railEl || id === null) return;
    railEl.querySelector<HTMLElement>(`[data-pillar-nav="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" });
  });

  function relatedLabel(kind: string): string {
    if (kind === "snapshot") return "snap";
    return kind === "other" ? "" : kind;
  }
</script>

<aside class="tour-rail" aria-label="Pillars" bind:this={railEl}>
  {#each pillars as p (p.id)}
    {@const active = p.id === activePillarId}
    {@const allReviewed = p.total > 0 && p.reviewed >= p.total}
    <div class="pillar-nav" class:active data-pillar-nav={p.id}>
      <button type="button" class="ph" aria-expanded={active} onclick={() => onpillar(p.id)}>
        <span class="ptitle" title={p.title}>{p.title}</span>
        <span class="pcount" title="{p.reviewed} of {p.total} files reviewed">{pad2(p.reviewed)}/{pad2(p.total)}</span>
      </button>
      {#if active}
        {#if p.summary}
          <div class="pdesc">{p.summary}</div>
        {/if}
        <div class="ptools">
          <span>{p.reviewed} of {p.total} reviewed</span>
          <button type="button" class="linkbtn" onclick={() => onreviewpillar(p.id, allReviewed)}>
            {allReviewed ? "Clear" : "Review all"}
          </button>
        </div>
        <div class="pfiles">
          {#each p.files as row (row.file.path)}
            {@render fileRow(row)}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
  <div class="rail-foot">
    <span><kbd>j</kbd> <kbd>k</kbd> next / previous file</span>
    <span><kbd>]</kbd> next pillar</span>
    <span><kbd>r</kbd> mark reviewed</span>
  </div>
</aside>
<!-- A sibling, not a child: the rail scrolls and clips, and the ruler's
     popover must open over the column. It takes its own strip between rail
     and column, so its ticks never sit on the column's code. -->
{#if rulerPillars && rulerFiles}
  <TourRuler pillars={rulerPillars} files={rulerFiles} {currentPath} onGoto={onjump} />
{/if}

<!-- Two sibling buttons, as in the file rail: the checkbox toggles reviewed,
     the rest of the row jumps, and neither wraps the other. -->
{#snippet fileRow(row: RailFile)}
  {@const f = row.file}
  {@const short = shortFileName(baseName(f.path))}
  {@const renamed = f.status === "renamed" || !!f.old_path}
  <div class="frow" class:done={f.reviewed} class:current={f.path === currentPath} class:nested={row.relatedKind !== null}>
    <button
      type="button"
      class="cb"
      role="checkbox"
      aria-checked={f.reviewed}
      aria-label={f.reviewed ? "Unmark as reviewed" : "Mark as reviewed"}
      title={f.reviewed ? "Reviewed — click to unmark" : "Mark reviewed"}
      onclick={() => ontogglefile(f)}
    >
      <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.5" aria-hidden="true">
        <polyline points="20 6 9 17 4 12" />
      </svg>
    </button>
    <button type="button" class="jump" title={f.path} onclick={() => onjump(f.path)}>
      <span class="nm">
        <span class="fn-main">{short.main}</span>
        {#if short.stamp}<span class="fn-ts mono" title="version {short.version}">{short.stamp}</span>{/if}
        {#if row.relatedKind !== null && relatedLabel(row.relatedKind)}
          <span class="chip">{relatedLabel(row.relatedKind)}</span>
        {:else if f.kind === "test"}
          <span class="chip">TEST</span>
        {/if}
      </span>
      <span class="st mono">
        {#if renamed && f.additions === 0 && f.deletions === 0}
          <span class="text-fg-3">R</span>
        {:else}
          <span class="text-add-fg">+{f.additions}</span> <span class="text-del-fg">−{f.deletions}</span>
        {/if}
      </span>
    </button>
  </div>
{/snippet}

<style>
  .tour-rail {
    width: 296px;
    flex: none;
    border-right: 1px solid var(--color-hairline);
    background: var(--color-bg);
    overflow: auto;
    padding: 14px 10px 24px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .pillar-nav {
    border-radius: 8px;
  }
  .pillar-nav.active {
    background: var(--color-surface);
    box-shadow: 0 0 0 1px var(--color-hairline);
    padding-bottom: 6px;
  }
  .ph {
    width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 8px;
    border-radius: 8px;
    text-align: left;
  }
  .ph:hover {
    background: var(--color-hover);
  }
  .ptitle {
    font-weight: 600;
    font-size: 13px;
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .active .ptitle::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 2px;
    background: var(--color-accent);
    margin-right: 7px;
    vertical-align: 2px;
  }
  .pcount {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--color-fg-3);
  }
  .pdesc {
    padding: 0 8px 8px;
    color: var(--color-fg-2);
    font-size: 12px;
    line-height: 1.55;
  }
  .ptools {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px 6px;
    font-size: 11px;
    color: var(--color-fg-3);
  }
  .linkbtn {
    font-size: 11px;
    color: var(--color-fg-2);
    padding: 2px 6px;
    border-radius: 5px;
    border: 1px solid var(--color-border);
  }
  .linkbtn:hover {
    color: var(--color-fg);
  }
  .pfiles {
    display: grid;
  }
  .frow {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    gap: 8px;
    align-items: center;
    padding: 0 8px;
    border-radius: 6px;
    font-size: 12px;
    position: relative;
  }
  .frow.nested {
    padding-left: 22px;
  }
  .frow:hover {
    background: var(--color-hover);
  }
  .frow.current {
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
  }
  .frow.current::before {
    content: "";
    position: absolute;
    left: -2px;
    top: 5px;
    bottom: 5px;
    width: 2px;
    border-radius: 2px;
    background: var(--color-accent);
  }
  .jump {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 4px 0;
    text-align: left;
    min-width: 0;
  }
  .nm {
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
    color: var(--color-fg);
  }
  .fn-main {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .done .fn-main {
    color: var(--color-fg-3);
  }
  .fn-ts {
    flex: none;
    font-size: 10.5px;
    color: var(--color-fg-3);
    padding: 0 4px;
    border-radius: 3px;
    background: var(--color-hover);
    line-height: 15px;
  }
  .chip {
    flex: none;
    font-family: var(--font-mono);
    font-size: 8px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 0 4px;
    border-radius: 3px;
    background: var(--color-card);
    color: var(--color-fg-3);
  }
  .st {
    font-size: 10.5px;
    white-space: nowrap;
  }
  .cb {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1.5px solid var(--color-border);
    display: inline-grid;
    place-items: center;
    background: var(--color-surface);
    color: transparent;
  }
  .cb[aria-checked="true"] {
    background: var(--color-add-fg);
    border-color: var(--color-add-fg);
    color: var(--color-ground);
  }
  .rail-foot {
    margin-top: auto;
    padding: 12px 8px 0;
    color: var(--color-fg-3);
    font-size: 11px;
    display: grid;
    gap: 4px;
  }
  .rail-foot kbd {
    font-family: var(--font-mono);
    font-size: 10px;
    padding: 0 4px;
    border: 1px solid var(--color-border);
    border-radius: 3px;
    color: var(--color-fg-2);
  }
  @media (max-width: 880px) {
    .tour-rail {
      width: auto;
      border-right: 0;
      border-bottom: 1px solid var(--color-hairline);
      max-height: 34vh;
    }
    .rail-foot {
      display: none;
    }
  }
</style>
