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
import { ResourceStore } from "./resourceStore";
import { TabsAdapter, tabsFromResource } from "./tabsAdapter";

function wireRow(key: number, label: string, overrides: Partial<TabRow> = {}): TabRow {
  return {
    key,
    kind: "local_branch",
    label,
    repo_root: `/r/${label}`,
    branch: label,
    pr_number: null,
    remote: null,
    live_branch: null,
    change_token: "",
    ...overrides,
  };
}

/** The backend's registry calls the second tab active. */
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
  it("reads the rows of the tabs resource through er_fetch", async () => {
    const { calls, store } = backend({
      kind: "tabs",
      tabs: [wireRow(11, "x"), wireRow(12, "y", { kind: "remote_pr", branch: null, pr_number: 9, remote: "acme/y" })],
      active: 12,
    });
    const tabs = new TabsAdapter({ store });
    await settle();
    expect(calls).toEqual(["er_fetch"]);
    expect(tabs.active).toBe(1);
    expect(tabs.tabs.map((t) => [t.idx, t.label, t.kind, t.is_active, t.pr_number, t.remote])).toEqual([
      [0, "x", "local_branch", false, null, null],
      [1, "y", "remote_pr", true, 9, "acme/y"],
    ]);
  });

  it("asks nothing but er_fetch for its rows", () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "tabsAdapter.ts"), "utf8");
    expect(source).not.toMatch(/legacySnapshot|snapshotRows|rows: \(\) =>/);
  });
});

describe("tabsFromResource", () => {
  it("labels a Working tab with the live branch its row carries", () => {
    const wire = { tabs: [wireRow(11, "main", { kind: "working", branch: null, live_branch: "main" })], active: 11 };
    const [tab] = tabsFromResource(wire).tabs;
    expect([tab?.label, tab?.kind, tab?.live_branch]).toEqual(["main", "working", "main"]);
  });

  it("labels a local PR whose row has no branch with the head view its row names", () => {
    const wire = {
      tabs: [wireRow(11, "pr/7", { kind: "local_pr", repo_root: "/r/proj", branch: null, pr_number: 7 })],
      active: 11,
    };
    const [tab] = tabsFromResource(wire).tabs;
    expect([tab?.label, tab?.branch, tab?.kind]).toEqual(["pr/7", null, "local_branch"]);
  });

  it("carries each row's change token", () => {
    const wire = { tabs: [wireRow(11, "a", { change_token: "h1" }), wireRow(12, "b", { change_token: "h2" })], active: 11 };
    expect(tabsFromResource(wire).tabs.map((t) => t.change_token)).toEqual(["h1", "h2"]);
  });

  it("gives a local PR its row's owner/repo, which its stale mark reads", () => {
    const wire = { tabs: [wireRow(11, "b", { kind: "local_pr", pr_number: 7, remote: "acme/b" })], active: 11 };
    const asked: (string | null)[] = [];
    const view = tabsFromResource(wire, (tab) => {
      asked.push(tab.remote);
      return false;
    });
    expect(view.tabs.map((t) => t.remote)).toEqual(["acme/b"]);
    expect(asked).toEqual(["acme/b"]);
  });

  it("takes each tab's staleness from its diff and PR list", () => {
    const asked: [number, boolean][] = [];
    const view = tabsFromResource(wireTabs, (wire, isActive) => {
      asked.push([wire.key, isActive]);
      return wire.key === 11;
    });
    expect(view.tabs.map((t) => t.stale)).toEqual([true, false]);
    expect(asked).toEqual([
      [11, false],
      [12, true],
    ]);
  });

  it("takes the first tab as active when the resource names none", () => {
    expect(tabsFromResource({ ...wireTabs, active: null }).active).toBe(0);
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
    // The Inbox asks for a new tab; the wire asks whether to replace the active one.
    ["open_inbox_item", { id: "i-1", newTab: false }, { kind: "open_inbox_item", id: "i-1", replace: true }],
    ["open_inbox_item", { id: "i-1", newTab: true }, { kind: "open_inbox_item", id: "i-1", replace: false }],
    ["prefetch_inbox_item", { id: "i-1" }, { kind: "prefetch_inbox_item", id: "i-1" }],
  ];

  it("covers every command wireCommand maps", () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "tabsAdapter.ts"), "utf8");
    const arms = [...source.matchAll(/case "(\w+)":/g)].map((m) => m[1]);
    expect([...new Set(ROUTED.map(([name]) => name))].sort()).toEqual(arms.sort());
  });

  for (const [name, args, wire] of ROUTED) {
    it(`sends ${name} ${JSON.stringify(args)} through er_send, and resolves once the Ack's versions are held`, async () => {
      const { sent, waited, releases, store } = sender();
      const tabs = new TabsAdapter({ store });
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
    const { sent, waited, store } = sender();
    const tabs = new TabsAdapter({ store });
    await tabs.route("prefetch_pr_open", { projectId: "p", prNumber: 7 });
    expect(sent).toEqual([]);
    expect(waited).toEqual([]);
    expect(tabs.route("run_ai_review", {})).toBeNull();
  });

  it("rejects a position the resource does not list", async () => {
    const { sent, store } = sender();
    const tabs = new TabsAdapter({ store });
    await expect(tabs.route("select_tab", { idx: 5 }) ?? Promise.resolve()).rejects.toThrow("no tab at position 5");
    expect(sent).toEqual([]);
  });
});

describe("the strip's staleness", () => {
  it("reads no snapshot row's stale mark", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const source = readFileSync(join(here, "tabsAdapter.ts"), "utf8");
    const body = source.slice(source.indexOf("export function tabsFromResource"), source.indexOf("type Args"));
    expect(body).not.toMatch(/seen\??\.stale|snapshotRows\[[^\]]*\]\??\.stale|candidate\??\.stale/);
    expect(body).toMatch(/stale: stale\(row, idx === active\)/);
  });
});
