<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { railView } from "$lib/stores/railView.svelte";
  import { footerScopeLabel } from "$lib/scopeRail";
  import type { CommitSummary } from "$lib/types";
  import ScopeSegment from "./ScopeSegment.svelte";
  import CommitsSection from "./CommitsSection.svelte";

  interface Props {
    mode: "branch" | "unstaged" | "staged" | "history" | "pr" | "conflicts" | "hidden" | "tour";
    total_count: number;
    reviewed_count: number;
    commits?: CommitSummary[];
    /** False once the rail mounts ScopeSegment at its top, as the design
     *  places it; until then it sits above the commits. */
    showScopeSegment?: boolean;
  }

  const { mode, total_count, reviewed_count, commits = [], showScopeSegment = true }: Props =
    $props();

  const view = railView();
  const snapshot = $derived(app.snapshot);
  const commitsToShow = $derived(commits.length > 0 ? commits : (snapshot?.commits ?? []));
  const selectedSha = $derived(snapshot?.selected_commit_sha ?? null);

  const allActive = $derived((mode === "branch" || mode === "pr") && selectedSha == null);
  // Only while "All changes" is on screen are snapshot.files that view's files;
  // in a commit or a working-tree scope the sum would describe something else.
  const allStat = $derived.by(() => {
    if (!allActive) return null;
    let additions = 0;
    let deletions = 0;
    for (const f of snapshot?.files ?? []) {
      additions += f.additions;
      deletions += f.deletions;
    }
    return { additions, deletions };
  });

  const scopeLabel = $derived(footerScopeLabel(mode, view.isPr, selectedSha));
  const scopeTitle = $derived(
    mode === "history"
      ? "Reviewed marks in a single commit are kept for that view, apart from the branch and PR diff"
      : undefined,
  );

  function returnToAllChanges() {
    // exit_history knows the view History was entered from, so it is right
    // even when the remembered source here is not.
    if (mode === "history") void app.cmd("exit_history");
    else if (view.isPr) void app.cmd("set_mode", { mode: "pr_diff" });
    else void app.cmd("set_mode", { mode: "branch" });
  }
</script>

<div class="shrink-0 flex flex-col">
  {#if showScopeSegment && view.scopes.length > 1 && !view.isPr}
    <div class="border-t border-line bg-surface px-2.5 pt-2">
      <ScopeSegment />
    </div>
  {/if}
  <CommitsSection
    commits={commitsToShow}
    {selectedSha}
    selectable={!view.remote}
    allLabel={view.isPr ? "All changes" : "Branch"}
    {allActive}
    {allStat}
    {scopeLabel}
    {scopeTitle}
    reviewedCount={reviewed_count}
    totalCount={total_count}
    onSelectCommit={(sha) => void app.cmd("select_commit", { sha })}
    onAllChanges={returnToAllChanges}
  />
</div>
