<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { CommitSummary } from "$lib/types";
  import { timeAgo } from "$lib/time";

  interface Props {
    mode: "branch" | "unstaged" | "staged" | "history" | "pr" | "conflicts" | "hidden" | "tour";
    total_count: number;
    reviewed_count: number;
    commits?: CommitSummary[];
  }

  const { mode, total_count, reviewed_count, commits = [] }: Props = $props();

  const scopeLabel = $derived(
    mode === "branch" ? "Branch" :
    mode === "unstaged" ? "Unstaged" :
    mode === "staged" ? "Staged" :
    mode === "pr" ? "PR Diff" :
    mode === "tour" ? "Guide" :
    "History"
  );

  const snapshot = $derived(app.snapshot);
  const features = $derived(
    snapshot?.features ?? {
      viewBranch: true,
      viewUnstaged: true,
      viewStaged: true,
      viewHistory: true,
      viewConflicts: true,
      viewHidden: true,
      viewTour: true,
    },
  );
  const commitsToShow = $derived(commits.length > 0 ? commits : (snapshot?.commits ?? []));
  const selectedCommitSha = $derived(snapshot?.selected_commit_sha ?? null);
  const activeTab = $derived(snapshot?.tabs?.find((t) => t.is_active) ?? null);
  /** "All changes" is active only in branch mode with no commit selected. */
  const allChangesActive = $derived(mode === "branch" && selectedCommitSha == null);
  /** Remote PR tabs only show PR Diff. Local read-only (non-checked-out branch) shows Branch + PR Diff only. */
  const isRemotePr = $derived(activeTab?.kind === "remote_pr");
  const isReadOnly = $derived(
    isRemotePr
      || (activeTab?.kind === "local_branch" && !snapshot?.local_branch_checked_out),
  );
  /** Remote PR tabs have no local object DB, so per-commit diffs can't be
   *  scoped — commits are display-only there. Local tabs (checked out or not)
   *  resolve commit SHAs from the shared object database. */
  const commitsSelectable = $derived(activeTab?.kind !== "remote_pr");

  /**
   * Derive visible modes from snapshot state, mirroring the Rust `visible_modes` logic
   * in `crates/er-engine/src/app/state/mod.rs`.
   *
   * - remote_pr tab → PR Diff only (no local working tree)
   * - local PR tab, head branch not checked out → PR Diff only (handled in
   *   BranchContextBar's toggle; ScopeSelector's working-tree rows are read-only-gated)
   * - local checked-out → Branch, Unstaged, Staged, PR Diff (if pr_number), History
   */
  const availableModes = $derived.by((): string[] => {
    if (isRemotePr) return ["pr"];
    // Local scopes only. PR Diff moved to the header toggle (BranchContextBar);
    // History is reachable by clicking a commit in the COMMITS list below — no
    // dedicated scope row.
    const modes: string[] = [];
    if (features.viewBranch) modes.push("branch");
    if (features.viewUnstaged && !isReadOnly) modes.push("unstaged");
    if (features.viewStaged && !isReadOnly) modes.push("staged");
    return modes;
  });

  /** Aggregate +/- across all files for the "All changes" row. */
  const totalAdditions = $derived(
    (snapshot?.files ?? []).reduce((s, f) => s + f.additions, 0),
  );
  const totalDeletions = $derived(
    (snapshot?.files ?? []).reduce((s, f) => s + f.deletions, 0),
  );

  /** Per-scope +/- counts so Unstaged/Staged rows show changes without switching. */
  const unstagedStat = $derived(snapshot?.unstaged_stat ?? { additions: 0, deletions: 0 });
  const stagedStat = $derived(snapshot?.staged_stat ?? { additions: 0, deletions: 0 });

  /** Commits are expanded by default so they're discoverable. */
  let commitsCollapsed = $state(false);
</script>

<div class="border-t border-hairline bg-bg shrink-0">
  <!-- View selector — driven by availableModes derived from snapshot state -->
  <div class="px-3 pt-2 pb-1.5 flex flex-col gap-0.5">
    {#if availableModes.includes("branch")}
    <button
      class="w-full text-left px-2 py-[5px] rounded-md flex items-center gap-2 relative {allChangesActive ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
      onclick={() => void app.cmd("set_mode", { mode: "branch" })}
    >
      {#if allChangesActive}
        <span class="absolute left-0 top-[4px] bottom-[4px] w-[2px] rounded-r bg-accent"></span>
      {/if}
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 {allChangesActive ? 'text-accent' : 'text-fg-3'}"><path d="M3 12l9-9 9 9M3 16l9-9 9 9M3 20l9-9 9 9"/></svg>
      <span class="text-[12px]">Branch</span>
      <span class="ml-auto flex items-center gap-1">
        {#if totalAdditions > 0}
          <span class="mono text-[10px] text-add-fg">+{totalAdditions}</span>
        {/if}
        {#if totalDeletions > 0}
          <span class="mono text-[10px] text-del-fg">−{totalDeletions}</span>
        {/if}
      </span>
    </button>
    {/if}

    {#if availableModes.includes("unstaged")}
    <button
      class="w-full text-left px-2 py-[5px] rounded-md flex items-center gap-2 relative {mode === 'unstaged' ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
      onclick={() => void app.cmd("set_mode", { mode: "unstaged" })}
    >
      {#if mode === 'unstaged'}
        <span class="absolute left-0 top-[4px] bottom-[4px] w-[2px] rounded-r bg-accent"></span>
      {/if}
      <span class="w-1.5 h-1.5 rounded-full bg-risk-med shrink-0"></span>
      <span class="text-[12px]">Unstaged</span>
      <span class="ml-auto flex items-center gap-1">
        {#if unstagedStat.additions > 0}<span class="mono text-[10px] text-add-fg">+{unstagedStat.additions}</span>{/if}
        {#if unstagedStat.deletions > 0}<span class="mono text-[10px] text-del-fg">−{unstagedStat.deletions}</span>{/if}
      </span>
    </button>
    {/if}

    {#if availableModes.includes("staged")}
    <button
      class="w-full text-left px-2 py-[5px] rounded-md flex items-center gap-2 relative {mode === 'staged' ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
      onclick={() => void app.cmd("set_mode", { mode: "staged" })}
    >
      {#if mode === 'staged'}
        <span class="absolute left-0 top-[4px] bottom-[4px] w-[2px] rounded-r bg-accent"></span>
      {/if}
      <span class="w-1.5 h-1.5 rounded-full bg-add-fg shrink-0"></span>
      <span class="text-[12px]">Staged</span>
      <span class="ml-auto flex items-center gap-1">
        {#if stagedStat.additions > 0}<span class="mono text-[10px] text-add-fg">+{stagedStat.additions}</span>{/if}
        {#if stagedStat.deletions > 0}<span class="mono text-[10px] text-del-fg">−{stagedStat.deletions}</span>{/if}
      </span>
    </button>
    {/if}

    {#if availableModes.includes("pr")}
    <button
      class="w-full text-left px-2 py-[5px] rounded-md flex items-center gap-2 relative {mode === 'pr' ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
      onclick={() => void app.cmd("set_mode", { mode: "pr_diff" })}
    >
      {#if mode === 'pr'}
        <span class="absolute left-0 top-[4px] bottom-[4px] w-[2px] rounded-r bg-accent"></span>
      {/if}
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 {mode === 'pr' ? 'text-accent' : 'text-fg-3'}"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
      <span class="text-[12px]">PR Diff</span>
    </button>
    {/if}

  </div>

  <!-- Commits in scope (only when engine provides them; hidden on remote PR tabs) -->
  {#if commitsToShow.length > 0}
    <div class="border-t border-hairline overflow-hidden" style="max-height: {commitsCollapsed ? '30px' : '280px'}; transition: max-height 0.15s ease; flex-shrink: 0;">
      <button
        class="w-full flex items-center gap-1.5 px-3 py-[6px] text-[10px] uppercase tracking-[0.06em] font-semibold text-muted hover:text-fg-2 sticky top-0 bg-bg"
        onclick={() => (commitsCollapsed = !commitsCollapsed)}
        aria-expanded={!commitsCollapsed}
        title={commitsCollapsed ? "Expand commits" : "Collapse commits"}
      >
        <!-- git-commit glyph -->
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0"><circle cx="12" cy="12" r="3"/><line x1="3" y1="12" x2="9" y2="12"/><line x1="15" y1="12" x2="21" y2="12"/></svg>
        <span>Commits</span>
        <span class="px-[5px] py-0 rounded-full text-[9px] text-muted" style="background: color-mix(in srgb, var(--color-fg) 6%, transparent);">{commitsToShow.length}</span>
        <div class="flex-1"></div>
        <svg
          width="10"
          height="10"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class="transition-transform {commitsCollapsed ? 'rotate-180' : ''}"
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
      {#if !commitsCollapsed}
      <div class="overflow-y-auto pb-1" style="max-height: 250px;">
        {#each commitsToShow as commit (commit.sha)}
          {@const isSelected = commit.sha === selectedCommitSha}
          <button
            class="w-full text-left mx-[6px] rounded-[5px] relative {isSelected ? 'bg-card text-fg' : 'text-fg-2'} {commitsSelectable ? 'hover:bg-card/50 cursor-pointer' : 'cursor-default'}"
            style="width: calc(100% - 12px); padding: 5px 8px 5px 10px;"
            onclick={() => commitsSelectable && app.cmd("select_commit", { sha: commit.sha })}
            title={commitsSelectable ? commit.title : `${commit.title} (view-only for remote PRs)`}
          >
            {#if isSelected}
              <span class="absolute left-0 top-[5px] bottom-[5px] w-[2px] rounded-r bg-accent"></span>
            {/if}
            <div class="flex items-center gap-2">
              <!-- Author initials avatar -->
              <div class="w-[18px] h-[18px] rounded-full flex items-center justify-center text-[9px] font-bold shrink-0 uppercase text-on-accent" style="background: var(--color-accent);">
                {commit.author.slice(0, 2)}
              </div>
              <!-- Message + meta row -->
              <div class="min-w-0 flex-1 flex flex-col gap-[2px]">
                <div class="text-[12px] truncate leading-tight">{commit.title}</div>
                <div class="flex items-center gap-1.5">
                  <!-- Short SHA chip -->
                  <span
                    class="mono text-[10px] px-[5px] rounded leading-tight"
                    style="{isSelected
                      ? 'color: var(--color-accent); background: var(--color-accent-soft); border: 1px solid var(--color-accent-border);'
                      : 'color: var(--color-muted); background: var(--color-card); border: 1px solid var(--color-hairline);'}"
                  >{commit.sha.slice(0, 7)}</span>
                  <span class="text-[10px] text-muted">{timeAgo(commit.committed_at)}</span>
                  <!-- +/- and local badge: CommitSummary lacks additions/deletions/is_pushed fields — omitted until backend exposes them -->
                </div>
              </div>
            </div>
          </button>
        {/each}
      </div>
      {/if}
    </div>
  {/if}

  <!-- Footer summary -->
  <div class="border-t border-hairline px-3 py-1.5 flex items-center justify-between text-[11px] text-muted mono">
    <span>{scopeLabel} · {reviewed_count} / {total_count} reviewed</span>
    <span>j/k · U next</span>
  </div>
</div>
