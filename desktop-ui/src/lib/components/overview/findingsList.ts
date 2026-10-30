import type { AiSnapshot, Confidence, FlatFinding, ThreadSnapshot } from "$lib/types";
import { findingAgentLabels } from "$lib/aiReviewAgents";
import { mergeFindingReplies } from "$lib/findingReplies";
import { findingsPanelView, type FindingsPanelView } from "$lib/components/rpanel/findingsPanel";

export interface OverviewFindingsView extends FindingsPanelView {
  /** Lenses that raised an open finding, in first-seen order. */
  lenses: string[];
  /** Distinct providers behind the open findings (ADR 0044/0045: raisers, never the arbiter). */
  reviewerCount: number;
}

/**
 * The Overview list: every open finding the Gate lets through, then the
 * resolved ones dimmed beneath (the prototype keeps them visible so the list
 * reads as a record of the review, not a to-do that shrinks). No severity tile
 * filter here: the Overview is the summary, the right panel is the filter.
 */
export function overviewFindingsView(
  ai: Pick<AiSnapshot, "findings" | "resolved_findings"> | null,
  minTrust: Confidence,
): OverviewFindingsView {
  const open = ai?.findings ?? [];
  const view = findingsPanelView(open, ai?.resolved_findings ?? [], {
    severity: "all",
    minTrust,
    showResolved: true,
  });
  const lenses = [...new Set(open.flatMap(findingAgentLabels))];
  const providers = new Set(open.flatMap((f) => f.providers.map((p) => p.trim()).filter(Boolean)));
  return { ...view, lenses, reviewerCount: providers.size };
}

/** "General · Data · 2 reviewers"; the reviewer count only once there is more than one. */
export function lensCaption(lenses: readonly string[], reviewerCount: number): string {
  const parts = [...lenses];
  if (reviewerCount > 1) parts.push(`${reviewerCount} reviewers`);
  return parts.join(" · ");
}

/** The word after the title: the grade, or "resolved" once the user fixed it. */
export function confidenceWord(finding: Pick<FlatFinding, "resolved" | "confidence">): string {
  return finding.resolved ? "resolved" : finding.confidence;
}

/**
 * Promote posts a GitHub comment, and GitHub comments belong to the PR
 * (ADR 0004), so the Overview offers it only on the PR diff view. A finding
 * already promoted, or resolved, has nothing left to promote.
 */
export function canPromote(
  finding: Pick<FlatFinding, "resolved" | "promoted_to">,
  onPrView: boolean,
): boolean {
  return onPrView && !finding.resolved && finding.promoted_to == null;
}

/**
 * The comment a promotion starts from: the finding, then its replies quoted,
 * the same body the inline card's Promote opens with so both entry points
 * post the same thing.
 */
export function promoteBody(finding: FlatFinding, thread: ThreadSnapshot | null): string {
  const parts = [finding.message_markdown ? `${finding.title}\n\n${finding.message_markdown}` : finding.title];
  for (const r of mergeFindingReplies(finding, thread)) {
    if (r.body_markdown === "…thinking") continue;
    const quoted = r.body_markdown
      .split("\n")
      .map((l) => `> ${l}`)
      .join("\n");
    parts.push(`> **${r.author}** replied:\n${quoted}`);
  }
  return parts.join("\n\n");
}
