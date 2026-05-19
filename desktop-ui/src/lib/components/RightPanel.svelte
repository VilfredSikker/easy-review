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
    width?: number;
    dragging?: boolean;
    onResizeStart?: (e: MouseEvent) => void;
  }

  const { ai, pr, width = 340, dragging = false, onResizeStart }: Props = $props();

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
<aside
  class="shrink-0 bg-surface border-l border-hairline overflow-hidden flex flex-col relative"
  style="width: {width}px"
>
  <!--
    4px drag handle along the panel's left edge. Mirrors the terminal drawer
    pattern: capture mousedown to start a horizontal resize; while dragging the
    parent sets a body class so the cursor stays consistent if the pointer
    briefly leaves the handle.
  -->
  {#if onResizeStart}
    <div
      class="absolute -left-[2px] top-0 bottom-0 w-1 cursor-ew-resize z-10 hover:bg-accent/40 {dragging ? 'bg-accent/60' : ''}"
      onmousedown={onResizeStart}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize right panel"
    ></div>
  {/if}
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
