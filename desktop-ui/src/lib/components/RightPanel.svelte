<script lang="ts">
  import type { AiSnapshot, PrSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import BranchCard from "./BranchCard.svelte";
  import AiReviewCard from "./AiReviewCard.svelte";
  import CommentsCard from "./CommentsCard.svelte";
  import QuestionsCard from "./QuestionsCard.svelte";
  import UiAnnotationsCard from "./UiAnnotationsCard.svelte";
  import AgentOutputCard from "./AgentOutputCard.svelte";
  import DiffSourceCard from "./DiffSourceCard.svelte";

  interface Props {
    ai: AiSnapshot | null;
    pr: PrSnapshot | null;
  }

  const { ai, pr }: Props = $props();

  const totalAdds = $derived(
    app.snapshot?.files.reduce((sum, f) => sum + f.additions, 0) ?? 0
  );
  const totalDels = $derived(
    app.snapshot?.files.reduce((sum, f) => sum + f.deletions, 0) ?? 0
  );

  const currentWorktree = $derived(
    app.snapshot?.worktrees.find((w) => w.is_current) ?? null
  );

  const checksStatus = $derived.by((): "success" | "pending" | "failure" | null => {
    const checks = app.snapshot?.github?.checks;
    if (!checks || checks.length === 0) return null;
    if (checks.some((c) => c.conclusion === "FAILURE" || c.conclusion === "fail")) return "failure";
    if (checks.some((c) => c.status === "PENDING")) return "pending";
    return "success";
  });
</script>

<!--
  Right panel matches mocks/01-main: no tab strip — just the card stack.
  (The 04-github mock uses a separate full-page layout; the tab-strip idea
  from build-plan §700 doesn't show up in 01-main even when a PR exists.)
-->
<aside class="w-[340px] shrink-0 bg-surface border-l border-hairline overflow-hidden flex flex-col">
  <div class="flex-1 overflow-y-auto p-4 space-y-4 pb-8">
    {#if app.snapshot}
      <BranchCard
        branch={app.snapshot.branch}
        base={app.snapshot.base}
        {pr}
        reviewed_count={app.snapshot.reviewed_count}
        total_count={app.snapshot.total_count}
        additions={totalAdds}
        deletions={totalDels}
        checks_status={checksStatus}
        is_pr={currentWorktree?.is_pr ?? false}
        pr_number={currentWorktree?.pr_number ?? null}
        is_merged={currentWorktree?.is_merged ?? false}
        github_url={app.snapshot?.github?.url ?? null}
        github={app.snapshot?.github ?? null}
      />
    {/if}

    {#if app.snapshot?.diff_source}
      <DiffSourceCard source={app.snapshot.diff_source} />
    {/if}

    {#if ai}
      <AiReviewCard {ai} />
    {/if}

    {#if ai && ai.comments > 0}
      <CommentsCard {ai} />
    {/if}

    {#if ai && ai.questions > 0}
      <QuestionsCard {ai} />
    {/if}

    <AgentOutputCard />

    <UiAnnotationsCard />
  </div>
</aside>
