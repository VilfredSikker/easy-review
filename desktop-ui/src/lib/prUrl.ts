import type { AppSnapshot, GithubStatusSnapshot, TabSummary } from "./types";

/** Parse `owner/repo` from a GitHub git remote URL (HTTPS or SSH). */
export function parseGithubSlug(remote: string): string | null {
  const trimmed = remote.trim();
  if (!trimmed) return null;

  let stripped: string | null = null;
  const https = trimmed.match(/^https?:\/\/github\.com\/(.+)$/i);
  if (https) stripped = https[1];
  const ssh = trimmed.match(/^git@github\.com:(.+)$/i);
  if (ssh) stripped = ssh[1];

  if (!stripped) return null;

  const path = stripped.replace(/\.git$/i, "");
  const parts = path.split("/").filter(Boolean);
  if (parts.length < 2) return null;
  return `${parts[0]}/${parts[1]}`;
}

function activeTab(snapshot: AppSnapshot): TabSummary | undefined {
  return snapshot.tabs.find((t) => t.is_active) ?? snapshot.tabs[snapshot.active_tab];
}

/**
 * PR number for the active review tab. Tab identity wins over the current
 * worktree (two PR tabs on one checkout must not share the checkout's PR).
 */
export function resolveActivePrNumber(snapshot: AppSnapshot | null): number | null {
  if (!snapshot) return null;

  const tab = activeTab(snapshot);
  if (tab?.pr_number != null) return tab.pr_number;
  if (snapshot.pr?.number != null) return snapshot.pr.number;
  if (snapshot.detected_pr_number != null) return snapshot.detected_pr_number;

  const github = snapshot.github?.number ?? null;
  if (github != null) return github;

  const currentWorktree = snapshot.worktrees.find((w) => w.is_current) ?? null;
  return currentWorktree?.pr_number ?? null;
}

/**
 * Live GitHub status for the Branch card. Dropped when `github.number` is a
 * different PR than the active tab (stale cache / worktree collision).
 */
export function githubStatusForActiveTab(
  snapshot: AppSnapshot | null,
): GithubStatusSnapshot | null {
  if (!snapshot?.github) return null;
  const resolved = resolveActivePrNumber(snapshot);
  if (resolved != null && snapshot.github.number !== resolved) return null;
  return snapshot.github;
}

/** PR URL for the active tab — same resolution as the right-panel Branch card. */
export function resolveActivePrUrl(snapshot: AppSnapshot | null): string | null {
  if (!snapshot) return null;

  const prNumber = resolveActivePrNumber(snapshot);
  const github = githubStatusForActiveTab(snapshot);
  if (github?.url) return github.url;
  if (
    snapshot.pr?.url &&
    (prNumber == null || snapshot.pr.number === prNumber)
  ) {
    return snapshot.pr.url;
  }

  if (prNumber == null) return null;

  const tab = activeTab(snapshot);
  const worktreeRemote = snapshot.worktrees.find((w) => w.is_current)?.remote ?? null;
  const remote =
    tab?.remote ??
    worktreeRemote ??
    snapshot.projects.find((p) => p.is_active)?.remote ??
    null;
  if (!remote) return null;

  const slug = parseGithubSlug(remote) ?? (remote.includes("/") && !remote.includes(":") ? remote : null);
  if (!slug) return null;

  return `https://github.com/${slug}/pull/${prNumber}`;
}

/**
 * Auto-pull key for GitHub comments. Tab idx + tab PR, not the worktree or a
 * mismatched github.number — otherwise switching PRs skips the new pull.
 */
export function commentAutoPullKey(snapshot: AppSnapshot | null): string | null {
  if (!snapshot) return null;
  const tab = activeTab(snapshot);
  const pr = tab?.pr_number ?? resolveActivePrNumber(snapshot);
  if (pr == null) return null;
  const repo = tab?.remote ?? tab?.repo_root ?? "unknown";
  return `${snapshot.active_tab}:${repo}:${pr}`;
}
