import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { TabSummary } from "$lib/pageTypes";
import { ResourceStore } from "./resourceStore";
import { WorktreesAdapter, worktreesFromResource } from "./worktreesAdapter";

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

function adapter() {
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
    store,
    activeTab: () => active,
  });
  const activate = (root: string) => {
    active = { repo_root: root };
    worktrees.follow();
  };
  return { activate, fetched, store, worktrees };
}

describe("WorktreesAdapter", () => {
  it("shows the active tab's root and marks the worktree at that root current", async () => {
    const { activate, fetched, worktrees } = adapter();
    // Until the key is held there are none.
    expect(worktrees.worktrees).toEqual([]);
    activate("/a");
    await settle();
    expect(worktrees.worktrees.map((w) => [w.path, w.is_current, w.pr_number, w.is_merged])).toEqual([
      ["/a", true, null, false],
      ["/a-pr", false, 7, true],
    ]);
    expect(fetched).toEqual(["/a"]);
  });

  it("releases the previous root when the active tab moves", async () => {
    const { activate, fetched, store, worktrees } = adapter();
    activate("/a");
    await settle();
    activate("/b");
    await settle();
    expect(store.get({ kind: "worktrees", root: "/a" })).toBeUndefined();
    expect(worktrees.worktrees.filter((w) => w.is_current).map((w) => w.path)).toEqual(["/b"]);
    expect(fetched).toEqual(["/a", "/b"]);
  });

  it("lists none for a tab with no checkout, a remote PR's", async () => {
    const { activate, fetched, store, worktrees } = adapter();
    activate("/a");
    await settle();
    activate("");
    expect(store.get({ kind: "worktrees", root: "/a" })).toBeUndefined();
    expect(worktrees.worktrees).toEqual([]);
    expect(fetched).toEqual(["/a"]);
  });

  it("derives is_current against the root it was given", () => {
    const rows = worktreesFromResource(wire("/a") as never, "/a-pr");
    expect(rows.map((w) => w.is_current)).toEqual([false, true]);
  });
});
