<script lang="ts">
  import { untrack } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { pendingReview } from "$lib/stores/pendingReview.svelte";
  import { pendingReviewKey, type PendingReview } from "$lib/reviewSubmit";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import ViewBar from "$lib/components/shell/ViewBar.svelte";
  import StaleBanner from "$lib/components/shell/StaleBanner.svelte";
  import LocalBranchBanner from "$lib/components/shell/LocalBranchBanner.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
    /** Seeds the pending review GitHub would return, since stories have no backend. */
    pending?: PendingReview;
    reviewOpen?: boolean;
  }
  const { snapshot, pending, reviewOpen = false }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
    if (!pending) return;
    untrack(() => {
      // Claiming the key first makes the bar's `follow` a no-op, so it does
      // not try to read the review from a backend that is not there.
      pendingReview.key = pendingReviewKey(resolveActivePrNumber(snapshot), snapshot.github);
      pendingReview.review = pending;
      pendingReview.decision = "approve";
      pendingReview.open = reviewOpen;
    });
  });
</script>

<div class="min-h-[520px] bg-bg text-fg">
  <ViewBar />
  <StaleBanner />
  <LocalBranchBanner />
</div>
