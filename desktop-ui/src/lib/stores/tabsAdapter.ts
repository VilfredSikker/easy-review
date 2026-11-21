import type { BrowserLayout } from "$lib/protocol/generated/BrowserLayout";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TabKind } from "$lib/protocol/generated/TabKind";
import type { TabRow } from "$lib/protocol/generated/TabRow";
import type { Tabs } from "$lib/protocol/generated/Tabs";
import type { AppSnapshot, TabSummary } from "$lib/types";
import type { ResourceStore } from "./resourceStore";

export const TABS_KEY: ResourceKey = { kind: "tabs" };

/** What the tab strip renders: the rows in strip order and the active row's `idx`. */
export interface TabsView {
  tabs: TabSummary[];
  active: number;
}

type TabsSlice = Pick<AppSnapshot, "tabs" | "active_tab">;

export function tabsFromSnapshot(snapshot: TabsSlice | null): TabsView {
  return { tabs: snapshot?.tabs ?? [], active: snapshot?.active_tab ?? 0 };
}

/** The snapshot names a local PR tab by the branch view it opens. */
function summaryKind(kind: TabKind): TabSummary["kind"] {
  return kind === "local_pr" ? "local_branch" : kind;
}

/**
 * Whether the snapshot row is the same tab as the resource row. A remote PR's
 * descriptor has no repo root while the App's tab takes the process cwd, so a
 * remote PR is matched on its `owner/repo` and number.
 */
function sameTab(row: TabRow, seen: TabSummary): boolean {
  if (row.kind === "remote_pr") {
    return seen.kind === "remote_pr" && seen.remote === row.remote && seen.pr_number === row.pr_number;
  }
  return seen.repo_root === row.repo_root;
}

/**
 * The resource's rows, with what the registry does not hold yet taken from
 * the snapshot row at the same position: a Working tab's live branch label, a
 * local PR's head branch label, the diff's change token, a local PR's
 * `owner/repo` and the staleness, which stays the snapshot's until P56.1
 * derives it from `diff(tab)` and `prs(remote)`. The snapshot row counts only while both lists
 * have the same length and the row is the same tab, since one side may not
 * have caught up with a change.
 */
export function tabsFromResource(wire: Tabs, snapshotRows: readonly TabSummary[]): TabsView {
  const at = wire.active === null ? -1 : wire.tabs.findIndex((row) => row.key === wire.active);
  const active = Math.max(at, 0);
  const agrees = wire.tabs.length === snapshotRows.length;
  const tabs = wire.tabs.map((row: TabRow, idx): TabSummary => {
    const candidate = agrees ? snapshotRows[idx] : undefined;
    const seen = candidate && sameTab(row, candidate) ? candidate : undefined;
    // No descriptor holds a Working tab's live branch or a local PR's head
    // branch, so the registry names the folder for those.
    const branchless = row.kind === "working" || (row.kind === "local_pr" && row.branch === null);
    return {
      idx,
      label: branchless && seen ? seen.label : row.label,
      kind: summaryKind(row.kind),
      branch: row.branch,
      pr_number: row.pr_number,
      remote: row.remote ?? seen?.remote ?? null,
      repo_root: row.repo_root,
      is_active: idx === active,
      change_token: seen?.change_token ?? "",
      stale: seen?.stale ?? false,
    };
  });
  return { tabs, active: tabs[active]?.idx ?? 0 };
}

type Args = Record<string, unknown>;

/**
 * The wire command for a tab command or an open `app.cmd` was asked for,
 * from the arguments its callers pass; undefined for any other command, null
 * for a prefetch with no hint to pin. A position names the tab `keyAt` gives
 * for it, as the strip rendered it.
 */
export function wireCommand(
  command: string,
  a: Args,
  keyAt: (idx: number) => TabKey | null,
): Command | null | undefined {
  return tabCommand(command, a, keyAt) ?? openCommand(command, a);
}

/** The commands on an open tab: the strip's and the review browser's. */
function tabCommand(command: string, a: Args, keyAt: (idx: number) => TabKey | null): Command | undefined {
  const key = (idx: unknown): TabKey => {
    const at = typeof idx === "number" ? keyAt(idx) : null;
    if (at === null) throw new Error(`${command}: no tab at position ${String(idx)}`);
    return at;
  };
  switch (command) {
    case "new_tab":
      return { kind: "new_tab" };
    case "select_tab":
      return { kind: "select_tab", key: key(a.idx) };
    case "close_tab":
      return { kind: "close_tab", key: key(a.idx) };
    case "reorder_tabs":
      return { kind: "reorder_tabs", key: key(a.fromIdx), toIdx: Number(a.toIdx) };
    case "update_tab_browser":
      return {
        kind: "update_tab_browser",
        layout: (a.layout as BrowserLayout | undefined) ?? null,
        url: (a.url as string | undefined) ?? null,
        navigate: a.navigate === true,
        annotate: (a.annotate as boolean | undefined) ?? null,
        tooltips: (a.tooltips as boolean | undefined) ?? null,
        splitRatio: (a.splitRatio as number | undefined) ?? null,
      };
    case "cycle_tab_browser_layout":
      return { kind: "cycle_tab_browser_layout" };
    default:
      return undefined;
  }
}

/** The opens and the open-cache warm-ups. */
function openCommand(command: string, a: Args): Command | null | undefined {
  const replace = a.replace === true;
  switch (command) {
    // A PR branch opens as the PR's review, as the flag-on command does.
    case "open_pr_review":
    case "open_pr_branch":
      return { kind: "open_pr_review", projectId: String(a.projectId), prNumber: Number(a.prNumber), replace };
    case "open_remote_pr":
      return { kind: "open_remote_pr", owner: String(a.owner), repo: String(a.repo), number: Number(a.number), replace };
    case "open_pr_url":
      return { kind: "open_pr_url", url: String(a.url), replace };
    case "open_local_branch":
      return { kind: "open_local_branch", projectId: String(a.projectId), name: String(a.name), replace };
    case "open_project_branch":
      return { kind: "open_project_branch", projectId: String(a.projectId), branch: String(a.branch), replace };
    case "open_worktree":
      return { kind: "open_worktree" };
    case "set_active_project":
      return { kind: "set_active_project", projectId: String(a.id) };
    // The page asks for a new tab; the wire asks whether to replace the active one.
    case "open_inbox_item":
      return { kind: "open_inbox_item", id: String(a.id), replace: a.newTab !== true };
    case "prefetch_inbox_item":
      return { kind: "prefetch_inbox_item", id: String(a.id) };
    case "prefetch_pr_open":
      return prefetchCommand(a);
    case "prefetch_remote_pr_open":
      return { kind: "prefetch_remote_pr_open", owner: String(a.owner), repo: String(a.repo), number: Number(a.number) };
    default:
      return undefined;
  }
}

function prefetchCommand(a: Args): Command | null {
  const hint = a.hint as { baseRef: string; headRef: string; headOid: string; updatedAt: string } | undefined;
  if (!hint) return null;
  const { baseRef, headRef, headOid, updatedAt } = hint;
  return {
    kind: "prefetch_pr_open",
    projectId: String(a.projectId),
    prNumber: Number(a.prNumber),
    hint: { baseRef, headRef, headOid, updatedAt },
  };
}

interface TabsAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
  snapshot: () => TabsSlice | null;
}

/**
 * The open tabs, read from `app.snapshot` with the flag off and from the
 * `tabs` resource with it on, behind one read API.
 */
export class TabsAdapter {
  constructor(private readonly deps: TabsAdapterDeps) {
    if (deps.flagOn) deps.store.show(TABS_KEY);
  }

  read(): TabsView {
    const snapshot = this.deps.snapshot();
    if (!this.deps.flagOn) return tabsFromSnapshot(snapshot);
    const held = this.deps.store.get(TABS_KEY)?.value;
    // Until the first fetch answers there are no rows to show.
    if (held?.kind !== "tabs") return tabsFromSnapshot(snapshot);
    return tabsFromResource(held, snapshot?.tabs ?? []);
  }

  get tabs(): TabSummary[] {
    return this.read().tabs;
  }

  get active(): number {
    return this.read().active;
  }

  /** The active row, as the strip highlights it. */
  get activeTab(): TabSummary | null {
    const view = this.read();
    return view.tabs.find((tab) => tab.is_active) ?? view.tabs[view.active] ?? null;
  }

  /** The active tab's wire key; null with the flag off or before the first fetch. */
  get activeKey(): TabKey | null {
    return this.held()?.active ?? null;
  }

  /** The wire key of the row at `idx` in the resource's order. */
  keyAt(idx: number): TabKey | null {
    return this.held()?.tabs[idx]?.key ?? null;
  }

  /**
   * With the flag on, a tab command or an open goes through `er_send` and
   * resolves once every key its `Ack` names is held at that version; null for
   * any other command, and for every command with the flag off.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    let wire: Command | null | undefined;
    try {
      wire = wireCommand(command, args, (idx) => this.keyAt(idx));
    } catch (e) {
      return Promise.reject(e);
    }
    if (wire === undefined) return null;
    return wire === null ? Promise.resolve() : this.send(wire);
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

  private held(): Tabs | null {
    if (!this.deps.flagOn) return null;
    const held = this.deps.store.get(TABS_KEY)?.value;
    return held?.kind === "tabs" ? held : null;
  }
}
