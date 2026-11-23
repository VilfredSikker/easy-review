import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { TabRow } from "$lib/protocol/generated/TabRow";
import { tabsFromResource } from "./tabsAdapter";

function wireRow(key: number, changeToken: string): TabRow {
  return {
    key,
    kind: "working",
    label: `t${key}`,
    repo_root: `/r/${key}`,
    branch: null,
    pr_number: null,
    remote: null,
    live_branch: null,
    change_token: changeToken,
  };
}

describe("the strip's new-changes dot", () => {
  it("compares the token each tabs resource row carries", () => {
    const strip = (token: string) =>
      tabsFromResource({ tabs: [wireRow(11, token), wireRow(12, "x")], active: 12 }).tabs.map((tab) => [
        tab.idx,
        tab.change_token,
      ]);
    expect(strip("h1")).toEqual([
      [0, "h1"],
      [1, "x"],
    ]);
    expect(strip("h2")[0]).toEqual([0, "h2"]);
  });

  it("marks and checks the strip's rows, which come from the tabs resource", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const source = readFileSync(join(here, "..", "components", "TabStrip.svelte"), "utf8");
    expect(source).toMatch(/tabsProp \?\? tabsResource\.tabs/);
    expect(source).toMatch(/tabSeen\.markSeen\(activeTab\.idx, activeTab\.change_token\)/);
    expect(source).toMatch(/tabSeen\.hasUnseen\(tab\.idx, tab\.change_token\)/);
  });
});
