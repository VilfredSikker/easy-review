import type { DiffStaleSnapshot, LocalBranchDivergenceSnapshot } from "./types";

/** Seven characters, as git prints a short SHA. */
export function shortOid(oid: string): string {
  return oid.slice(0, 7);
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

export interface StaleBannerCopy {
  /** What moved, then what the open diff and its artifacts were built from. */
  message: string;
  latestShort: string;
  builtShort: string;
}

/**
 * Copy for the banner under the view bar while the open diff is behind
 * origin. The artifacts are described as built from the old commit, not as
 * stale: they still match the diff on screen until a Sync replaces it, and
 * artifact staleness is a different condition (ADR 0010).
 */
export function staleBannerCopy(stale: DiffStaleSnapshot, baseName: string | null): StaleBannerCopy {
  const latestShort = shortOid(stale.latest_oid);
  const builtShort = shortOid(stale.built_from_oid);
  const base = baseName ? `The base ${baseName}` : "The base";
  const what = stale.kind === "pr_head" ? "The PR head" : base;
  // `commits_behind` is absent when the new commits are not fetched yet, and
  // counting them would mean a fetch on every snapshot build.
  const count =
    stale.commits_behind != null && stale.commits_behind > 0
      ? ` (${plural(stale.commits_behind, "new commit", "new commits")})`
      : "";
  const builtVerb = stale.kind === "pr_head" ? "built from" : "built against";
  return {
    latestShort,
    builtShort,
    message:
      `${what} moved to ${latestShort}${count}. This diff, its triage, findings and tour were ` +
      `${builtVerb} ${builtShort}. Syncing clears Reviewed on files whose diff changed.`,
  };
}

export interface BannerSegment {
  text: string;
  /** Drawn as inline code: a commit the reader may want to copy. */
  code: boolean;
}

/** `message` split around the given short SHAs, so they can be set as code. */
export function oidSegments(message: string, oids: readonly string[]): BannerSegment[] {
  const wanted = oids.filter((o) => o.length > 0);
  if (wanted.length === 0) return [{ text: message, code: false }];
  const out: BannerSegment[] = [];
  for (const part of message.split(new RegExp(`(${wanted.join("|")})`))) {
    if (part) out.push({ text: part, code: wanted.includes(part) });
  }
  return out;
}

/**
 * Copy for the banner on a local PR tab's local branch view, or null when the
 * worktree matches the PR head and there is nothing to explain.
 *
 * Questions, notes, findings and Reviewed marks are kept per view, but a tour
 * is reused across views whenever the two diffs hash the same (ADR 0004,
 * ADR 0025), so the copy cannot say that nothing carries over.
 */
export function localBranchBannerCopy(div: LocalBranchDivergenceSnapshot): string | null {
  const ahead = div.ahead;
  const unstaged = div.unstaged_files;
  const parts: string[] = [];
  if (ahead != null && ahead > 0) {
    parts.push(`is ${plural(ahead, "unpushed commit", "unpushed commits")} ahead of the PR head`);
  }
  if (unstaged != null && unstaged > 0) {
    parts.push(`has ${plural(unstaged, "unstaged change", "unstaged changes")}`);
  }
  const carry =
    "Findings, notes and Reviewed marks stay with the view they were made in; the tour is shared while both diffs match.";
  if (parts.length > 0) return `Your worktree ${parts.join(" and ")}. ${carry}`;
  // Without the PR head locally the ahead count is unknown, so the worktree
  // may still differ from the PR.
  if (ahead == null) return `The PR head ${shortOid(div.pr_head_oid)} is not fetched, so this worktree may differ from it. ${carry}`;
  return null;
}

/**
 * Reviewed marks a Sync cleared, counted from the snapshots either side of
 * it. A file that left the diff is not counted: its mark went with the file,
 * not because its diff changed.
 */
export function reviewedClearedBySync(
  before: readonly { path: string; reviewed: boolean }[],
  after: readonly { path: string; reviewed: boolean }[],
): number {
  const wasReviewed = new Set(before.filter((f) => f.reviewed).map((f) => f.path));
  return after.filter((f) => !f.reviewed && wasReviewed.has(f.path)).length;
}
