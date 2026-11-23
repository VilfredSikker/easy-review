import { describe, expect, it } from "bun:test";
import type { AppSnapshot, TabSummary } from "./pageTypes";
import { viewTaskTarget } from "./viewIdentity";

function tab(over: Partial<TabSummary>): TabSummary {
  return {
    idx: 0,
    label: "repo",
    kind: "working",
    branch: null,
    pr_number: null,
    remote: null,
    repo_root: "/r/repo",
    is_active: true,
    change_token: "",
    ...over,
  };
}

function view(row: TabSummary, branch = ""): AppSnapshot {
  return { tabs: [row], branch } as unknown as AppSnapshot;
}

describe("viewTaskTarget", () => {
  it("takes a Working tab's branch from its row before diff(tab) has loaded", () => {
    const target = viewTaskTarget(view(tab({ live_branch: "feature" })));
    expect(target?.branch_label).toBe("feature");
  });

  it("falls back to the view's branch while the row has none", () => {
    expect(viewTaskTarget(view(tab({}), "main"))?.branch_label).toBe("main");
  });

  it("files a local PR under its clone, and a remote PR under its owner/repo, as the task host does", () => {
    const localPr = tab({ kind: "local_branch", pr_number: 7, remote: "acme/b", live_branch: "pr/7" });
    expect(viewTaskTarget(view(localPr))?.remote_repo).toBeNull();
    const remotePr = tab({ kind: "remote_pr", pr_number: 9, remote: "acme/g", repo_root: "" });
    expect(viewTaskTarget(view(remotePr))?.remote_repo).toBe("acme/g");
  });
});
