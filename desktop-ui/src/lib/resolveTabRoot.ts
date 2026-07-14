import type { AppSnapshot, TabSummary } from "./types";

/** Resolved filesystem root for the active tab: the linked worktree path when
 *  the viewed branch is checked out in one, otherwise the tab's `repo_root`. */
export function resolveTabRoot(
  snapshot: AppSnapshot | null | undefined,
  activeTab: TabSummary | undefined,
): string {
  const branch = snapshot?.branch ?? activeTab?.branch ?? null;
  const fromWorktree =
    branch != null
      ? snapshot?.worktrees?.find((w) => w.branch === branch)?.path
      : undefined;
  return fromWorktree?.trim() || activeTab?.repo_root?.trim() || "";
}
