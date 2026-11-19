import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
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
 * The resource's rows, with what the registry does not hold yet taken from
 * the snapshot row at the same position: a Working tab's live branch label,
 * the diff's change token, a local PR's `owner/repo` and the staleness, which
 * stays the snapshot's until P55.2 derives it from `prs(remote)`. The
 * snapshot row counts only while both lists have the same length and the row
 * views the same repo, since one side may not have caught up with a change.
 */
export function tabsFromResource(wire: Tabs, snapshotRows: readonly TabSummary[]): TabsView {
  const at = wire.active === null ? -1 : wire.tabs.findIndex((row) => row.key === wire.active);
  const active = Math.max(at, 0);
  const agrees = wire.tabs.length === snapshotRows.length;
  const tabs = wire.tabs.map((row: TabRow, idx): TabSummary => {
    const seen = agrees && snapshotRows[idx]?.repo_root === row.repo_root ? snapshotRows[idx] : undefined;
    return {
      idx,
      label: row.kind === "working" && seen ? seen.label : row.label,
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

interface TabsAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show">;
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
}
