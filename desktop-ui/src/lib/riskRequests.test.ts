import { describe, expect, test } from "bun:test";
import { changedPaths, riskTarget } from "./riskRequests";
import type { AppSnapshot, FileSnapshot, TabSummary } from "./types";

const tab = (over: Partial<TabSummary>): TabSummary =>
  ({
    idx: 0,
    label: "t",
    kind: "working",
    branch: null,
    pr_number: null,
    remote: "acme/widgets",
    repo_root: "/work/widgets",
    is_active: true,
    change_token: "",
    ...over,
  }) as TabSummary;

const snapshot = (tabs: TabSummary[], paths: string[] = []): AppSnapshot =>
  ({ tabs, files: paths.map((path) => ({ path }) as FileSnapshot) }) as AppSnapshot;

describe("riskTarget", () => {
  test("names the active tab's root, and no remote for a clone", () => {
    const target = riskTarget(snapshot([tab({ is_active: false, repo_root: "/other" }), tab({ idx: 1 })]));
    expect(target).toEqual({ repoRoot: "/work/widgets", remoteRepo: null });
  });

  test("names the PR's repo for a remote-only tab", () => {
    const target = riskTarget(snapshot([tab({ kind: "remote_pr", repo_root: "/" })]));
    expect(target).toEqual({ repoRoot: "/", remoteRepo: "acme/widgets" });
  });

  test("has no target without an active tab", () => {
    expect(riskTarget(snapshot([tab({ is_active: false })]))).toBeNull();
    expect(riskTarget(null)).toBeNull();
  });
});

describe("changedPaths", () => {
  test("lists the snapshot's files in order", () => {
    expect(changedPaths(snapshot([], ["b.rs", "a.rs"]))).toEqual(["b.rs", "a.rs"]);
    expect(changedPaths(null)).toEqual([]);
  });
});
