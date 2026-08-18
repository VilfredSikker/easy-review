import type { AppSnapshot } from "./types";

/**
 * Stable key for the once-per-PR GitHub comment auto-pull.
 *
 * Must not depend on whether `snapshot.github` (live status cache) is
 * populated. That cache is filled asynchronously and can be missing from a
 * command snapshot (e.g. adding a local inline comment). Keying on it
 * re-fired `pull_github_comments` — spinner + network — for every local add.
 */
export function githubCommentsAutoPullKey(
  snapshot: Pick<AppSnapshot, "branch" | "pr" | "github" | "tabs" | "active_tab"> | null,
): string | null {
  if (!snapshot?.branch) return null;

  const activeTab =
    snapshot.tabs?.find((t) => t.is_active) ??
    (typeof snapshot.active_tab === "number" ? snapshot.tabs?.[snapshot.active_tab] : undefined);

  const pr = snapshot.pr?.number ?? activeTab?.pr_number ?? snapshot.github?.number ?? null;
  if (!pr) return null;

  const repoKey = activeTab?.remote || activeTab?.repo_root || "unknown";
  return `${repoKey}:${snapshot.branch}:${pr}`;
}
