import type { FileRiskSnapshot, FileSnapshot, MatchedRiskRule, RiskTier } from "$lib/types";
import { riskRows } from "$lib/components/rpanel/risksPanel";

/**
 * How the risk queue is ordered. Every signal here is available with no AI.
 *
 * `mtime` and file `size` are not here: the engine sorts by mtime for the diff
 * but neither reaches this card, so offering them would be a control that does
 * nothing until the snapshot carries them.
 */
export type RiskQueueSort = "risk" | "churn" | "status" | "findings" | "comments" | "path";

export type RiskQueueRow = {
  path: string;
  /**
   * The level the row reads as: the declared tier (ADR 0048), or for an `ai`
   * file the review's rating. Still `ai` when the review has not rated it.
   */
  shown: RiskTier;
  /** The tier the rules gave the file; `ai` means no rule fixed one. */
  tier: RiskTier;
  rule: MatchedRiskRule | null;
  /** The review's reason, kept only where the review set the level. */
  aiReason: string | null;
  additions: number;
  deletions: number;
  churn: number;
  reviewed: boolean;
  status: FileSnapshot["status"];
  findingCount: number;
  commentCount: number;
};

const TIER_ORDER: Record<RiskTier, number> = { high: 0, med: 1, low: 2, ai: 3 };

/// Structural changes first: a file that appeared, moved or went away is where
/// a reviewer's assumptions are most likely to be stale.
const STATUS_ORDER: Record<FileSnapshot["status"], number> = {
  added: 0,
  deleted: 1,
  renamed: 2,
  copied: 3,
  unmerged: 4,
  modified: 5,
};

/**
 * The risk card's rows: every changed file at the level the merged risk rules
 * give it, with the review's rating standing in for files the rules leave to
 * the AI (ADR 0036, 0048).
 *
 * A review verdict with no matching file still gets a row — the verdict is what
 * this card is about, and dropping it would hide a file the review flagged
 * because the diff moved on.
 */
export function riskQueueRows(
  risks: FileRiskSnapshot[],
  files: FileSnapshot[],
  sort: RiskQueueSort,
): RiskQueueRow[] {
  const byPath = new Map(files.map((file) => [file.path, file]));
  const rows: RiskQueueRow[] = riskRows(files, risks).map((r) => {
    // riskRows yields exactly one row per input file.
    const file = byPath.get(r.path) as FileSnapshot;
    return {
      path: file.path,
      shown: r.shown,
      tier: r.tier,
      rule: r.rule,
      aiReason: r.aiReason,
      additions: file.additions,
      deletions: file.deletions,
      churn: file.additions + file.deletions,
      reviewed: file.reviewed,
      status: file.status,
      findingCount: file.finding_count ?? 0,
      commentCount: (file.comment_count ?? 0) + (file.question_count ?? 0),
    };
  });

  for (const risk of risks) {
    if (byPath.has(risk.path)) continue;
    rows.push({
      path: risk.path,
      shown: risk.risk,
      tier: "ai",
      rule: null,
      aiReason: risk.risk_reason || null,
      additions: 0,
      deletions: 0,
      churn: 0,
      reviewed: false,
      // Reads as `modified` with nothing behind it rather than being dropped.
      status: "modified",
      findingCount: 0,
      commentCount: 0,
    });
  }

  // Every order falls back to the path, so the list never reshuffles between
  // renders when two rows tie.
  return rows.sort((a, b) => {
    const tieBreak = () => a.path.localeCompare(b.path);
    if (sort === "churn") return b.churn - a.churn || tieBreak();
    if (sort === "path") return tieBreak();
    if (sort === "status") return STATUS_ORDER[a.status] - STATUS_ORDER[b.status] || tieBreak();
    if (sort === "findings") return b.findingCount - a.findingCount || tieBreak();
    if (sort === "comments") return b.commentCount - a.commentCount || tieBreak();
    return TIER_ORDER[a.shown] - TIER_ORDER[b.shown] || tieBreak();
  });
}

/** Per-level counts for the header; `ai` counts files nobody has rated. */
export function riskCounts(rows: RiskQueueRow[]): Record<RiskTier, number> {
  const counts: Record<RiskTier, number> = { high: 0, med: 0, low: 0, ai: 0 };
  for (const row of rows) counts[row.shown] += 1;
  return counts;
}

/**
 * The queue lists files with a level. A file nobody has rated would otherwise
 * crowd the list on a repo with no rules and no review, so it is counted in
 * the header instead.
 */
export function ratedRows(rows: RiskQueueRow[]): RiskQueueRow[] {
  return rows.filter((row) => row.shown !== "ai");
}

/**
 * Why the row has its level. Whoever set the level answers: the review's
 * reason when the tier was `ai`, else the rule that declared it. A rule that
 * only says `ai` is not the reason for a rating.
 */
export function riskWhy(row: RiskQueueRow): { text: string; source: "rule" | "ai" | null } {
  if (row.aiReason) return { text: row.aiReason, source: "ai" };
  if (row.rule && row.tier !== "ai") return { text: row.rule.label || row.rule.key, source: "rule" };
  return { text: "", source: null };
}
