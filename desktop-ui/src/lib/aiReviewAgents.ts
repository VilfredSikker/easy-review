import type { AiSnapshot, FlatFinding } from "$lib/types";

/** Matches `list_ai_reviewers` / EXPERTS registry order in er-engine. */
export const AGENT_LABEL_ORDER: readonly string[] = [
  "Triage",
  "General",
  "Security",
  "Performance",
  "Reliability",
  "Testing",
  "API / contracts",
  "Patterns",
  "Simplifying",
  "Mentorship",
  "Professor",
];

export const ALL_REVIEWERS = "all" as const;
export type AgentFilter = typeof ALL_REVIEWERS | string;

export function uniqueAgentLabels(
  findings: FlatFinding[],
  agentSummaryLabels: string[] = [],
): string[] {
  const seen = new Set<string>();
  for (const f of findings) {
    seen.add(f.agent_label ?? f.expert_label ?? "General");
  }
  for (const label of agentSummaryLabels) {
    seen.add(label);
  }
  return sortAgentLabels([...seen]);
}

export function sortAgentLabels(labels: string[]): string[] {
  const order = new Map(AGENT_LABEL_ORDER.map((l, i) => [l, i]));
  return [...labels].sort((a, b) => {
    const ia = order.get(a);
    const ib = order.get(b);
    if (ia !== undefined && ib !== undefined) return ia - ib;
    if (ia !== undefined) return -1;
    if (ib !== undefined) return 1;
    return a.localeCompare(b);
  });
}

export function countBySeverity(findings: FlatFinding[]): {
  high: number;
  med: number;
  low: number;
} {
  let high = 0;
  let med = 0;
  let low = 0;
  for (const f of findings) {
    if (f.severity === "high") high++;
    else if (f.severity === "med") med++;
    else low++;
  }
  return { high, med, low };
}

/**
 * Finding counts for Review badges / AI Review severity tiles.
 * Unfiltered: backend `high`/`med`/`low`. Agent filter: scoped findings.
 */
export function reviewFindingCounts(
  ai: Pick<AiSnapshot, "high" | "med" | "low" | "findings">,
  agentFilter: AgentFilter,
): { high: number; med: number; low: number } {
  if (agentFilter === ALL_REVIEWERS) {
    return { high: ai.high, med: ai.med, low: ai.low };
  }
  return countBySeverity(filterByAgent(ai.findings, agentFilter));
}

export function totalReviewFindings(
  ai: Pick<AiSnapshot, "high" | "med" | "low" | "findings">,
  agentFilter: AgentFilter = ALL_REVIEWERS,
): number {
  const c = reviewFindingCounts(ai, agentFilter);
  return c.high + c.med + c.low;
}

export function filterByAgent(
  findings: FlatFinding[],
  agentFilter: AgentFilter,
): FlatFinding[] {
  if (agentFilter === ALL_REVIEWERS) return findings;
  return findings.filter(
    (f) => (f.agent_label ?? f.expert_label ?? "General") === agentFilter,
  );
}

/** Default agent filter when findings load or agent set changes. */
export function defaultAgentFilter(agentLabels: string[]): AgentFilter {
  if (agentLabels.length <= 1) return agentLabels[0] ?? ALL_REVIEWERS;
  return ALL_REVIEWERS;
}

/** Keep current filter when still valid; otherwise apply default. */
export function coerceAgentFilter(
  labels: string[],
  current: AgentFilter,
): AgentFilter {
  if (labels.length === 0) return ALL_REVIEWERS;
  if (labels.length === 1) return labels[0]!;
  if (current !== ALL_REVIEWERS && labels.includes(current)) return current;
  if (current === ALL_REVIEWERS) return ALL_REVIEWERS;
  return defaultAgentFilter(labels);
}

export function agentPillStyle(agentLabel: string): string {
  if (agentLabel === "Professor") {
    return "background: color-mix(in srgb, var(--color-emphasis) 15%, transparent); color: var(--color-emphasis); border-color: color-mix(in srgb, var(--color-emphasis) 25%, transparent)";
  }
  if (agentLabel === "General") {
    return "background: color-mix(in srgb, var(--color-fg-3) 15%, transparent); color: var(--color-fg-3); border-color: color-mix(in srgb, var(--color-fg-3) 25%, transparent)";
  }
  return "background: color-mix(in srgb, var(--color-info) 15%, transparent); color: var(--color-info); border-color: color-mix(in srgb, var(--color-info) 25%, transparent)";
}

export function findingAgentLabel(finding: FlatFinding): string {
  return finding.agent_label ?? finding.expert_label ?? "General";
}

/** True when global markdown summary should be hidden (single non-General agent). */
export function useAgentScopedSummary(agentFilter: AgentFilter): boolean {
  return agentFilter !== ALL_REVIEWERS && agentFilter !== "General";
}

export function agentScopedSummaryLine(
  agentFilter: AgentFilter,
  counts: { high: number; med: number; low: number },
): string {
  const total = counts.high + counts.med + counts.low;
  const kind = agentFilter === "Professor" ? "insight" : "finding";
  const plural = total === 1 ? kind : `${kind}s`;
  return `${total} ${plural} from ${agentFilter}`;
}

export type ResolvedSummary = { text: string; markdown: boolean };

/** Completed review with zero findings. Distinct from missing sidecar output. */
export const EMPTY_FINDINGS_STATUS = "No findings.";

const MISSING_REVIEW_OUTPUT =
  "No findings written. Use Reveal review files to inspect raw output, or re-run the review.";

export type ReviewSummarySource = Pick<
  AiSnapshot,
  "summary_markdown" | "agent_summaries"
> &
  Partial<Pick<AiSnapshot, "has_review_json">>;

/** True when the summary block should lead with EMPTY_FINDINGS_STATUS. */
export function showEmptyFindingsStatus(
  isEmpty: boolean,
  summary: ResolvedSummary,
): boolean {
  return isEmpty && (summary.markdown || summary.text === EMPTY_FINDINGS_STATUS);
}

/** Summary text for the AI Review card for the current agent filter. */
export function resolveAgentSummary(
  ai: ReviewSummarySource,
  agentFilter: AgentFilter,
  counts: { high: number; med: number; low: number },
  fileCount: number,
  isEmpty: boolean,
  fileRiskCount = 0,
): ResolvedSummary {
  if (useAgentScopedSummary(agentFilter)) {
    const scoped = ai.agent_summaries?.[agentFilter]?.trim();
    if (scoped) return { text: scoped, markdown: true };
    if (isEmpty) {
      return { text: `No findings from ${agentFilter}.`, markdown: false };
    }
    return {
      text: agentScopedSummaryLine(agentFilter, counts),
      markdown: false,
    };
  }

  if (ai.summary_markdown?.trim()) {
    return { text: ai.summary_markdown.trim(), markdown: true };
  }

  if (!isEmpty) {
    const total = counts.high + counts.med + counts.low;
    return {
      text: `${total} findings across ${fileCount} files.`,
      markdown: false,
    };
  }

  if (fileRiskCount > 0) {
    const plural = fileRiskCount === 1 ? "file" : "files";
    return {
      text: `No line findings. ${fileRiskCount} ${plural} assessed.`,
      markdown: false,
    };
  }

  if (ai.has_review_json) {
    return { text: EMPTY_FINDINGS_STATUS, markdown: false };
  }

  return { text: MISSING_REVIEW_OUTPUT, markdown: false };
}
