import type { AppSnapshot, TabSummary } from "./pageTypes";

/** Fields the backend uses to refuse a write on a switched view. */
export type ViewParts = {
  active_tab: number;
  repo_root: string;
  pr_number: number | null;
  branch: string;
  mode: string;
};

function activeTab(view: Pick<AppSnapshot, "tabs" | "active_tab">): TabSummary | undefined {
  return (
    view.tabs?.find((t) => t.is_active) ??
    (typeof view.active_tab === "number" ? view.tabs?.[view.active_tab] : undefined)
  );
}

/**
 * The review view the page shows: the active tab's fields and the mode. A PR
 * number or branch filling in later on the same tab is chrome, not a different
 * view, so only the tab's own fields count.
 */
export function viewParts(view: AppSnapshot): ViewParts {
  const tab = activeTab(view);
  return {
    active_tab: view.active_tab,
    repo_root: tab?.repo_root ?? "",
    pr_number: tab?.pr_number ?? null,
    branch: tab?.branch ?? "",
    mode: view.mode ?? "",
  };
}

export function viewIdentity(view: AppSnapshot): string {
  const p = viewParts(view);
  return `${p.active_tab}|${p.repo_root}|${p.pr_number ?? ""}|${p.branch}|${p.mode}`;
}

/**
 * The active tab, by its position and what it opens. A PR tab's branch is left
 * out: its head branch fills in after the tab opens.
 */
export function tabViewKey(view: Pick<AppSnapshot, "tabs" | "active_tab">): string {
  const tab = activeTab(view);
  if (!tab) return `idx:${view.active_tab}`;
  const branch = tab.pr_number != null ? "" : (tab.branch ?? "");
  return `${tab.idx}|${tab.repo_root}|${tab.kind}|${branch}|${tab.pr_number ?? ""}`;
}
