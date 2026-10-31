<script lang="ts">
  import type { CommitSummary } from "$lib/types";
  import { commitColorVar } from "$lib/commitColors";
  import { commitDiffstat } from "$lib/scopeRail";
  import { timeAgo } from "$lib/time";
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    commits: CommitSummary[];
    selectedSha: string | null;
    /** False on remote PR tabs: no object DB to diff a single commit in. */
    selectable: boolean;
    /** "All changes" in the PR diff view, "Branch" in the local branch view. */
    allLabel: string;
    allActive: boolean;
    /** Totals for the "All changes" row; null when the files on screen are
     *  not that view's (a commit, or a working-tree scope). */
    allStat: { additions: number; deletions: number } | null;
    scopeLabel: string;
    scopeTitle?: string;
    reviewedCount: number;
    totalCount: number;
    onSelectCommit: (sha: string) => void;
    onAllChanges: () => void;
    /** Mark lines state and setter; default to the saved view preference, so
     *  the host only passes them to override it (stories). */
    markLines?: boolean;
    onMarkLines?: (on: boolean) => void;
  }

  const {
    commits,
    selectedSha,
    selectable,
    allLabel,
    allActive,
    allStat,
    scopeLabel,
    scopeTitle,
    reviewedCount,
    totalCount,
    onSelectCommit,
    onAllChanges,
    markLines,
    onMarkLines,
  }: Props = $props();

  const markOn = $derived(markLines ?? app.markLines);
  function toggleMarkLines() {
    if (onMarkLines) onMarkLines(!markOn);
    else app.setMarkLines(!markOn);
  }

  const ON = "bg-[color-mix(in_srgb,var(--color-accent)_9%,var(--color-hover))]";
</script>

<!-- Pinned under the file list, so the commit context stays in view while the
     list scrolls. Colours are by position in the engine's list, the index the
     line attribution uses too. -->
<section class="shrink-0 border-t border-line bg-surface px-2.5 pt-2 pb-1.5 grid gap-[2px]" aria-label="Commits">
  {#if commits.length > 0}
    <button
      type="button"
      class="flex items-center gap-2 px-2 py-1.5 rounded-md text-left text-[12.5px] {allActive ? ON : 'hover:bg-hover'}"
      aria-current={allActive ? "true" : undefined}
      onclick={onAllChanges}
    >
      <span class="w-3.5 text-center text-fg-3" aria-hidden="true">⇡</span>
      <b class="font-semibold text-fg">{allLabel}</b>
      <span class="text-[11px] text-muted">{commits.length} commit{commits.length === 1 ? "" : "s"}</span>
      {#if allStat}
        <span class="ml-auto mono text-[11px]">
          <span class="text-add-fg">+{allStat.additions}</span>
          <span class="text-del-fg">−{allStat.deletions}</span>
        </span>
      {/if}
    </button>

    <div class="flex items-center justify-between px-2 pt-2 pb-[2px] text-[10.5px] font-semibold tracking-[0.06em] uppercase text-muted">
      <span>Commits {commits.length}</span>
      <button
        type="button"
        class="toggle"
        role="switch"
        aria-checked={markOn}
        title="Colour each changed line by the commit that introduced it"
        onclick={toggleMarkLines}
      >
        <span class="sw" aria-hidden="true"></span>Mark lines
      </button>
    </div>

    <div class="grid gap-[2px] max-h-[240px] overflow-y-auto">
      {#each commits as commit, i (commit.sha)}
        {@const on = commit.sha === selectedSha}
        {@const stat = commitDiffstat(commit)}
        <button
          type="button"
          class="grid grid-cols-[8px_minmax(0,1fr)] gap-2 px-2 py-1.5 rounded-md text-left {on ? ON : ''} {selectable
            ? 'hover:bg-hover'
            : 'cursor-default'}"
          style="--cc: {commitColorVar(i)}"
          aria-current={on ? "true" : undefined}
          title={selectable ? commit.title : `${commit.title} (view only on a remote PR)`}
          onclick={() => selectable && onSelectCommit(commit.sha)}
        >
          <span class="w-2 h-2 rounded-full mt-[5px] bg-[var(--cc)]" aria-hidden="true"></span>
          <span class="grid gap-[3px] min-w-0">
            <span class="text-[12.5px] truncate {on ? 'text-fg' : 'text-fg-2'}">{commit.title}</span>
            <span class="flex items-center gap-1.5 text-[11px] min-w-0">
              <span class="mono text-[10.5px] leading-4 px-[5px] rounded shrink-0 text-[var(--cc)] bg-[color-mix(in_srgb,var(--cc)_15%,transparent)]">{commit.sha.slice(0, 7)}</span>
              {#if commit.unpushed}
                <span class="h-4 px-[5px] rounded-[5px] inline-flex items-center text-[10px] font-medium shrink-0 text-warning bg-[color-mix(in_srgb,var(--color-warning)_14%,transparent)]">unpushed</span>
              {/if}
              <span class="text-muted truncate min-w-0">{commit.author} · {timeAgo(commit.committed_at)}</span>
              {#if stat}
                <span class="ml-auto mono shrink-0">
                  <span class="text-add-fg">+{stat.additions}</span>
                  <span class="text-del-fg">−{stat.deletions}</span>
                </span>
              {/if}
            </span>
          </span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="flex justify-between gap-2 flex-wrap px-2 pt-2 mt-1 {commits.length > 0 ? 'border-t border-line' : ''} text-[11px] text-muted">
    <span title={scopeTitle}>{scopeLabel} · {reviewedCount}/{totalCount} reviewed</span>
    <span><span class="kbd">j</span>/<span class="kbd">k</span> files</span>
  </div>
</section>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    color: var(--color-fg-2);
  }
  .sw {
    position: relative;
    width: 26px;
    height: 16px;
    border-radius: 8px;
    background: var(--color-border);
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
  @media (prefers-reduced-motion: reduce) {
    .sw,
    .sw::after {
      transition: none;
    }
  }
</style>
