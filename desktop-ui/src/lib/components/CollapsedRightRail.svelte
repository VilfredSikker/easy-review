<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { panelThreads } from "./rpanel/commentsPanel";
  import { scopeToReviewer } from "./rpanel/findingsPanel";
  import { useReviewerFilterSync } from "./rpanel/reviewerFilter.svelte";
  import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
  import type { RightPanelTab } from "$lib/stores/rightPanelTab.svelte";

  interface Props {
    ai: AiSnapshot | null;
    onExpand: (tab: RightPanelTab) => void;
  }

  const { ai, onExpand }: Props = $props();

  // The same counts the expanded panel's tabs show, so expanding never
  // changes a number under the reader's eye.
  // Scoped to the reviewer filter, as the Findings tab and the diff are.
  useReviewerFilterSync(() => ai);
  const scopedFindings = $derived(scopeToReviewer(ai, aiReviewFilter.filter).open);
  const totalFindings = $derived(scopedFindings.length);
  const worstSeverity = $derived.by((): "high" | "med" | "low" | null => {
    const findings = scopedFindings;
    if (findings.some((f) => f.severity === "high")) return "high";
    if (findings.some((f) => f.severity === "med")) return "med";
    return findings.length > 0 ? "low" : null;
  });
  const threadCount = $derived(
    panelThreads(ai?.threads, app.commentVisibility, app.snapshot?.files).length,
  );
  const fileCount = $derived(app.snapshot?.files.length ?? 0);

  const github = $derived(app.snapshot?.github ?? null);
  const checksStatus = $derived.by((): "success" | "failure" | "pending" | null => {
    const checks = github?.checks;
    if (!checks || checks.length === 0) return null;
    if (checks.some((c) => c.conclusion === "FAILURE" || c.conclusion === "fail")) return "failure";
    if (checks.some((c) => c.status === "PENDING")) return "pending";
    return "success";
  });
  const isDraft = $derived(github?.is_draft ?? false);

  const findingBadgeClass = $derived.by(() => {
    if (worstSeverity === "high") return "bg-risk-high/20 text-risk-high";
    if (worstSeverity === "med") return "bg-risk-med/20 text-risk-med";
    if (worstSeverity === "low") return "bg-risk-low/20 text-risk-low";
    return "";
  });

  const ci = $derived.by(() => {
    if (checksStatus === "success") return { icon: "text-add-fg", badgeClass: "bg-add-fg/20 text-add-fg", badge: "✓" };
    if (checksStatus === "failure") return { icon: "text-del-fg", badgeClass: "bg-del-fg/20 text-del-fg", badge: "!" };
    return { icon: "text-accent", badgeClass: "", badge: null };
  });

  const atom = "relative w-8 h-8 rounded-lg flex items-center justify-center hover:bg-hover transition-colors";
  const badge =
    "absolute -top-0.5 -right-0.5 min-w-3.5 h-3.5 px-0.5 flex items-center justify-center text-[8px] font-bold rounded-full border-2 border-surface";
</script>

<aside
  class="w-11 shrink-0 bg-surface border-l border-hairline flex flex-col items-center py-2 gap-1 overflow-hidden"
  aria-label="Right panel, collapsed"
>
  <button type="button" title="Findings — expand panel" aria-label="Findings" onclick={() => onExpand("findings")} class={atom}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="text-finding">
      <path d="M12 2l2.4 7.2H22l-6.2 4.5 2.4 7.2L12 17l-6.2 3.9 2.4-7.2L2 9.2h7.6z"/>
    </svg>
    {#if totalFindings > 0}
      <span class="{badge} {findingBadgeClass}">{totalFindings}</span>
    {/if}
  </button>

  <button type="button" title="Risks — expand panel" aria-label="Risks" onclick={() => onExpand("risks")} class={atom}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-risk-med">
      <path d="M12 3l9 16H3z"/><path d="M12 10v4"/><path d="M12 17h.01"/>
    </svg>
    {#if fileCount > 0}
      <span class="{badge} bg-hairline text-fg-3">{fileCount}</span>
    {/if}
  </button>

  <button type="button" title="Comments — expand panel" aria-label="Comments" onclick={() => onExpand("comments")} class={atom}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-comment">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
    </svg>
    {#if threadCount > 0}
      <span class="{badge} bg-comment/20 text-comment">{threadCount}</span>
    {/if}
  </button>

  <div class="w-5 h-px bg-hairline my-1"></div>

  <!-- Branch & PR: CI state wins the badge; a draft shows when CI has none. -->
  <button type="button" title="Branch & PR — on the Overview" aria-label="Branch and PR" onclick={() => onExpand("branch")} class={atom}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class={ci.icon}>
      <line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>
    </svg>
    {#if ci.badge}
      <span class="{badge} {ci.badgeClass}">{ci.badge}</span>
    {:else if isDraft}
      <span class="{badge} bg-card text-muted">D</span>
    {/if}
  </button>

  <button type="button" title="Diagrams — on the Overview" aria-label="Diagrams" onclick={() => onExpand("context")} class={atom}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-fg-3">
      <rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><path d="M10 6.5h5.5a2 2 0 0 1 2 2V14"/>
    </svg>
    {#if (ai?.diagrams?.length ?? 0) > 0}
      <span class="{badge} bg-accent/20 text-accent">{ai?.diagrams?.length}</span>
    {/if}
  </button>

  <div class="flex-1"></div>
</aside>
