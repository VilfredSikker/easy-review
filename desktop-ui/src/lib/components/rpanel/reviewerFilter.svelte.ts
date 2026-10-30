import type { AiSnapshot } from "$lib/types";
import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";

/**
 * Keep the reviewer filter valid for the review on screen: a reviewer that no
 * longer has findings falls back to all reviewers. Both the panel and its
 * collapsed rail call this, so the filter is coerced whichever one is mounted.
 * Call during component init.
 */
export function useReviewerFilterSync(getAi: () => AiSnapshot | null): void {
  $effect(() => {
    const ai = getAi();
    if (!ai) return;
    aiReviewFilter.syncFromFindings(ai.findings, Object.keys(ai.agent_summaries ?? {}));
  });
}
