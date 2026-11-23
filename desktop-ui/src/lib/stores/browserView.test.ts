import { describe, expect, it } from "bun:test";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import { BrowserViewAdapter, NO_BROWSER } from "./browserView";

const split = { url: "http://localhost:5173/", layout: "split" as const, split_ratio: 0.6, annotate_mode: true, show_tooltips: false };

function store() {
  const shown: string[] = [];
  const released: string[] = [];
  const id = (key: ResourceKey) => JSON.stringify(key);
  return {
    shown,
    released,
    get: (key: ResourceKey) =>
      id(key) === id({ kind: "view", tab: 12 })
        ? { key, version: 2, value: { kind: "view" as const, browser: split } }
        : undefined,
    show: (key: ResourceKey) => {
      shown.push(id(key));
      return () => released.push(id(key));
    },
  };
}

describe("BrowserViewAdapter", () => {
  it("reads the active tab's view and follows the active key", () => {
    const s = store();
    let active = 12;
    const queued: (() => void)[] = [];
    const view = new BrowserViewAdapter({
      store: s,
      snapshot: () => ({ browser: { ...split, url: "from the snapshot" } }),
      activeKey: () => active,
      later: (run) => queued.push(run),
    });
    expect(view.read()).toEqual(split);
    active = 11;
    expect(view.read()).toEqual(NO_BROWSER);
    for (const run of queued) run();
    expect(s.shown).toEqual(['{"kind":"view","tab":11}']);
    active = 12;
    view.read();
    queued.splice(0).at(-1)?.();
    expect(s.released).toEqual(['{"kind":"view","tab":11}']);
  });
});
