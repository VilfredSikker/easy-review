import { describe, expect, test } from "bun:test";
import { findPrTab, sidebarPrOpenRequest } from "./sidebarPrOpen";
import type { TabSummary } from "./types";

const tab = (over: Partial<TabSummary>): TabSummary => ({
  idx: 0,
  label: "tab",
  kind: "local_branch",
  branch: null,
  pr_number: null,
  remote: null,
  repo_root: "/work/app",
  is_active: false,
  change_token: "",
  ...over,
});

const project = { root_path: "/work/app", remote: "https://github.com/acme/app.git" };

describe("findPrTab", () => {
  test("matches a tab on the same remote and PR number", () => {
    const tabs = [tab({ idx: 0, pr_number: 7, remote: "acme/other" }), tab({ idx: 1, pr_number: 7, remote: "Acme/App" })];
    expect(findPrTab(project, 7, tabs)?.idx).toBe(1);
  });

  test("falls back to the clone when the tab names no remote", () => {
    const tabs = [tab({ idx: 2, pr_number: 7, repo_root: "/work/app" })];
    expect(findPrTab(project, 7, tabs)?.idx).toBe(2);
    expect(findPrTab({ ...project, root_path: "/elsewhere" }, 7, tabs)).toBeNull();
  });

  test("a different PR in the same repo is not a match", () => {
    expect(findPrTab(project, 8, [tab({ pr_number: 7, remote: "acme/app" })])).toBeNull();
  });
});

describe("sidebarPrOpenRequest", () => {
  const now = Date.parse("2026-10-30T12:00:00Z");
  const pr = {
    number: 42,
    title: "Reorder migrations",
    author: "sul",
    updated_at: "2026-10-30T11:39:00Z",
    state: "OPEN" as const,
    is_draft: false,
  };

  test("names the PR, its repo and when it last moved", () => {
    const req = sidebarPrOpenRequest({ id: "p1", name: "app" }, pr, now);
    expect(req).toMatchObject({
      key: "p1:42",
      title: "Reorder migrations",
      ref: "app #42",
      context: "Pull request · sul · updated 21 min ago",
      tone: "add-fg",
      quote: null,
    });
  });

  test("says when the PR is a draft or merged", () => {
    expect(sidebarPrOpenRequest({ id: "p", name: "app" }, { ...pr, is_draft: true }, now).context).toStartWith(
      "Draft pull request",
    );
    const merged = sidebarPrOpenRequest({ id: "p", name: "app" }, { ...pr, state: "MERGED", updated_at: "" }, now);
    expect(merged.context).toBe("Merged pull request · sul");
    expect(merged.tone).toBe("periwinkle");
  });
});
