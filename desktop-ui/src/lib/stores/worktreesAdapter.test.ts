import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { TabSummary, WorktreeSnapshot } from "$lib/types";
import { ResourceStore } from "./resourceStore";
import { WorktreesAdapter, worktreesFromResource } from "./worktreesAdapter";

const snapshotRows: WorktreeSnapshot[] = [
  { path: "/old", branch: "main", is_current: true, is_pr: false, pr_number: null, is_merged: false, remote: null },
];

function wire(root: string) {
  return {
    kind: "worktrees",
    worktrees: [
      { path: root, branch: "main", isPr: false, prNumber: null, isMerged: false, remote: "acme/a" },
      { path: `${root}-pr`, branch: "pr-7", isPr: true, prNumber: 7, isMerged: true, remote: "acme/a" },
    ],
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

function adapter(flagOn: boolean) {
  const fetched: string[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    if (command !== "er_fetch") throw new Error(`unexpected ${command}`);
    const keys = args.keys as { kind: string; root: string }[];
    return keys.map((key) => {
      fetched.push(key.root);
      return { key, version: 1, value: wire(key.root) };
    }) as T;
  };
  const store = new ResourceStore(resourceClient(run, async () => () => {}), 60_000);
  let active: Pick<TabSummary, "repo_root"> | null = { repo_root: "/a" };
  const worktrees = new WorktreesAdapter({
    flagOn,
    store,
    snapshot: () => ({ worktrees: snapshotRows }),
    activeTab: () => active,
  });
  const activate = (root: string) => {
    active = { repo_root: root };
    worktrees.follow();
  };
  return { activate, fetched, store, worktrees };
}

describe("WorktreesAdapter", () => {
  it("reads the snapshot's rows with the flag off and fetches nothing", async () => {
    const { activate, fetched, worktrees } = adapter(false);
    activate("/a");
    await settle();
    expect(worktrees.worktrees).toBe(snapshotRows);
    expect(fetched).toEqual([]);
  });

  it("shows the active tab's root and marks the worktree at that root current", async () => {
    const { activate, fetched, worktrees } = adapter(true);
    // Until the key is held the snapshot's rows stand in.
    expect(worktrees.worktrees).toBe(snapshotRows);
    activate("/a");
    await settle();
    expect(worktrees.worktrees.map((w) => [w.path, w.is_current, w.pr_number, w.is_merged])).toEqual([
      ["/a", true, null, false],
      ["/a-pr", false, 7, true],
    ]);
    expect(fetched).toEqual(["/a"]);
  });

  it("releases the previous root when the active tab moves", async () => {
    const { activate, fetched, store, worktrees } = adapter(true);
    activate("/a");
    await settle();
    activate("/b");
    await settle();
    expect(store.get({ kind: "worktrees", root: "/a" })).toBeUndefined();
    expect(worktrees.worktrees.filter((w) => w.is_current).map((w) => w.path)).toEqual(["/b"]);
    expect(fetched).toEqual(["/a", "/b"]);
  });

  it("derives is_current against the root it was given", () => {
    const rows = worktreesFromResource(wire("/a") as never, "/a-pr");
    expect(rows.map((w) => w.is_current)).toEqual([false, true]);
  });
});
