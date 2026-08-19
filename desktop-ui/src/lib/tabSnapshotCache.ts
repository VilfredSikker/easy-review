import type { AppSnapshot, TabSummary } from "./types";

/** Max tab snapshots kept for instant revisit. Matches backend sent-files cap. */
export const TAB_SNAPSHOT_CACHE_CAP = 8;

export type TabCacheEntry = {
  key: string;
  changeToken: string;
  snapshot: AppSnapshot;
};

type TabCacheIdentity = Pick<TabSummary, "idx" | "repo_root" | "kind" | "branch" | "pr_number">;

/**
 * Stable per-tab key. Includes kind + branch so closing tab 0 cannot make the
 * remaining local-branch tab reuse the closed slot (idx is compacted).
 * Mode lives on the snapshot, not the key — last visit of that tab wins.
 */
export function tabSnapshotCacheKeyFromTab(tab: TabCacheIdentity): string {
  return `${tab.idx}|${tab.repo_root}|${tab.kind}|${tab.branch ?? ""}|${tab.pr_number ?? ""}`;
}

function activeTab(snap: AppSnapshot): TabSummary | undefined {
  return (
    snap.tabs.find((t) => t.is_active) ??
    (typeof snap.active_tab === "number" ? snap.tabs[snap.active_tab] : undefined)
  );
}

export function tabSnapshotCacheKey(snap: AppSnapshot): string {
  const tab = activeTab(snap);
  if (!tab) return `idx:${snap.active_tab}`;
  return tabSnapshotCacheKeyFromTab(tab);
}

export function snapshotsShareTabCacheKey(a: AppSnapshot, b: AppSnapshot): boolean {
  return tabSnapshotCacheKey(a) === tabSnapshotCacheKey(b);
}

export function openTabCacheKeys(snap: AppSnapshot | null): Set<string> {
  const keys = new Set<string>();
  if (!snap) return keys;
  for (const tab of snap.tabs) {
    keys.add(tabSnapshotCacheKeyFromTab(tab));
  }
  return keys;
}

/** Keep the live tab strip (and project lists) while painting a cached diff. */
export function applyCachedTabSnapshot(
  cached: AppSnapshot,
  live: AppSnapshot,
  idx: number,
): AppSnapshot {
  return {
    ...cached,
    files: cached.files.map((f) => ({ ...f })),
    tabs: live.tabs.map((t) => ({ ...t, is_active: t.idx === idx })),
    active_tab: idx,
    projects: live.projects,
    panels: live.panels,
    theme: live.theme,
  };
}

/** Drop a command snapshot that belongs to a different tab than the one painted. */
export function shouldDropCommandSnapshot(
  painted: AppSnapshot | null,
  incoming: AppSnapshot,
  allowTabChange: boolean,
): boolean {
  if (allowTabChange || painted === null) return false;
  return tabSnapshotCacheKey(painted) !== tabSnapshotCacheKey(incoming);
}

function isTabDiffStub(snap: AppSnapshot): boolean {
  return snap.bg_loading?.tab_diff === true;
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

  /** Read without LRU bump or change_token eviction. */
  peek(key: string): AppSnapshot | null {
    return this.entries.find((e) => e.key === key)?.snapshot ?? null;
  }

  put(snap: AppSnapshot): void {
    // First-select stubs have empty files. Caching them makes revisit skip the
    // overlay and paint a blank diff.
    if (isTabDiffStub(snap)) return;
    const key = tabSnapshotCacheKey(snap);
    const tab = activeTab(snap);
    const changeToken = tab?.change_token ?? "";
    this.entries = this.entries.filter((e) => e.key !== key);
    this.entries.push({
      key,
      changeToken,
      snapshot: { ...snap, files: snap.files.map((f) => ({ ...f })) },
    });
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
