import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabSummary } from "$lib/pageTypes";
import { JOBS } from "./annotationsAdapter";
import { PrAdapter, prStatusKey } from "./prAdapter";
import { prListRefreshing } from "./prsAdapter";

const lib = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path: string) => readFileSync(join(lib, path), "utf8");

/** A store holding `values` by key. */
function store(values: [ResourceKey, Resource][]) {
  return {
    get: (key: ResourceKey) => {
      const found = values.find(([k]) => JSON.stringify(k) === JSON.stringify(key));
      return (found ? { key, version: 1, value: found[1] } : undefined) as never;
    },
    send: async () => ({ changed: [], created: null }),
    whenAt: async () => {},
  };
}

const JOBS_VALUE = (refreshing: boolean): Resource => ({
  kind: "jobs",
  ai_replies: [],
  arena_runs: [],
  review_flows: [],
  tasks: [],
  triage: [],
  pr_list_refresh: refreshing,
});

describe("the loading marks the page shows", () => {
  it("reads the PR-list refresh from jobs", () => {
    expect(prListRefreshing(store([[JOBS, JOBS_VALUE(true)]]))).toBe(true);
    expect(prListRefreshing(store([[JOBS, JOBS_VALUE(false)]]))).toBe(false);
    expect(prListRefreshing(store([]))).toBe(false);
  });

  it("reads a status fetch from the pr_status the card shows", () => {
    const target = { remote: "acme/widgets", number: 9 };
    const tab: TabSummary = {
      idx: 0,
      label: "",
      kind: "remote_pr",
      branch: null,
      pr_number: 9,
      remote: "acme/widgets",
      repo_root: "",
      is_active: true,
      change_token: "",
    };
    const adapter = (fetching: boolean) =>
      new PrAdapter({
        store: store([[prStatusKey(target), { kind: "pr_status", status: null, fetching }]]),
        activeKey: () => 3,
        activeTab: () => tab,
        repoRemote: () => null,
        heldPrs: () => [],
      });
    expect(adapter(true).statusFetching("feat")).toBe(true);
    expect(adapter(false).statusFetching("feat")).toBe(false);
  });

  it("the sidebar and the branch card read their resource, and nothing reads bg_loading", () => {
    expect(read("components/LeftSidebar.svelte")).toMatch(/loadingPrList = \$derived\(prListRefreshing\(resources\)\)/);
    expect(read("components/BranchCard.svelte")).toMatch(/manualRefreshing \|\| prResource\.statusFetching\(/);
    for (const path of [
      "components/LeftSidebar.svelte",
      "components/BranchCard.svelte",
      "components/CommentsCard.svelte",
      "components/rpanel/CommentsTab.svelte",
      "components/FlatDiffView.svelte",
      "stores/pageView.svelte.ts",
      "pageTypes.ts",
    ]) {
      expect(read(path)).not.toMatch(/bg_loading/);
    }
  });
});
