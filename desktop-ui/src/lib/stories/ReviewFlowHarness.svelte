<script lang="ts">
  import type { ReviewFlowSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import AiReviewCard from "$lib/components/AiReviewCard.svelte";
  import TriageCard from "$lib/components/TriageCard.svelte";
  import { aiWithFindings, richSnapshot, triageBroadGuarded } from "./fixtures";

  let { flow, showTriage = true }: { flow: ReviewFlowSnapshot | null; showTriage?: boolean } = $props();
  $effect(() => {
    app.snapshot = {
      ...richSnapshot,
      review_flow: flow,
      ai: { ...aiWithFindings, triage: showTriage ? triageBroadGuarded : null },
    };
  });
</script>

<div class="flex max-w-md flex-col gap-3">
  {#if showTriage}<TriageCard triage={triageBroadGuarded} />{/if}
  <AiReviewCard ai={{ ...aiWithFindings, triage: showTriage ? triageBroadGuarded : null }} />
</div>
