<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { CommitSummary } from "$lib/types";

  interface Props {
    mode: "branch" | "unstaged" | "staged" | "history";
    total_count: number;
    reviewed_count: number;
    commits?: CommitSummary[];
  }

  const { mode, total_count, reviewed_count, commits = [] }: Props = $props();

  const scopeLabel = $derived(
    mode === "branch" ? "Branch" :
    mode === "unstaged" ? "Unstaged" :
    mode === "staged" ? "Staged" :
    "History"
  );

  const snapshot = $derived(app.snapshot);
  const commitsToShow = $derived(commits.length > 0 ? commits : (snapshot?.commits ?? []));
  const selectedCommitSha = $derived(snapshot?.selected_commit_sha ?? null);
  const activeTab = $derived(snapshot?.tabs?.find((t) => t.is_active) ?? null);
  /** "All changes" is active only in branch mode with no commit selected. */
  const allChangesActive = $derived(mode === "branch" && selectedCommitSha == null);
  /** Remote PR and read-only local-branch views hide unstaged/staged/commits. */
  const isReadOnly = $derived(
    activeTab?.kind === "remote_pr"
      || (activeTab?.kind === "local_branch" && !snapshot?.local_branch_checked_out),
  );

  let commitsCollapsed = $state(true);
</script>

<div class="border-t border-hairline bg-bg shrink-0">
  <!-- Current scope -->
  <div class="px-3 pt-2 pb-1.5">
    <button
      class="w-full text-left px-3 py-1.5 rounded-md text-sm flex items-center gap-2 {allChangesActive ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
      onclick={() => app.cmd("set_mode", { mode: "branch" })}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12l9-9 9 9M3 16l9-9 9 9M3 20l9-9 9 9"/></svg>
      <span>All changes</span>
      <span class="ml-auto mono text-[11px] text-muted">{total_count} files</span>
    </button>
  </div>

  <!-- Quick scopes -->
  {#if !isReadOnly}
  <div class="px-3 pb-1.5 grid grid-cols-2 gap-1">
    <button
      class="px-2 py-1 rounded text-xs text-left flex items-center gap-1.5 {mode === 'unstaged' ? 'bg-ink-650 text-fg-2' : 'text-fg-2 hover:bg-card'}"
      onclick={() => app.cmd("set_mode", { mode: "unstaged" })}
    >
      <span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>
      <span>Unstaged</span>
    </button>
    <button
      class="px-2 py-1 rounded text-xs text-left flex items-center gap-1.5 {mode === 'staged' ? 'bg-ink-650 text-fg-2' : 'text-fg-2 hover:bg-card'}"
      onclick={() => app.cmd("set_mode", { mode: "staged" })}
    >
      <span class="w-1.5 h-1.5 rounded-full bg-add-fg"></span>
      <span>Staged</span>
    </button>
  </div>
  {/if}

  <!-- Commits in scope (only when engine provides them; hidden on read-only tabs) -->
  {#if !isReadOnly && commitsToShow.length > 0}
    <div class="border-t border-hairline {commitsCollapsed ? '' : 'max-h-72 overflow-y-auto'}">
      <button
        class="w-full flex items-center gap-1.5 px-3 pt-2 pb-1 text-[10px] uppercase tracking-wider text-muted hover:text-fg-2 sticky top-0 bg-bg"
        onclick={() => (commitsCollapsed = !commitsCollapsed)}
        aria-expanded={!commitsCollapsed}
        title={commitsCollapsed ? "Expand commits" : "Collapse commits"}
      >
        <svg
          width="10"
          height="10"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class="transition-transform {commitsCollapsed ? '-rotate-90' : ''}"
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
        <span>Commits · {commitsToShow.length}</span>
      </button>
      {#if !commitsCollapsed}
      {#each commitsToShow as commit (commit.sha)}
        {@const isSelected = commit.sha === selectedCommitSha}
        <button
          class="w-full text-left px-3 py-1.5 {isSelected ? 'bg-ink-650 text-fg' : 'hover:bg-card text-fg-2'}"
          onclick={() => app.cmd("select_commit", { sha: commit.sha })}
          title={commit.title}
        >
          <div class="text-[13px] truncate">{commit.title}</div>
          <div class="text-[11px] text-muted mono truncate">{commit.sha.slice(0, 7)} · {commit.author} · {commit.age}</div>
        </button>
      {/each}
      {/if}
    </div>
  {/if}

  <!-- Footer summary -->
  <div class="border-t border-hairline px-3 py-1.5 flex items-center justify-between text-[11px] text-muted mono">
    <span>{scopeLabel} · {reviewed_count} / {total_count} reviewed</span>
    <span>j/k · U next</span>
  </div>
</div>
