<script lang="ts">
  import type { PrSnapshot } from "$lib/types";

  interface Props {
    pr: PrSnapshot;
  }

  const { pr }: Props = $props();

  /** Map PR state to mock-spec colors. */
  const stateChip = $derived.by(() => {
    switch (pr.state.toLowerCase()) {
      case "draft":
        return { bg: "#9333ea22", border: "#9333ea44", text: "#c4b5fd", dot: "#a78bfa", label: "Draft" };
      case "open":
        return { bg: "#1f4d2a", border: "#7ee2a866", text: "#7ee2a8", dot: "#7ee2a8", label: "Open" };
      case "merged":
        return { bg: "#5b21b6", border: "#7c3aed66", text: "#c4b5fd", dot: "#a78bfa", label: "Merged" };
      case "closed":
        return { bg: "#3a1a1a", border: "#f4a3a366", text: "#f4a3a3", dot: "#f4a3a3", label: "Closed" };
      default:
        return { bg: "#1f1f1f", border: "#2a2a2a", text: "#999", dot: "#5e5e5e", label: pr.state };
    }
  });
</script>

<!-- PR header -->
<div class="px-4 pt-4 pb-3 border-b border-hairline">
  <div class="flex items-center gap-2 mb-1">
    <span
      class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium border"
      style="background: {stateChip.bg}; border-color: {stateChip.border}; color: {stateChip.text};"
    >
      <span class="w-1.5 h-1.5 rounded-full" style="background: {stateChip.dot};"></span>
      {stateChip.label}
    </span>
    <span class="mono text-[11px] text-muted">#{pr.number}</span>
  </div>
  <h2 class="text-base text-fg leading-snug mb-2">{pr.title}</h2>
  <div class="flex items-center gap-3 text-[11px] text-fg-3">
    <span class="mono">{pr.head}</span>
    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
    <span class="mono">{pr.base}</span>
  </div>
</div>

<!--
  Checks / Reviewers / Conversation sections are spec'd in build-plan §3 but
  the AppSnapshot.PrSnapshot doesn't carry those fields yet — they need an
  engine change (extend PrSnapshot to include checks, reviewers, conversation).
  Skeleton sections below render once the data lands.
-->
<div class="px-4 py-6 text-[11px] text-muted leading-relaxed text-center">
  Checks · Reviewers · Conversation will appear here when the engine starts populating the PR snapshot with that data.
</div>
