import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
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

/** The snapshot calls the first tab active; the backend's registry says the second. */
const snapshot = { tabs: [row(0, "a", true), row(1, "b", false)], active_tab: 0 };

function backend() {
  const calls: string[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push(command);
    const keys = (args.keys ?? []) as unknown[];
    if (command === "er_fetch") {
      return keys.map((key) => ({ key, version: 3, value: { kind: "tabs", tabs: [11, 12], active: 12 } })) as T;
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

  it("reads the tabs resource through er_fetch with the flag on", async () => {
    const { calls, store } = backend();
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => snapshot });
    await settle();
    expect(calls).toEqual(["er_fetch"]);
    expect(tabs.active).toBe(1);
    expect(tabs.tabs.map((t) => [t.label, t.is_active])).toEqual([
      ["a", false],
      ["b", true],
    ]);
  });

  it("keeps the last rows that agreed while the snapshot lags the resource", async () => {
    const { store } = backend();
    let current = snapshot;
    const tabs = new TabsAdapter({ flagOn: true, store, snapshot: () => current });
    await settle();
    const agreed = tabs.tabs;
    current = { tabs: [row(0, "a", true)], active_tab: 0 };
    expect(tabs.tabs).toEqual(agreed);
  });
});

describe("tabsFromResource", () => {
  it("answers null when the lists differ in length", () => {
    expect(tabsFromResource({ tabs: [1, 2, 3], active: 1 }, snapshot.tabs)).toBeNull();
  });

  it("takes the first tab as active when the resource names none", () => {
    expect(tabsFromResource({ tabs: [1, 2], active: null }, snapshot.tabs)?.active).toBe(0);
  });
});
