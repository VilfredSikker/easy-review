import { describe, expect, it } from "bun:test";
import { arenaRunsAcrossTabs, tabRowModel } from "./tabStripModel";
import type { TabSummary } from "./types";
import type { ArenaRunSummary } from "./types/arena";

const tab = (p: Partial<TabSummary>): TabSummary => ({
  idx: 0,
  label: "easy-review",
  kind: "working",
  branch: null,
  pr_number: null,
  remote: null,
  repo_root: "/r",
  is_active: false,
  change_token: "",
  ...p,
});
const none = { running: false, unseen: false };

describe("tabRowModel", () => {
  it("prefixes PR tabs and keeps non-PR titles intact", () => {
    expect(tabRowModel(tab({}), none).prefix).toBeNull();
    const m = tabRowModel(tab({ pr_number: 412, label: "Tour rail" }), none);
    expect(m.prefix).toBe("#412");
    expect(m.title).toBe("Tour rail");
  });
  it("does not repeat a number the label already leads with", () => {
    const m = tabRowModel(tab({ pr_number: 412, label: "#412 Tour rail" }), none);
    expect(m.title).toBe("Tour rail");
  });
  it("strips the trailing number from the backend's repo#N label", () => {
    const m = tabRowModel(tab({ pr_number: 412, label: "easy-review#412" }), none);
    expect(m.prefix).toBe("#412");
    expect(m.title).toBe("easy-review");
  });
  it("keeps a label that is only the number", () => {
    expect(tabRowModel(tab({ pr_number: 412, label: "#412" }), none).title).toBe("#412");
  });
  it("ranks running over stale over unseen", () => {
    const stale = tab({ stale: true });
    expect(tabRowModel(stale, { running: true, unseen: true }).mark).toBe("running");
    expect(tabRowModel(stale, { running: false, unseen: true }).mark).toBe("stale");
    expect(tabRowModel(tab({}), { running: false, unseen: true }).mark).toBe("unseen");
    expect(tabRowModel(tab({}), none).mark).toBeNull();
  });
  it("never shows unseen on the active tab", () => {
    const m = tabRowModel(tab({ is_active: true }), { running: false, unseen: true });
    expect(m.mark).toBeNull();
  });
});

describe("arenaRunsAcrossTabs", () => {
  const run = (id: string, branch_ref: string): ArenaRunSummary => ({
    id,
    branch_ref,
    status: "running",
    created_at: "2026-10-01T00:00:00Z",
    reviewer_count: 2,
    finding_count: 0,
  });
  it("adds runs from other tabs and counts a shared run once", () => {
    const here = run("a", "main");
    const out = arenaRunsAcrossTabs([here], [run("a", "main"), run("b", "feat/x")]);
    expect(out.map((r) => r.id)).toEqual(["a", "b"]);
    expect(out[0]).toBe(here);
  });
});
