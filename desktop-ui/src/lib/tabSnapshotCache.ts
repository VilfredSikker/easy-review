import type { AppSnapshot, TabSummary } from "./types";

/** Max tab snapshots kept for instant revisit. Matches backend sent-files cap. */
export const TAB_SNAPSHOT_CACHE_CAP = 8;

export type TabCacheEntry = {
  key: string;
  changeToken: string;
  snapshot: AppSnapshot;
};

/** Stable per-tab key (idx + repo + PR). Mode lives on the snapshot, not the key. */
export function tabSnapshotCacheKeyFromTab(tab: Pick<TabSummary, "idx" | "repo_root" | "pr_number">): string {
  return `${tab.idx}|${tab.repo_root}|${tab.pr_number ?? ""}`;
}

export function tabSnapshotCacheKey(snap: AppSnapshot): string {
  const tab =
    snap.tabs.find((t) => t.is_active) ??
    (typeof snap.active_tab === "number" ? snap.tabs[snap.active_tab] : undefined);
  if (!tab) return `idx:${snap.active_tab}`;
  return tabSnapshotCacheKeyFromTab(tab);
}

export function openTabCacheKeys(snap: AppSnapshot | null): Set<string> {
  const keys = new Set<string>();
  if (!snap) return keys;
  for (const tab of snap.tabs) {
    keys.add(tabSnapshotCacheKeyFromTab(tab));
  }
  return keys;
}

export class TabSnapshotCache {
  private entries: TabCacheEntry[] = [];

  get(key: string, changeToken?: string | null): AppSnapshot | null {
    const i = this.entries.findIndex((e) => e.key === key);
    if (i < 0) return null;
    const entry = this.entries[i];
    if (
      changeToken != null &&
      changeToken !== "" &&
      entry.changeToken !== "" &&
      entry.changeToken !== changeToken
    ) {
      this.entries.splice(i, 1);
      return null;
    }
    this.entries.splice(i, 1);
    this.entries.push(entry);
    return entry.snapshot;
  }

  put(snap: AppSnapshot): void {
    const key = tabSnapshotCacheKey(snap);
    const tab =
      snap.tabs.find((t) => t.is_active) ??
      (typeof snap.active_tab === "number" ? snap.tabs[snap.active_tab] : undefined);
    const changeToken = tab?.change_token ?? "";
    this.entries = this.entries.filter((e) => e.key !== key);
    this.entries.push({ key, changeToken, snapshot: snap });
    while (this.entries.length > TAB_SNAPSHOT_CACHE_CAP) {
      this.entries.shift();
    }
  }

  evict(key: string): void {
    this.entries = this.entries.filter((e) => e.key !== key);
  }

  /** Drop entries whose tabs are no longer open. */
  retain(openKeys: Set<string>): void {
    this.entries = this.entries.filter((e) => openKeys.has(e.key));
  }

  get size(): number {
    return this.entries.length;
  }

  /** Test helper. */
  keys(): string[] {
    return this.entries.map((e) => e.key);
  }
}
