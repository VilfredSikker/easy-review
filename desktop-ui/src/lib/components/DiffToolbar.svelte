<script lang="ts">
  import type { DiffViewMode } from "$lib/stores/app.svelte";
  import type { CommitChip, CommitNote } from "$lib/diffToolbarModel";
  import { commitColorVar } from "$lib/commitColors";

  interface Props {
    viewMode: DiffViewMode;
    onViewMode: (mode: DiffViewMode) => void;
    hideReviewed: boolean;
    onHideReviewed: (on: boolean) => void;
    note: CommitNote;
    /** Leaves the single-commit view for the diff it was opened from. */
    onShowAllChanges: () => void;
    reviewed: number;
    total: number;
  }

  const {
    viewMode,
    onViewMode,
    hideReviewed,
    onHideReviewed,
    note,
    onShowAllChanges,
    reviewed,
    total,
  }: Props = $props();

  const LAYOUTS: { mode: DiffViewMode; label: string }[] = [
    { mode: "unified", label: "Unified" },
    { mode: "split", label: "Split" },
  ];

  function chipColor(chip: CommitChip): string {
    return chip.colorIndex === null ? "var(--color-muted)" : commitColorVar(chip.colorIndex);
  }
</script>

{#snippet shaChip(chip: CommitChip)}
  <span class="shachip mono" style="--cc: {chipColor(chip)}">{chip.sha}</span>
{/snippet}

<div class="shrink-0 flex flex-wrap items-center gap-2.5 px-4 pt-2.5 pb-2 text-[12px]">
  <div class="flex p-[2px] rounded-[7px] bg-hover" role="group" aria-label="Diff layout">
    {#each LAYOUTS as l (l.mode)}
      {@const on = viewMode === l.mode}
      <button
        type="button"
        class="px-[9px] py-[2px] rounded-[5px] font-medium {on
          ? 'bg-card text-fg shadow-[0_0_0_1px_var(--color-border)]'
          : 'text-fg-2 hover:text-fg'}"
        aria-pressed={on}
        onclick={() => onViewMode(l.mode)}
      >
        {l.label}
      </button>
    {/each}
  </div>

  <button
    type="button"
    class="toggle"
    role="switch"
    aria-checked={hideReviewed}
    onclick={() => onHideReviewed(!hideReviewed)}
  >
    <span class="sw"></span>Hide reviewed
  </button>

  <div class="flex-1"></div>

  {#if note?.kind === "commit"}
    <span class="inline-flex items-center gap-1.5 min-w-0">
      {@render shaChip(note.chip)}
      {#if note.title}<span class="truncate text-fg-2" title={note.title}>{note.title}</span>{/if}
      <button type="button" class="linkbtn" onclick={onShowAllChanges}>Show all changes</button>
    </span>
  {:else if note?.kind === "legend"}
    <span class="inline-flex items-center gap-1.5 min-w-0 flex-wrap">
      <span class="text-muted">Lines marked by commit:</span>
      {#each note.chips as chip (chip.sha)}
        {@render shaChip(chip)}
      {/each}
    </span>
  {/if}

  {#if total > 0}
    <span class="text-fg-2">{reviewed} of {total} files reviewed</span>
  {/if}
</div>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--color-fg-2);
  }
  .sw {
    width: 26px;
    height: 16px;
    border-radius: 8px;
    background: var(--color-border);
    position: relative;
    transition: background 0.15s;
  }
  .sw::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-card);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--color-bg) 60%, transparent);
    transition: transform 0.15s;
  }
  .toggle[aria-checked="true"] .sw {
    background: var(--color-accent);
  }
  .toggle[aria-checked="true"] .sw::after {
    transform: translateX(10px);
  }
  .shachip {
    flex: none;
    font-size: 10.5px;
    line-height: 16px;
    padding: 0 5px;
    border-radius: 4px;
    color: var(--cc);
    background: color-mix(in srgb, var(--cc) 15%, transparent);
  }
  .linkbtn {
    flex: none;
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 5px;
    color: var(--color-fg-2);
    border: 1px solid var(--color-border);
  }
  .linkbtn:hover {
    color: var(--color-fg);
  }
</style>
