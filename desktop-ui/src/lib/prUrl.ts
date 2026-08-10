import type { AppSnapshot } from "$lib/types";

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

/** PR URL for the active tab — same resolution as the right-panel Branch card. */
export function resolveActivePrUrl(snapshot: AppSnapshot | null): string | null {
  if (!snapshot) return null;

  const direct = snapshot.github?.url ?? snapshot.pr?.url;
  if (direct) return direct;

  const activeTab = snapshot.tabs.find((t) => t.is_active) ?? snapshot.tabs[snapshot.active_tab];
  const currentWorktree = snapshot.worktrees.find((w) => w.is_current) ?? null;

  const prNumber =
    snapshot.github?.number ??
    snapshot.pr?.number ??
    activeTab?.pr_number ??
    currentWorktree?.pr_number ??
    null;

  if (prNumber == null) return null;

  // Resolve the repo from the tab's own remote first — a project may point at
  // a different repo than the branch/PR actually being viewed (e.g. a project
  // registered under the EasyReview repo reviewing a PR from another repo).
  // Fall back to the current worktree's remote, then the active project's.
  const worktreeRemote = snapshot.worktrees.find((w) => w.is_current)?.remote ?? null;
  const remote =
    activeTab?.remote ??
    worktreeRemote ??
    snapshot.projects.find((p) => p.is_active)?.remote ??
    null;
  if (!remote) return null;

  const slug = parseGithubSlug(remote);
  if (!slug) return null;

  return `https://github.com/${slug}/pull/${prNumber}`;
}
