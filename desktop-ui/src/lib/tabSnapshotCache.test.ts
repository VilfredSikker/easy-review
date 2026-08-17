import { describe, expect, it } from "vitest";
import type { AppSnapshot, TabSummary } from "./types";
import {
  TAB_SNAPSHOT_CACHE_CAP,
  TabSnapshotCache,
  tabSnapshotCacheKey,
  tabSnapshotCacheKeyFromTab,
} from "./tabSnapshotCache";

function emptyAi(): AppSnapshot["ai"] {
  return {
    fresh: true,
    stale_reason: null,
    summary_markdown: null,
    agent_summaries: {},
    high: 0,
    med: 0,
    low: 0,
    local_comment_count: 0,
    github_comment_count: 0,
    comments: 0,
    questions: 0,
    notes: 0,
    unpushed: 0,
    threads: [],
    findings: [],
    has_review_json: false,
    eligible_comment_count: 0,
    triage: null,
  };
}

function tab(partial: Partial<TabSummary> & Pick<TabSummary, "idx" | "label">): TabSummary {
  return {
    kind: "remote_pr",
    branch: null,
    pr_number: null,
    remote: null,
    repo_root: "/repo",
    is_active: false,
    change_token: "t0",
    ...partial,
  };
}

function snap(partial: Partial<AppSnapshot> & { active_tab: number; tabs: TabSummary[] }): AppSnapshot {
  return {
    mode: "pr",
    branch: "main",
    base: "main",
    input_mode: "normal",
    files: [],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 0,
    ai: emptyAi(),
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "graphite",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    bg_loading: {
      pr_list: false,
      gh_status: false,
      gh_comments: false,
      tab_diff: false,
    },
    ...partial,
  };
}

describe("tabSnapshotCacheKey", () => {
  it("keys by tab idx, repo, and PR — not the worktree", () => {
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "a", pr_number: 1469, is_active: true })],
    });
    const b = snap({
      active_tab: 1,
      tabs: [
        tab({ idx: 0, label: "a", pr_number: 1469 }),
        tab({ idx: 1, label: "b", pr_number: 1473, is_active: true }),
      ],
    });
    expect(tabSnapshotCacheKey(a)).toBe("0|/repo|1469");
    expect(tabSnapshotCacheKey(b)).toBe("1|/repo|1473");
    expect(tabSnapshotCacheKey(a)).not.toBe(tabSnapshotCacheKey(b));
  });
});

describe("TabSnapshotCache", () => {
  it("returns a stored snapshot on hit", () => {
    const cache = new TabSnapshotCache();
    const a = snap({
      active_tab: 0,
      branch: "pr-1469",
      tabs: [tab({ idx: 0, label: "a", pr_number: 1469, is_active: true, change_token: "c1" })],
      ai: { ...emptyAi(), high: 4 },
    });
    cache.put(a);
    const hit = cache.get(tabSnapshotCacheKeyFromTab(a.tabs[0]), "c1");
    expect(hit?.ai.high).toBe(4);
    expect(hit?.branch).toBe("pr-1469");
  });

  it("does not leak write-through across identities", () => {
    const cache = new TabSnapshotCache();
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "a", pr_number: 1469, is_active: true })],
      ai: { ...emptyAi(), high: 4 },
    });
    const b = snap({
      active_tab: 1,
      tabs: [
        tab({ idx: 0, label: "a", pr_number: 1469 }),
        tab({ idx: 1, label: "b", pr_number: 1473, is_active: true }),
      ],
      ai: { ...emptyAi(), high: 9 },
    });
    cache.put(a);
    cache.put(b);
    expect(cache.get("0|/repo|1469")?.ai.high).toBe(4);
    expect(cache.get("1|/repo|1473")?.ai.high).toBe(9);
  });

  it("evicts on close via retain", () => {
    const cache = new TabSnapshotCache();
    cache.put(
      snap({
        active_tab: 0,
        tabs: [tab({ idx: 0, label: "a", pr_number: 1, is_active: true })],
      }),
    );
    cache.put(
      snap({
        active_tab: 1,
        tabs: [
          tab({ idx: 0, label: "a", pr_number: 1 }),
          tab({ idx: 1, label: "b", pr_number: 2, is_active: true }),
        ],
      }),
    );
    cache.retain(new Set(["1|/repo|2"]));
    expect(cache.get("0|/repo|1")).toBeNull();
    expect(cache.get("1|/repo|2")).not.toBeNull();
  });

  it("evicts when change_token moves", () => {
    const cache = new TabSnapshotCache();
    cache.put(
      snap({
        active_tab: 0,
        tabs: [tab({ idx: 0, label: "a", pr_number: 1, is_active: true, change_token: "old" })],
      }),
    );
    expect(cache.get("0|/repo|1", "new")).toBeNull();
  });

  it("caps LRU at TAB_SNAPSHOT_CACHE_CAP", () => {
    const cache = new TabSnapshotCache();
    for (let i = 0; i < TAB_SNAPSHOT_CACHE_CAP + 2; i++) {
      cache.put(
        snap({
          active_tab: i,
          tabs: [tab({ idx: i, label: `t${i}`, pr_number: i, is_active: true })],
        }),
      );
    }
    expect(cache.size).toBe(TAB_SNAPSHOT_CACHE_CAP);
    expect(cache.get("0|/repo|0")).toBeNull();
    expect(cache.get(`${TAB_SNAPSHOT_CACHE_CAP + 1}|/repo|${TAB_SNAPSHOT_CACHE_CAP + 1}`)).not.toBeNull();
  });
});
