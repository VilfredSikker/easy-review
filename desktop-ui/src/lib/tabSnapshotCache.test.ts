import { describe, expect, it } from "vitest";
import type { AppSnapshot, TabSummary } from "./types";
import {
  TAB_SNAPSHOT_CACHE_CAP,
  TabSnapshotCache,
  applyCachedTabSnapshot,
  shouldDropCommandSnapshot,
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
    file_risks: [],
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
  it("keys by idx, repo, kind, branch, and PR — not the worktree", () => {
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
    expect(tabSnapshotCacheKey(a)).toBe("0|/repo|remote_pr||1469");
    expect(tabSnapshotCacheKey(b)).toBe("1|/repo|remote_pr||1473");
    expect(tabSnapshotCacheKey(a)).not.toBe(tabSnapshotCacheKey(b));
  });

  it("does not collide when idx is reused after close of a different branch", () => {
    const closed = tabSnapshotCacheKeyFromTab(
      tab({ idx: 0, label: "feat-a", kind: "local_branch", branch: "feat-a" }),
    );
    const reused = tabSnapshotCacheKeyFromTab(
      tab({ idx: 0, label: "feat-b", kind: "local_branch", branch: "feat-b" }),
    );
    expect(closed).toBe("0|/repo|local_branch|feat-a|");
    expect(reused).toBe("0|/repo|local_branch|feat-b|");
    expect(closed).not.toBe(reused);
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
    expect(cache.get("0|/repo|remote_pr||1469")?.ai.high).toBe(4);
    expect(cache.get("1|/repo|remote_pr||1473")?.ai.high).toBe(9);
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
    cache.retain(new Set(["1|/repo|remote_pr||2"]));
    expect(cache.get("0|/repo|remote_pr||1")).toBeNull();
    expect(cache.get("1|/repo|remote_pr||2")).not.toBeNull();
  });

  it("evicts when change_token moves", () => {
    const cache = new TabSnapshotCache();
    cache.put(
      snap({
        active_tab: 0,
        tabs: [tab({ idx: 0, label: "a", pr_number: 1, is_active: true, change_token: "old" })],
      }),
    );
    expect(cache.get("0|/repo|remote_pr||1", "new")).toBeNull();
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
    expect(cache.get("0|/repo|remote_pr||0")).toBeNull();
    expect(
      cache.get(
        `${TAB_SNAPSHOT_CACHE_CAP + 1}|/repo|remote_pr||${TAB_SNAPSHOT_CACHE_CAP + 1}`,
      ),
    ).not.toBeNull();
  });

  it("does not cache a tab_diff stub", () => {
    const cache = new TabSnapshotCache();
    cache.put(
      snap({
        active_tab: 0,
        tabs: [tab({ idx: 0, label: "a", pr_number: 1, is_active: true })],
        bg_loading: {
          pr_list: false,
          gh_status: false,
          gh_comments: false,
          tab_diff: true,
        },
      }),
    );
    expect(cache.size).toBe(0);
  });

  it("clones files so later mutations do not hit the cache", () => {
    const cache = new TabSnapshotCache();
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "a", pr_number: 1, is_active: true })],
      files: [
        {
          path: "a.ts",
          status: "modified",
          additions: 1,
          deletions: 0,
          reviewed: false,
          compacted: false,
          risk: null,
          finding_count: 0,
          comment_count: 0,
          question_count: 0,
          hunks: [],
          source_index: 0,
          cache_key: "ck",
        } as AppSnapshot["files"][number],
      ],
    });
    cache.put(a);
    a.files[0].reviewed = true;
    expect(cache.peek("0|/repo|remote_pr||1")?.files[0].reviewed).toBe(false);
  });
});

describe("applyCachedTabSnapshot", () => {
  it("overlays the live tab strip onto the cached diff", () => {
    const cached = snap({
      active_tab: 0,
      branch: "pr-1469",
      tabs: [tab({ idx: 0, label: "a", pr_number: 1469, is_active: true })],
      ai: { ...emptyAi(), high: 4 },
    });
    const live = snap({
      active_tab: 1,
      tabs: [
        tab({ idx: 0, label: "a", pr_number: 1469 }),
        tab({ idx: 1, label: "new", pr_number: 1473, is_active: true }),
      ],
      projects: [
        {
          id: "p1",
          name: "proj",
          root_path: "/repo",
          remote: null,
          is_active: true,
          local_branches: [],
          auto_branches: [],
          saved_prs: [],
          my_prs: [],
          prs_to_review: [],
          recent_prs: [],
          recently_merged: [],
        },
      ],
    });
    const painted = applyCachedTabSnapshot(cached, live, 0);
    expect(painted.ai.high).toBe(4);
    expect(painted.branch).toBe("pr-1469");
    expect(painted.active_tab).toBe(0);
    expect(painted.tabs).toHaveLength(2);
    expect(painted.tabs[0].is_active).toBe(true);
    expect(painted.tabs[1].is_active).toBe(false);
    expect(painted.projects).toHaveLength(1);
    expect(painted.files).not.toBe(cached.files);
    if (cached.files[0] && painted.files[0]) {
      expect(painted.files[0]).not.toBe(cached.files[0]);
    }
  });
});

describe("shouldDropCommandSnapshot", () => {
  it("drops a different tab unless allowTabChange", () => {
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "a", pr_number: 1469, is_active: true })],
    });
    const b = snap({
      active_tab: 1,
      tabs: [tab({ idx: 1, label: "b", pr_number: 1473, is_active: true })],
    });
    expect(shouldDropCommandSnapshot(a, b, false)).toBe(true);
    expect(shouldDropCommandSnapshot(a, b, true)).toBe(false);
    expect(shouldDropCommandSnapshot(a, a, false)).toBe(false);
    expect(shouldDropCommandSnapshot(null, b, false)).toBe(false);
  });
});
