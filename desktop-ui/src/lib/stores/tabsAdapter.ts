import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
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

/**
 * The resource's order and active tab over the snapshot's rows, matched by
 * position: the resource names tabs by key only, and the snapshot's rows
 * follow the same registry order. Null while the two lists differ in length,
 * since one of them has not caught up with a change yet.
 */
export function tabsFromResource(wire: Tabs, rows: readonly TabSummary[]): TabsView | null {
  if (wire.tabs.length !== rows.length) return null;
  const at = wire.active === null ? -1 : wire.tabs.indexOf(wire.active);
  const active = Math.max(at, 0);
  return {
    tabs: rows.map((row, i) => ({ ...row, is_active: i === active })),
    active: rows[active]?.idx ?? 0,
  };
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
  private lastAgreed: TabsView | null = null;

  constructor(private readonly deps: TabsAdapterDeps) {
    if (deps.flagOn) deps.store.show(TABS_KEY);
  }

  read(): TabsView {
    const snapshot = this.deps.snapshot();
    if (!this.deps.flagOn) return tabsFromSnapshot(snapshot);
    const held = this.deps.store.get(TABS_KEY)?.value;
    // Until the first fetch answers there is nothing to order the rows by.
    if (held?.kind !== "tabs") return this.lastAgreed ?? tabsFromSnapshot(snapshot);
    const view = tabsFromResource(held, snapshot?.tabs ?? []);
    if (view) this.lastAgreed = view;
    return view ?? this.lastAgreed ?? tabsFromSnapshot(snapshot);
  }

  get tabs(): TabSummary[] {
    return this.read().tabs;
  }

  get active(): number {
    return this.read().active;
  }
}
