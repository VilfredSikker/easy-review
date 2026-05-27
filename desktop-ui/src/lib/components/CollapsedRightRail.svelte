<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    ai: AiSnapshot | null;
    onExpand: (tab: "branch" | "review" | "notes") => void;
  }

  const { ai, onExpand }: Props = $props();

  const totalFindings = $derived(ai?.findings.length ?? 0);
  const worstSeverity = $derived.by((): "high" | "med" | "low" | null => {
    if (!ai || ai.findings.length === 0) return null;
    if (ai.findings.some((f) => f.severity === "high")) return "high";
    if (ai.findings.some((f) => f.severity === "med")) return "med";
    return "low";
  });

  const questionCount = $derived(
    ai?.threads.filter((t) => t.kind === "question").length ?? 0
  );

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

  const ciIconColor = $derived(
    checksStatus === "success" ? "text-add-fg"
    : checksStatus === "failure" ? "text-del-fg"
    : "text-fg-3"
  );
  const ciBadgeClass = $derived(
    checksStatus === "success" ? "bg-add-fg/20 text-add-fg"
    : checksStatus === "failure" ? "bg-del-fg/20 text-del-fg"
    : ""
  );
  const ciBadge = $derived(checksStatus === "success" ? "✓" : checksStatus === "failure" ? "!" : null);
</script>

<aside class="w-11 shrink-0 bg-surface border-l border-hairline flex flex-col items-center py-2 gap-1 overflow-hidden">
  <!-- Branch atom -->
  <button
    type="button"
    title="Branch — expand rail"
    onclick={() => onExpand("branch")}
    class="relative w-8 h-8 rounded-lg flex items-center justify-center hover:bg-hover transition-colors"
  >
    <!-- git-branch icon -->
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent">
      <line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>
    </svg>
    {#if isDraft}
      <span class="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 flex items-center justify-center text-[8px] font-bold rounded-full bg-card text-muted border border-hairline" style="border-width: 2px;">D</span>
    {/if}
  </button>

  <!-- GitHub atom -->
  <button
    type="button"
    title="GitHub — expand rail"
    onclick={() => onExpand("branch")}
    class="relative w-8 h-8 rounded-lg flex items-center justify-center hover:bg-hover transition-colors"
  >
    <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" class={ciIconColor}>
      <path d="M12 0C5.4 0 0 5.4 0 12c0 5.3 3.4 9.8 8.2 11.4.6.1.8-.3.8-.6v-2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.6 3.3-1.2 3.3-1.2.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6C20.6 21.8 24 17.3 24 12c0-6.6-5.4-12-12-12z"/>
    </svg>
    {#if ciBadge}
      <span class="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 flex items-center justify-center text-[8px] font-bold rounded-full border-2 border-surface {ciBadgeClass}">{ciBadge}</span>
    {/if}
  </button>

  <!-- AI Review atom -->
  <button
    type="button"
    title="AI Review — expand rail"
    onclick={() => onExpand("review")}
    class="relative w-8 h-8 rounded-lg flex items-center justify-center hover:bg-hover transition-colors"
  >
    <!-- sparkle -->
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="text-finding">
      <path d="M12 2l2.4 7.2H22l-6.2 4.5 2.4 7.2L12 17l-6.2 3.9 2.4-7.2L2 9.2h7.6z"/>
    </svg>
    {#if totalFindings > 0}
      <span class="absolute -top-0.5 -right-0.5 min-w-3.5 h-3.5 px-0.5 flex items-center justify-center text-[8px] font-bold rounded-full border-2 border-surface {findingBadgeClass}">{totalFindings}</span>
    {/if}
  </button>

  <!-- Notes atom -->
  <button
    type="button"
    title="Notes — expand rail"
    onclick={() => onExpand("notes")}
    class="relative w-8 h-8 rounded-lg flex items-center justify-center hover:bg-hover transition-colors"
  >
    <!-- chat bubble -->
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-fg-3">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
    </svg>
    {#if questionCount > 0}
      <span class="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 flex items-center justify-center text-[8px] font-bold rounded-full bg-question/20 text-question border-2 border-surface">{questionCount}</span>
    {/if}
  </button>

  <div class="flex-1"></div>
</aside>
