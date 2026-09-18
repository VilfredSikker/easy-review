import type { FileRiskSnapshot, FileSnapshot } from "$lib/types";

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
  risk: FileRiskSnapshot["risk"];
  riskReason: string;
  churn: number;
  reviewed: boolean;
  status: FileSnapshot["status"];
  findingCount: number;
  commentCount: number;
};

const RISK_ORDER: Record<FileRiskSnapshot["risk"], number> = { high: 0, med: 1, low: 2 };

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
 * The risk card's rows: the review's per-file verdicts, joined to what the diff
 * knows about each file.
 *
 * A verdict with no matching file still gets a row — the verdict is what this
 * card is about, and dropping it would hide a file the review flagged because
 * the diff moved on.
 */
export function riskQueueRows(
  risks: FileRiskSnapshot[],
  files: FileSnapshot[],
  sort: RiskQueueSort,
): RiskQueueRow[] {
  const byPath = new Map(files.map((file) => [file.path, file]));

  const rows = risks.map((risk) => {
    const file = byPath.get(risk.path);
    return {
      path: risk.path,
      risk: risk.risk,
      riskReason: risk.risk_reason,
      churn: (file?.additions ?? 0) + (file?.deletions ?? 0),
      reviewed: file?.reviewed ?? false,
      // A verdict whose file has left the diff still gets a row, and reads as
      // `modified` with nothing behind it rather than being dropped.
      status: file?.status ?? "modified",
      findingCount: file?.finding_count ?? 0,
      commentCount: (file?.comment_count ?? 0) + (file?.question_count ?? 0),
    };
  });

  // Every order falls back to the path, so the list never reshuffles between
  // renders when two rows tie.
  return rows.sort((a, b) => {
    const tieBreak = () => a.path.localeCompare(b.path);
    if (sort === "churn") return b.churn - a.churn || tieBreak();
    if (sort === "path") return tieBreak();
    if (sort === "status") return STATUS_ORDER[a.status] - STATUS_ORDER[b.status] || tieBreak();
    if (sort === "findings") return b.findingCount - a.findingCount || tieBreak();
    if (sort === "comments") return b.commentCount - a.commentCount || tieBreak();
    return RISK_ORDER[a.risk] - RISK_ORDER[b.risk] || tieBreak();
  });
}

/** Per-level counts for the header. */
export function riskCounts(rows: RiskQueueRow[]): { high: number; med: number; low: number } {
  const counts = { high: 0, med: 0, low: 0 };
  for (const row of rows) counts[row.risk] += 1;
  return counts;
}
