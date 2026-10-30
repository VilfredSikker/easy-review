import { describe, expect, it } from "bun:test";
import {
  filterInboxThreads,
  findOpenTab,
  groupInboxThreads,
  inboxDay,
  inboxReadCounts,
  inboxThreadRef,
  inboxThreadRole,
  inboxThreadTitle,
  inboxTypeChips,
  selectedInboxThread,
  stepInboxSelection,
  withDaySeparators,
} from "./inboxThreads";
import type { InboxItemSnapshot, PrInfo, ProjectSnapshot, TabSummary } from "./types";

const NOW = new Date(2026, 9, 30, 15, 0, 0).getTime();
const HOUR = 3_600_000;

function item(id: string, o: Partial<InboxItemSnapshot> = {}): InboxItemSnapshot {
  return {
    id,
    kind: "pr_comment",
    severity: "info",
    title: `title ${id}`,
    body: "",
    source: "github",
    target: { project_id: "p1", pr_number: 7 },
    created_at_ms: NOW - HOUR,
    read_at_ms: null,
    dedupe_key: id,
    ...o,
  };
}

function project(o: Partial<ProjectSnapshot> = {}): ProjectSnapshot {
  return {
    id: "p1",
    name: "discovery",
    root_path: "/repo/discovery",
    remote: "org/discovery",
    is_active: true,
    local_branches: [],
    auto_branches: [],
    saved_prs: [],
    my_prs: [],
    prs_to_review: [],
    recent_prs: [],
    recently_merged: [],
    ...o,
  };
}

const pr = (number: number, title: string) => ({ number, title }) as PrInfo;

function tab(o: Partial<TabSummary>): TabSummary {
  return {
    idx: 0,
    label: "t",
    kind: "remote_pr",
    branch: null,
    pr_number: null,
    remote: "org/discovery",
    repo_root: "/repo/discovery",
    is_active: false,
    change_token: "",
    ...o,
  };
}

describe("groupInboxThreads", () => {
  it("folds every event on one PR into one thread, oldest event first", () => {
    const threads = groupInboxThreads(
      [
        item("late", { created_at_ms: NOW - HOUR, read_at_ms: NOW }),
        item("early", { kind: "review_requested", created_at_ms: NOW - 3 * HOUR }),
        item("other", { target: { project_id: "p1", pr_number: 8 }, created_at_ms: NOW - 2 * HOUR }),
      ],
      [project()],
    );
    expect(threads.map((t) => t.prNumber)).toEqual([7, 8]);
    expect(threads[0].items.map((i) => i.id)).toEqual(["early", "late"]);
    expect(threads[0].latest.id).toBe("late");
    expect(threads[0].unreadIds).toEqual(["early"]);
    expect(threads[0].categories).toEqual(["pr_comment", "review_requested"]);
  });

  it("keys a stale project id and its live remote to the same PR", () => {
    const threads = groupInboxThreads(
      [
        item("a", { target: { project_id: "gone", remote: "Org/Discovery", pr_number: 7 } }),
        item("b", { target: { project_id: "p1", pr_number: 7 } }),
      ],
      [project()],
    );
    expect(threads).toHaveLength(1);
  });

  it("groups branch-only items by branch and leaves targetless items alone", () => {
    const threads = groupInboxThreads(
      [
        item("ci", { kind: "ci_failed", target: { project_id: "p1", branch: "feat" } }),
        item("ai", { kind: "ai_review_done", target: { project_id: "p1", branch: "feat" } }),
        item("x", { kind: "github_refresh_failed", target: {} }),
        item("y", { kind: "github_refresh_failed", target: {} }),
      ],
      [project()],
    );
    expect(threads.map((t) => t.items.length).sort()).toEqual([1, 1, 2]);
  });
});

describe("filters and counts", () => {
  const threads = groupInboxThreads(
    [
      item("a", { kind: "review_requested" }),
      item("b", { kind: "pr_comment", target: { project_id: "p1", pr_number: 8 }, read_at_ms: NOW }),
      item("c", { kind: "ci_failed", target: { project_id: "p2", pr_number: 9 } }),
    ],
    [project(), project({ id: "p2", name: "ink", remote: "org/ink", root_path: "/repo/ink" })],
  );

  it("counts PRs, not events, by read state", () => {
    expect(inboxReadCounts(threads)).toEqual({ all: 3, unread: 2, read: 1 });
  });

  it("filters by project, read state and type together", () => {
    const f = (o: Partial<Parameters<typeof filterInboxThreads>[1]>) =>
      filterInboxThreads(threads, { read: "all", projectId: "all", category: "all", ...o }).map((t) => t.prNumber);
    expect(f({ projectId: "p2" })).toEqual([9]);
    expect(f({ read: "read" })).toEqual([8]);
    expect(f({ read: "unread", category: "review_requested" })).toEqual([7]);
  });

  it("offers a chip for each type present, counting each PR once", () => {
    expect(inboxTypeChips(threads).map((c) => [c.category, c.count])).toEqual([
      ["pr_comment", 1],
      ["review_requested", 1],
      ["ci", 1],
    ]);
  });
});

describe("day separators", () => {
  it("buckets by calendar day, not by 24 hours", () => {
    const midnight = new Date(2026, 9, 30).getTime();
    expect(inboxDay(midnight + 1, NOW)).toBe("Today");
    expect(inboxDay(midnight - 1, NOW)).toBe("Yesterday");
    expect(inboxDay(midnight - 25 * HOUR, NOW)).toBe("Earlier");
  });

  it("labels only the first row of each day", () => {
    const threads = groupInboxThreads(
      [
        item("a", { created_at_ms: NOW - HOUR }),
        item("b", { created_at_ms: NOW - 2 * HOUR, target: { project_id: "p1", pr_number: 8 } }),
        item("c", { created_at_ms: NOW - 40 * HOUR, target: { project_id: "p1", pr_number: 9 } }),
      ],
      [project()],
    );
    expect(withDaySeparators(threads, NOW).map((r) => r.day)).toEqual(["Today", null, "Earlier"]);
  });
});

describe("thread facts", () => {
  it("takes the PR title from the sidebar lists when it is there", () => {
    const [t] = groupInboxThreads([item("a")], [project({ my_prs: [pr(7, "Reorder migrations")] })]);
    expect(inboxThreadTitle(t, [project({ my_prs: [pr(7, "Reorder migrations")] })])).toBe("Reorder migrations");
    expect(inboxThreadTitle(t, [project()])).toBe("title a");
    expect(inboxThreadRef(t, [project()])).toBe("discovery #7");
  });

  it("works out the reader's role from the lists, then from the event types", () => {
    const [t] = groupInboxThreads([item("a", { kind: "mention" })], [project()]);
    expect(inboxThreadRole(t, [project({ prs_to_review: [pr(7, "x")] })])).toBe("reviewer");
    expect(inboxThreadRole(t, [project({ my_prs: [pr(7, "x")] })])).toBe("author");
    expect(inboxThreadRole(t, [project()])).toBeNull();
    const [c] = groupInboxThreads([item("b", { kind: "pr_comment" })], [project()]);
    expect(inboxThreadRole(c, [project()])).toBe("author");
  });
});

describe("findOpenTab", () => {
  const [thread] = groupInboxThreads([item("a")], [project()]);

  it("finds a tab already on the same PR in the same repo", () => {
    const tabs = [tab({ idx: 0, pr_number: 7, remote: "org/ink" }), tab({ idx: 2, pr_number: 7 })];
    expect(findOpenTab(thread, tabs, [project()])?.idx).toBe(2);
  });

  it("returns null when no tab shows that PR", () => {
    expect(findOpenTab(thread, [tab({ pr_number: 8 })], [project()])).toBeNull();
  });
});

describe("selection", () => {
  const threads = groupInboxThreads(
    [1, 2, 3].map((n) => item(`i${n}`, { target: { project_id: "p1", pr_number: n }, created_at_ms: NOW - n * HOUR })),
    [project()],
  );

  it("falls back to the first row when the chosen one is filtered out", () => {
    expect(selectedInboxThread(threads, "missing")?.prNumber).toBe(1);
    expect(selectedInboxThread([], null)).toBeNull();
  });

  it("steps and clamps at both ends", () => {
    expect(stepInboxSelection(threads, threads[0].key, 1)?.prNumber).toBe(2);
    expect(stepInboxSelection(threads, threads[2].key, 1)?.prNumber).toBe(3);
    expect(stepInboxSelection(threads, threads[0].key, -1)?.prNumber).toBe(1);
    expect(stepInboxSelection(threads, null, 1)?.prNumber).toBe(1);
  });
});
