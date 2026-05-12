<script lang="ts">
  import type { AiSnapshot, PrSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import BranchCard from "./BranchCard.svelte";
  import AiReviewCard from "./AiReviewCard.svelte";
  import CommentsCard from "./CommentsCard.svelte";
  import QuestionsCard from "./QuestionsCard.svelte";

  interface Props {
    ai: AiSnapshot | null;
    pr: PrSnapshot | null;
  }

  const { ai, pr }: Props = $props();

  const hasAiData = $derived(
    ai !== null && (ai.high + ai.med + ai.low > 0 || ai.findings.length > 0)
  );
</script>

<aside class="w-80 border-l border-ink-500/40 bg-ink-850 shrink-0 flex flex-col overflow-hidden">
  <div class="flex-1 overflow-y-auto flex flex-col gap-0">
    {#if app.snapshot}
      <BranchCard
        branch={app.snapshot.branch}
        base={app.snapshot.base}
        {pr}
        reviewed_count={app.snapshot.reviewed_count}
        total_count={app.snapshot.total_count}
      />
    {/if}

    {#if ai && hasAiData}
      <AiReviewCard {ai} />
    {/if}

    {#if ai && ai.comments > 0}
      <CommentsCard {ai} />
    {/if}

    {#if ai && ai.questions > 0}
      <QuestionsCard {ai} />
    {/if}
  </div>
</aside>
