import type { Command } from "$lib/protocol/generated/Command";
import type { Prs } from "$lib/protocol/generated/Prs";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TabPr } from "$lib/protocol/generated/TabPr";
import type { AppSnapshot, GithubStatusSnapshot, PrSnapshot, StackSnapshot, TabSummary } from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";

export function prKey(tab: TabKey): ResourceKey {
  return { kind: "pr", tab };
}

export function prStatusKey(target: StatusTarget): ResourceKey {
  return { kind: "pr_status", remote: target.remote, number: target.number };
}

/** The PR whose GitHub status a tab's card shows. */
export interface StatusTarget {
  /** `owner/repo`. */
  remote: string;
  number: number;
}

/** One remote's PR list, as `prs(remote)` holds it. */
export interface HeldPrs {
  remote: string;
  prs: Prs;
}

/** Trunk names a stack is based on, as `snapshot.rs`'s `TRUNK_BRANCHES`. */
const TRUNK_BRANCHES = new Set(["main", "master", "develop", "dev"]);

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/**
 * The PR whose status the tab's card shows, as `snapshot.rs`'s
 * `resolve_github_status_key` picks it: a remote PR names it; a local PR
 * keeps its own number and finds its remote in the lists; a branch tab takes
 * the PR whose head is the viewed branch, an open one first. `repoRemote`
 * limits the search to the tab's own repo when it is known.
 */
export function statusTarget(
  tab: Pick<TabSummary, "kind" | "pr_number" | "remote"> | null,
  branch: string | null | undefined,
  repoRemote: string | null,
  held: readonly HeldPrs[],
): StatusTarget | null {
  if (!tab) return null;
  if (tab.kind === "remote_pr") {
    return tab.remote && tab.pr_number != null ? { remote: tab.remote, number: tab.pr_number } : null;
  }
  const restrict = tab.remote ?? repoRemote;
  const lists = held.filter(({ remote }) => restrict === null || same(remote, restrict));
  const own = tab.pr_number;
  if (own != null) {
    const found =
      lists.find(({ prs }) => prs.rows.some((row) => row.number === own)) ??
      lists.find(({ prs }) => prs.rows.some((row) => row.headRef === branch));
    return found ? { remote: found.remote, number: own } : null;
  }
  if (!branch) return null;
  for (const { remote, prs } of lists) {
    const rows = prs.rows.filter((row) => row.headRef === branch);
    const row = rows.find((r) => r.state === "OPEN") ?? rows[0];
    if (row) return { remote, number: row.number };
  }
  return null;
}

/**
 * Whether the open PRs put `branch` in a stack, as `snapshot.rs`'s
 * `branch_in_pr_stack` reads the PR cache: its PR is based on another open
 * PR's branch, or another open PR is based on it.
 */
export function branchInPrStack(branch: string, restrict: string | null, held: readonly HeldPrs[]): boolean {
  if (!branch || TRUNK_BRANCHES.has(branch)) return false;
  return held
    .filter(({ remote }) => restrict === null || same(remote, restrict))
    .some(({ prs }) => {
      const open = prs.rows.filter((row) => row.state === "OPEN");
      const basedOnThis = open.some((row) => row.baseRef === branch);
      const thisOnAPr = open
        .filter((row) => row.headRef === branch)
        .some((own) => !TRUNK_BRANCHES.has(own.baseRef) && open.some((row) => row.headRef === own.baseRef));
      return basedOnThis || thisOnAPr;
    });
}

interface PrAdapterDeps {
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  /** The registry's active tab key and its row. */
  activeKey: () => TabKey | null;
  activeTab: () => TabSummary | null;
  /** The `owner/repo` of the project whose checkout `root` is, when one is listed. */
  repoRemote: (root: string) => string | null;
  /** The PR lists the page holds. */
  heldPrs: () => readonly HeldPrs[];
}

/** The PR card commands `er_send` carries. */
const ROUTED: ReadonlySet<string> = new Set(["refresh_stack", "refresh_github_status"]);

/**
 * The active tab's PR card, stack and GitHub status, from `pr(tab)` and
 * `pr_status(remote, number)`. Until a resource is held the view's field
 * stays.
 */
export class PrAdapter {
  constructor(private readonly deps: PrAdapterDeps) {}

  private held(): TabPr | null {
    const key = this.deps.activeKey();
    if (key === null) return null;
    const value = this.deps.store.get(prKey(key))?.value;
    return value?.kind === "pr" ? value : null;
  }

  private restrict(tab: TabSummary | null): string | null {
    return tab?.remote ?? (tab ? this.deps.repoRemote(tab.repo_root) : null);
  }

  /** The status key the active tab's card reads, for `branch`. */
  target(branch: string | null | undefined): StatusTarget | null {
    const tab = this.deps.activeTab();
    return statusTarget(tab, branch, tab ? this.deps.repoRemote(tab.repo_root) : null, this.deps.heldPrs());
  }

  /** `raw` with the resources' PR fields laid over it, when the page holds them. */
  overlay(raw: AppSnapshot): AppSnapshot {
    const held = this.held();
    const next: AppSnapshot = { ...raw };
    const branch = raw.local_branch ?? raw.branch;
    if (held) {
      next.pr = held.pr as PrSnapshot | null;
      const restrict = this.restrict(this.deps.activeTab());
      next.stack = held.stack
        ? ({ ...held.stack, likely_stacked: branchInPrStack(branch, restrict, this.deps.heldPrs()) } as StackSnapshot)
        : null;
    }
    const target = this.target(branch);
    if (target === null) {
      if (held) next.github = null;
      return next;
    }
    const status = this.deps.store.get(prStatusKey(target))?.value;
    if (status?.kind === "pr_status") next.github = status.status as GithubStatusSnapshot | null;
    return next;
  }

  /** Whether the status the card shows for `branch` is being fetched. */
  statusFetching(branch: string | null | undefined): boolean {
    const target = this.target(branch);
    const status = target === null ? undefined : this.deps.store.get(prStatusKey(target))?.value;
    return status?.kind === "pr_status" && status.fetching;
  }

  route(command: string): Promise<void> | null {
    if (!ROUTED.has(command)) return null;
    const tab = this.deps.activeKey();
    if (tab === null) return null;
    const wire: Command =
      command === "refresh_stack" ? { kind: "refresh_stack", tab } : { kind: "refresh_github_status", tab };
    return this.send(wire);
  }

  private async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
    }
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
  }
}
