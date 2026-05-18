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
  /** Read-only tab: a local-branch view or a remote PR view. Hide write-mode tabs. */
  const isReadOnly = $derived(
    (snapshot?.local_branch ?? null) !== null || (snapshot?.pr ?? null) !== null,
  );
</script>

<div class="border-t border-hairline bg-bg shrink-0">
  <!-- Current scope -->
  <div class="px-3 pt-2 pb-1.5">
    <button
      class="w-full text-left px-3 py-1.5 rounded-md text-sm flex items-center gap-2 {mode === 'branch' ? 'bg-ink-650 text-fg' : 'text-fg-2 hover:bg-card'}"
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
    <div class="border-t border-hairline max-h-44 overflow-y-auto">
      <div class="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wider text-muted">
        Commits · {commitsToShow.length}
      </div>
      {#each commitsToShow as commit (commit.sha)}
        <button
          class="w-full text-left px-3 py-1.5 hover:bg-card"
          onclick={() => app.cmd("select_commit", { sha: commit.sha })}
        >
          <div class="text-[13px] text-fg-2 truncate">{commit.title}</div>
          <div class="text-[11px] text-muted mono">{commit.sha.slice(0, 7)} · {commit.author} · {commit.age}</div>
        </button>
      {/each}
    </div>
  {/if}

  <!-- Footer summary -->
  <div class="border-t border-hairline px-3 py-1.5 flex items-center justify-between text-[11px] text-muted mono">
    <span>{scopeLabel} · {reviewed_count} / {total_count} reviewed</span>
    <span>j/k · U next</span>
  </div>
</div>
