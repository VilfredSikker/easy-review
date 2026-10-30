import type { TabSummary } from "./types";

/** The repo a tab views: its remote slug's name when known, else the clone's folder. */
export function tabPlaceholderRepo(tab: Pick<TabSummary, "remote" | "repo_root">): string {
  const remote = tab.remote?.trim();
  if (remote) return remote.split("/").pop() ?? remote;
  const parts = tab.repo_root.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/** The tab an uncached switch is heading to; null when the shell has nothing to name. */
export function tabPlaceholderTarget(
  tabs: readonly TabSummary[] | undefined,
  switchingIdx: number | null,
): TabSummary | null {
  if (switchingIdx === null || !tabs) return null;
  const target = tabs.find((t) => t.idx === switchingIdx) ?? null;
  return target && !target.is_active ? target : null;
}
