import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { TabRow } from "$lib/protocol/generated/TabRow";
import type { TabSummary } from "$lib/types";
import { ResourceStore } from "./resourceStore";
import { TabsAdapter, tabsFromResource } from "./tabsAdapter";

function row(idx: number, label: string, isActive: boolean): TabSummary {
  return {
    idx,
    label,
    kind: "working",
    branch: null,
    pr_number: null,
    remote: null,
    repo_root: `/r/${label}`,
    is_active: isActive,
    change_token: label,
  };
}

function wireRow(key: number, label: string, overrides: Partial<TabRow> = {}): TabRow {
  return {
    key,
    kind: "local_branch",
    label,
    repo_root: `/r/${label}`,
    branch: label,
    pr_number: null,
    remote: null,
    ...overrides,
  };
}

/** The snapshot calls the first tab active; the backend's registry says the second. */
const snapshot = { tabs: [row(0, "a", true), row(1, "b", false)], active_tab: 0 };
const wireTabs = { tabs: [wireRow(11, "a"), wireRow(12, "b")], active: 12 };

function backend(value: unknown = { kind: "tabs", ...wireTabs }) {
  const calls: string[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push(command);
    const keys = (args.keys ?? []) as unknown[];
    if (command === "er_fetch") {
      return keys.map((key) => ({ key, version: 3, value })) as T;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  const client = resourceClient(run, async () => () => {});
  return { calls, store: new ResourceStore(client, 60_000) };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("TabsAdapter", () => {
  it("reads app.snapshot with the flag off and never fetches", async () => {
    const { calls, store } = backend();
    const tabs = new TabsAdapter({ flagOn: false, store, snapshot: () => snapshot });
    await settle();
    expect(tabs.active).toBe(0);
    expect(tabs.tabs.map((t) => t.label)).toEqual(["a", "b"]);
    expect(calls).toEqual([]);
  });

  it("reads the rows of the tabs resource through er_fetch with the flag on", async () => {
    const { calls, store } = backend({
      kind: "tabs",
      tabs: [wireRow(11, "x"), wireRow(12, "y", { kind: "remote_pr", branch: null, pr_number: 9, remote: "acme/y" })],
      active: 12,
    });
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => null });
    await settle();
    expect(calls).toEqual(["er_fetch"]);
    expect(tabs.active).toBe(1);
    expect(tabs.tabs.map((t) => [t.idx, t.label, t.kind, t.is_active, t.pr_number, t.remote])).toEqual([
      [0, "x", "local_branch", false, null, null],
      [1, "y", "remote_pr", true, 9, "acme/y"],
    ]);
  });

  it("shows the resource's rows while the snapshot lags it", async () => {
    const { store } = backend();
    let current = snapshot;
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => current });
    await settle();
    current = { tabs: [row(0, "a", true)], active_tab: 0 };
    expect(tabs.tabs.map((t) => [t.label, t.is_active])).toEqual([
      ["a", false],
      ["b", true],
    ]);
  });
});

describe("tabsFromResource", () => {
  it("keeps the snapshot's staleness and change token while the rows agree", () => {
    const rows = [{ ...row(0, "a", true), stale: true }, row(1, "b", false)];
    const view = tabsFromResource(wireTabs, rows);
    expect(view.tabs.map((t) => [t.stale, t.change_token])).toEqual([
      [true, "a"],
      [false, "b"],
    ]);
    const lagging = tabsFromResource(wireTabs, rows.slice(0, 1));
    expect(lagging.tabs.map((t) => [t.stale, t.change_token])).toEqual([
      [false, ""],
      [false, ""],
    ]);
  });

  it("labels a Working tab with the snapshot's live branch and a local PR with its remote", () => {
    const wire = {
      tabs: [
        wireRow(11, "a", { kind: "working", branch: null }),
        wireRow(12, "b", { kind: "local_pr", pr_number: 7 }),
      ],
      active: 11,
    };
    const rows = [
      { ...row(0, "main", true), repo_root: "/r/a" },
      { ...row(1, "b", false), kind: "local_branch" as const, remote: "acme/b" },
    ];
    expect(tabsFromResource(wire, rows).tabs.map((t) => [t.label, t.kind, t.remote])).toEqual([
      ["main", "working", null],
      ["b", "local_branch", "acme/b"],
    ]);
    expect(tabsFromResource(wire, []).tabs[0]?.label).toBe("a");
  });

  it("takes the first tab as active when the resource names none", () => {
    expect(tabsFromResource({ ...wireTabs, active: null }, snapshot.tabs).active).toBe(0);
  });
});
