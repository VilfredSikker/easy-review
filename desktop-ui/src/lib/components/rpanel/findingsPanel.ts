import type { Confidence, FlatFinding } from "$lib/types";
import { findingPassesTrust, TRUST_RANK } from "$lib/diffAnnotations";
import { ALL_REVIEWERS, filterByAgent, uniqueAgentLabels, type AgentFilter } from "$lib/aiReviewAgents";

export type Severity = FlatFinding["severity"];
export type SeverityFilter = "all" | Severity;

export const SEVERITIES: readonly Severity[] = ["high", "med", "low"];

const SEVERITY_ORDER: Record<Severity, number> = { high: 0, med: 1, low: 2 };

export interface FindingsPanelView {
  /** What the list draws, in draw order. */
  rows: FlatFinding[];
  /** Open findings per severity, before the gate and the tile filter: a tile
   *  counts what clicking it would show, not what is already shown. */
  counts: Record<Severity, number>;
  openCount: number;
  /** Files carrying at least one open finding. */
  fileCount: number;
  resolvedCount: number;
  /** Open rows matching the tile filter that the gate holds back. Said out
   *  loud because a list that is quietly shorter is how people stop trusting it. */
  gatedCount: number;
}

export interface FindingsPanelOpts {
  severity: SeverityFilter;
  minTrust: Confidence;
  showResolved: boolean;
}

/**
 * Severity first, then how much the grade can be trusted — the order the TUI
 * panel draws — with resolved rows sunk below every open one so a fixed
 * finding never sits above work still to do.
 */
export function findingsPanelView(
  open: readonly FlatFinding[],
  resolved: readonly FlatFinding[],
  opts: FindingsPanelOpts,
): FindingsPanelView {
  const counts: Record<Severity, number> = { high: 0, med: 0, low: 0 };
  for (const f of open) counts[f.severity]++;

  const bySeverity = (f: FlatFinding) => opts.severity === "all" || f.severity === opts.severity;
  const candidates = open.filter(bySeverity);
  const visibleOpen = candidates.filter((f) => findingPassesTrust(f, opts.minTrust));
  const visibleResolved = opts.showResolved
    ? resolved.filter((f) => bySeverity(f) && findingPassesTrust(f, opts.minTrust))
    : [];

  const rows = [...visibleOpen, ...visibleResolved].sort(
    (a, b) =>
      Number(a.resolved) - Number(b.resolved) ||
      SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity] ||
      TRUST_RANK[a.confidence] - TRUST_RANK[b.confidence] ||
      a.file.localeCompare(b.file) ||
      (a.line ?? 0) - (b.line ?? 0),
  );

  return {
    rows,
    counts,
    openCount: open.length,
    fileCount: new Set(open.map((f) => f.file)).size,
    resolvedCount: resolved.length,
    gatedCount: candidates.length - visibleOpen.length,
  };
}

/** Clicking the active tile again clears it, so a tile is its own off switch. */
export function toggleSeverity(current: SeverityFilter, clicked: Severity): SeverityFilter {
  return current === clicked ? "all" : clicked;
}

/** The row's right-hand tag: the category half of the engine's
 *  `producers · category` tag. The producers are already on the finding card;
 *  in a list the category is what tells two rows apart. */
export function findingLens(finding: Pick<FlatFinding, "raised_by" | "lens_category">): string {
  const tail = finding.lens_category.split("·").pop()?.trim();
  if (tail) return tail;
  return finding.raised_by[finding.raised_by.length - 1] ?? "";
}

/** Truncate from the end with an ellipsis; file names lead with what differs. */
export function clip(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 2)}…` : text;
}

/** `name:line` for a row; a finding without a line names only the file. */
export function findingLocation(name: string, line: number | null, max = 24): string {
  const shown = clip(name, max);
  return line === null ? shown : `${shown}:${line}`;
}

/** Arbiter rulings as one line: "2 dropped, 1 merged by arbiter", or null. */
export function arbiterSummary(counts: {
  arbiter_dropped: number;
  arbiter_merged: number;
  arbiter_regraded: number;
}): string | null {
  const parts = [
    counts.arbiter_dropped > 0 ? `${counts.arbiter_dropped} dropped` : "",
    counts.arbiter_merged > 0 ? `${counts.arbiter_merged} merged` : "",
    counts.arbiter_regraded > 0 ? `${counts.arbiter_regraded} regraded` : "",
  ].filter(Boolean);
  return parts.length > 0 ? `${parts.join(", ")} by arbiter` : null;
}

export interface ReviewerScope {
  open: FlatFinding[];
  resolved: FlatFinding[];
  /** Reviewers the picker offers, in registry order. */
  labels: string[];
}

/**
 * The findings one reviewer raised, under the same filter the diff applies
 * (`aiReviewFilter`), so the panel, its tab count and the collapsed rail never
 * count rows the diff is hiding.
 */
export function scopeToReviewer(
  ai: { findings: FlatFinding[]; resolved_findings?: FlatFinding[]; agent_summaries?: Record<string, string> } | null,
  filter: AgentFilter,
): ReviewerScope {
  const findings = ai?.findings ?? [];
  return {
    open: filterByAgent(findings, filter),
    resolved: filterByAgent(ai?.resolved_findings ?? [], filter),
    labels: uniqueAgentLabels(findings, Object.keys(ai?.agent_summaries ?? {})),
  };
}

/** A picker is worth showing when there is a choice, or a filter hiding rows.
 *  A lone reviewer's own filter hides nothing, so it gets no picker. */
export function showReviewerPicker(labels: readonly string[], filter: AgentFilter): boolean {
  if (labels.length > 1) return true;
  return filter !== ALL_REVIEWERS && !(labels.length === 1 && labels[0] === filter);
}
