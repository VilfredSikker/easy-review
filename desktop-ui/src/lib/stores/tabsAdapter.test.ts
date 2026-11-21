import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceChanged } from "$lib/protocol/generated/ResourceChanged";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
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
  it("keeps the snapshot's change token while the rows agree", () => {
    const rows = [row(0, "a", true), row(1, "b", false)];
    const view = tabsFromResource(wireTabs, rows);
    expect(view.tabs.map((t) => t.change_token)).toEqual(["a", "b"]);
    const lagging = tabsFromResource(wireTabs, rows.slice(0, 1));
    expect(lagging.tabs.map((t) => t.change_token)).toEqual(["", ""]);
  });

  it("takes each tab's staleness from its diff and PR list, never the snapshot row", () => {
    const rows = [{ ...row(0, "a", true), stale: true }, { ...row(1, "b", false), stale: true }];
    const asked: [number, boolean][] = [];
    const view = tabsFromResource(wireTabs, rows, (wire, isActive) => {
      asked.push([wire.key, isActive]);
      return wire.key === 11;
    });
    expect(view.tabs.map((t) => t.stale)).toEqual([true, false]);
    expect(asked).toEqual([
      [11, false],
      [12, true],
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

  it("matches a remote PR on its remote and number, since its descriptor has no repo root", () => {
    const wire = {
      tabs: [
        wireRow(11, "a"),
        wireRow(12, "gadgets#9", { kind: "remote_pr", repo_root: "", branch: null, pr_number: 9, remote: "acme/gadgets" }),
      ],
      active: 11,
    };
    const remotePr = {
      ...row(1, "gadgets#9", false),
      kind: "remote_pr" as const,
      repo_root: "/Users/x/proj",
      pr_number: 9,
      remote: "acme/gadgets",
      stale: true,
      change_token: "t1",
    };
    const pick = (rows: TabSummary[]) =>
      tabsFromResource(wire, rows).tabs.map((t) => [t.label, t.change_token]);
    expect(pick([row(0, "a", true), remotePr])).toEqual([
      ["a", "a"],
      ["gadgets#9", "t1"],
    ]);
    // Same length, other tabs: another PR number, and a branch in another repo.
    expect(pick([{ ...row(0, "a", true), repo_root: "/r/other" }, { ...remotePr, pr_number: 10 }])).toEqual([
      ["a", ""],
      ["gadgets#9", ""],
    ]);
  });

  it("labels a local PR whose row has no branch with the snapshot's head branch", () => {
    const wire = {
      tabs: [
        wireRow(11, "proj#7", { kind: "local_pr", repo_root: "/r/proj", branch: null, pr_number: 7 }),
        wireRow(12, "b"),
      ],
      active: 11,
    };
    const rows = [
      { ...row(0, "feature-x", true), kind: "local_branch" as const, repo_root: "/r/proj", branch: "feature-x", pr_number: 7 },
      { ...row(1, "b", false), kind: "local_branch" as const, label: "older-name" },
    ];
    expect(tabsFromResource(wire, rows).tabs.map((t) => t.label)).toEqual(["feature-x", "b"]);
    expect(tabsFromResource(wire, rows.slice(0, 1)).tabs[0]?.label).toBe("proj#7");
  });

  it("takes the first tab as active when the resource names none", () => {
    expect(tabsFromResource({ ...wireTabs, active: null }, snapshot.tabs).active).toBe(0);
  });
});

describe("TabsAdapter.route", () => {
  /** The Ack names two keys, so a route that waits for only one of them still resolves early. */
  const ACK_CHANGED: ResourceChanged[] = [
    { key: { kind: "tabs" }, version: 4 },
    { key: { kind: "view", tab: 12 }, version: 2 },
  ];

  /** A store whose `whenAt` holds each wait until the test releases it, or answers at once with `held`. */
  function sender(held = false) {
    const sent: Command[] = [];
    const waited: [ResourceKey, number][] = [];
    const releases: (() => void)[] = [];
    const store = {
      get: () => ({ key: { kind: "tabs" as const }, version: 3, value: { kind: "tabs" as const, ...wireTabs } }),
      show: () => () => {},
      send: async (command: Command): Promise<Ack> => {
        sent.push(command);
        return { changed: ACK_CHANGED, created: null };
      },
      whenAt: (key: ResourceKey, version: number) => {
        waited.push([key, version]);
        return held ? Promise.resolve() : new Promise<void>((resolve) => releases.push(resolve));
      },
    };
    return { sent, waited, releases, store };
  }

  const hint = { baseRef: "main", headRef: "feature", headOid: "abc123", updatedAt: "2026-01-02T03:04:05Z" };

  /** Every command `wireCommand` maps, the arguments its callers pass, and the wire command it sends. */
  const ROUTED: [string, Record<string, unknown>, Command][] = [
    ["new_tab", {}, { kind: "new_tab" }],
    ["select_tab", { idx: 1 }, { kind: "select_tab", key: 12 }],
    ["close_tab", { idx: 0 }, { kind: "close_tab", key: 11 }],
    ["reorder_tabs", { fromIdx: 1, toIdx: 0 }, { kind: "reorder_tabs", key: 12, toIdx: 0 }],
    [
      "update_tab_browser",
      { splitRatio: 0.6 },
      { kind: "update_tab_browser", layout: null, url: null, navigate: false, annotate: null, tooltips: null, splitRatio: 0.6 },
    ],
    [
      "update_tab_browser",
      { layout: "split", url: "https://example.com", navigate: true, annotate: true, tooltips: false },
      {
        kind: "update_tab_browser",
        layout: "split",
        url: "https://example.com",
        navigate: true,
        annotate: true,
        tooltips: false,
        splitRatio: null,
      },
    ],
    ["cycle_tab_browser_layout", {}, { kind: "cycle_tab_browser_layout" }],
    ["open_pr_review", { projectId: "p", prNumber: 7 }, { kind: "open_pr_review", projectId: "p", prNumber: 7, replace: false }],
    [
      "open_pr_branch",
      { projectId: "p", prNumber: 7, headRef: "h", replace: true },
      { kind: "open_pr_review", projectId: "p", prNumber: 7, replace: true },
    ],
    [
      "open_remote_pr",
      { owner: "acme", repo: "gadgets", number: 9, replace: true },
      { kind: "open_remote_pr", owner: "acme", repo: "gadgets", number: 9, replace: true },
    ],
    [
      "open_pr_url",
      { url: "https://github.com/a/b/pull/1" },
      { kind: "open_pr_url", url: "https://github.com/a/b/pull/1", replace: false },
    ],
    [
      "open_local_branch",
      { projectId: "p", name: "feature" },
      { kind: "open_local_branch", projectId: "p", name: "feature", replace: false },
    ],
    [
      "open_project_branch",
      { projectId: "p", branch: "main", replace: true },
      { kind: "open_project_branch", projectId: "p", branch: "main", replace: true },
    ],
    ["open_worktree", {}, { kind: "open_worktree" }],
    ["set_active_project", { id: "p" }, { kind: "set_active_project", projectId: "p" }],
    ["prefetch_pr_open", { projectId: "p", prNumber: 7, hint }, { kind: "prefetch_pr_open", projectId: "p", prNumber: 7, hint }],
    [
      "prefetch_remote_pr_open",
      { owner: "acme", repo: "gadgets", number: 9 },
      { kind: "prefetch_remote_pr_open", owner: "acme", repo: "gadgets", number: 9 },
    ],
  ];

  it("leaves every command to app.cmd with the flag off", () => {
    const { store } = sender();
    const tabs = new TabsAdapter({ flagOn: false, store, snapshot: () => snapshot });
    for (const [name, args] of ROUTED) expect(tabs.route(name, args)).toBeNull();
  });

  it("covers every command wireCommand maps", () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "tabsAdapter.ts"), "utf8");
    const arms = [...source.matchAll(/case "(\w+)":/g)].map((m) => m[1]);
    expect([...new Set(ROUTED.map(([name]) => name))].sort()).toEqual(arms.sort());
  });

  for (const [name, args, wire] of ROUTED) {
    it(`sends ${name} ${JSON.stringify(args)} through er_send with the flag on, and resolves once the Ack's versions are held`, async () => {
      const { sent, waited, releases, store } = sender();
      const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
      const routed = tabs.route(name, args);
      if (routed === null) throw new Error(`${name} fell back to app.cmd`);
      let settled = false;
      const done = routed.then(() => {
        settled = true;
      });
      await settle();
      expect(sent).toEqual([wire]);
      expect(waited).toEqual(ACK_CHANGED.map(({ key, version }) => [key, version]));
      expect(settled).toBe(false);
      releases[0]?.();
      await settle();
      expect(settled).toBe(false);
      releases[1]?.();
      await done;
      expect(settled).toBe(true);
    });
  }

  it("resolves a prefetch with no hint at once and sends nothing", async () => {
    const { sent, waited, store } = sender(true);
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
    await tabs.route("prefetch_pr_open", { projectId: "p", prNumber: 7 });
    expect(sent).toEqual([]);
    expect(waited).toEqual([]);
    expect(tabs.route("run_ai_review", {})).toBeNull();
  });

  it("rejects a position the resource does not list", async () => {
    const { sent, store } = sender(true);
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
    await expect(tabs.route("select_tab", { idx: 5 }) ?? Promise.resolve()).rejects.toThrow("no tab at position 5");
    expect(sent).toEqual([]);
  });
});

describe("the strip's staleness with the flag on", () => {
  it("reads no snapshot row's stale mark", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const source = readFileSync(join(here, "tabsAdapter.ts"), "utf8");
    const body = source.slice(source.indexOf("export function tabsFromResource"), source.indexOf("type Args"));
    expect(body).not.toMatch(/seen\??\.stale|snapshotRows\[[^\]]*\]\??\.stale|candidate\??\.stale/);
    expect(body).toMatch(/stale: stale\(row, idx === active\)/);
  });
});
