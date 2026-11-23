import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Worktrees } from "$lib/protocol/generated/Worktrees";
import type { AppSnapshot, TabSummary, WorktreeSnapshot } from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";

export function worktreesKey(root: string): ResourceKey {
  return { kind: "worktrees", root };
}

/**
 * The rows with `is_current` derived as the snapshot did: the worktree at
 * the root the tab shows (`build_worktrees` compares each path with it).
 */
export function worktreesFromResource(wire: Worktrees, root: string): WorktreeSnapshot[] {
  return wire.worktrees.map((w) => ({
    path: w.path,
    branch: w.branch,
    is_current: w.path === root,
    is_pr: w.isPr,
    pr_number: w.prNumber,
    is_merged: w.isMerged,
    remote: w.remote,
  }));
}

interface WorktreesAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show">;
  snapshot: () => Pick<AppSnapshot, "worktrees"> | null;
  /** The active row of the tab strip. */
  activeTab: () => Pick<TabSummary, "repo_root"> | null;
}

/**
 * The active tab's worktrees, read from `app.snapshot` with the flag off and
 * from `worktrees(root)` with it on, once that key is held.
 */
export class WorktreesAdapter {
  private shown: { root: string; release: () => void } | null = null;

  constructor(private readonly deps: WorktreesAdapterDeps) {}

  /** Shows the worktrees of the active tab's root, releasing the previous root's. */
  follow(): void {
    if (!this.deps.flagOn) return;
    const root = this.root();
    if (this.shown?.root === root) return;
    this.shown?.release();
    this.shown = root ? { root, release: this.deps.store.show(worktreesKey(root)) } : null;
  }

  /** A tab with no checkout, a remote PR's, has none. */
  get worktrees(): WorktreeSnapshot[] {
    const rows = this.deps.snapshot()?.worktrees ?? [];
    if (!this.deps.flagOn) return rows;
    const root = this.root();
    if (!root) return [];
    const held = this.deps.store.get(worktreesKey(root))?.value;
    return held?.kind === "worktrees" ? worktreesFromResource(held, root) : rows;
  }

  private root(): string {
    return this.deps.activeTab()?.repo_root ?? "";
  }
}
