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

describe("TabsAdapter.route", () => {
  function sender() {
    const sent: unknown[] = [];
    const waited: unknown[] = [];
    const store = {
      get: () => ({ key: { kind: "tabs" as const }, version: 3, value: { kind: "tabs" as const, ...wireTabs } }),
      show: () => () => {},
      send: async (command: unknown) => {
        sent.push(command);
        return { changed: [{ key: { kind: "tabs" as const }, version: 4 }], created: null };
      },
      whenAt: async (key: unknown, version: number) => {
        waited.push([key, version]);
      },
    };
    return { sent, waited, store };
  }

  it("leaves every command to app.cmd with the flag off", () => {
    const { store } = sender();
    const tabs = new TabsAdapter({ flagOn: false, store, snapshot: () => snapshot });
    expect(tabs.route("select_tab", { idx: 1 })).toBeNull();
    expect(tabs.route("open_worktree", {})).toBeNull();
  });

  it("sends the tab commands and the opens by key through er_send with the flag on, and waits for the Ack", async () => {
    const { sent, waited, store } = sender();
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
    await tabs.route("select_tab", { idx: 1 });
    await tabs.route("close_tab", { idx: 0 });
    await tabs.route("reorder_tabs", { fromIdx: 1, toIdx: 0 });
    await tabs.route("open_pr_branch", { projectId: "p", prNumber: 7, headRef: "h", replace: true });
    await tabs.route("open_pr_url", { url: "https://github.com/a/b/pull/1" });
    await tabs.route("update_tab_browser", { splitRatio: 0.6 });
    await tabs.route("prefetch_pr_open", { projectId: "p", prNumber: 7 });
    expect(sent).toEqual([
      { kind: "select_tab", key: 12 },
      { kind: "close_tab", key: 11 },
      { kind: "reorder_tabs", key: 12, toIdx: 0 },
      { kind: "open_pr_review", projectId: "p", prNumber: 7, replace: true },
      { kind: "open_pr_url", url: "https://github.com/a/b/pull/1", replace: false },
      {
        kind: "update_tab_browser",
        layout: null,
        url: null,
        navigate: false,
        annotate: null,
        tooltips: null,
        splitRatio: 0.6,
      },
    ]);
    expect(waited).toHaveLength(6);
    expect(tabs.route("run_ai_review", {})).toBeNull();
  });

  it("rejects a position the resource does not list", async () => {
    const { sent, store } = sender();
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
    await expect(tabs.route("select_tab", { idx: 5 }) ?? Promise.resolve()).rejects.toThrow("no tab at position 5");
    expect(sent).toEqual([]);
  });
});
