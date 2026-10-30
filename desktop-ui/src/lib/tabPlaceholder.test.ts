import { describe, expect, test } from "bun:test";
import { tabPlaceholderRepo, tabPlaceholderTarget } from "./tabPlaceholder";
import type { TabSummary } from "./types";

const tab = (over: Partial<TabSummary>): TabSummary => ({
  idx: 0,
  label: "main",
  kind: "local_branch",
  branch: "main",
  pr_number: null,
  remote: null,
  repo_root: "/Users/me/work/discovery",
  is_active: false,
  change_token: "",
  ...over,
});

describe("tabPlaceholderRepo", () => {
  test("prefers the remote's repo name", () => {
    expect(tabPlaceholderRepo(tab({ remote: "acme/discovery-platform" }))).toBe("discovery-platform");
  });

  test("falls back to the clone's folder", () => {
    expect(tabPlaceholderRepo(tab({}))).toBe("discovery");
    expect(tabPlaceholderRepo(tab({ repo_root: "C:\\code\\app\\" }))).toBe("app");
    expect(tabPlaceholderRepo(tab({ repo_root: "" }))).toBe("");
  });
});

describe("tabPlaceholderTarget", () => {
  const tabs = [tab({ idx: 0, is_active: true }), tab({ idx: 1, label: "#42" })];

  test("names the tab a switch is heading to", () => {
    expect(tabPlaceholderTarget(tabs, 1)?.label).toBe("#42");
  });

  test("nothing to name without a switch, an unknown tab, or the tab already shown", () => {
    expect(tabPlaceholderTarget(tabs, null)).toBeNull();
    expect(tabPlaceholderTarget(tabs, 5)).toBeNull();
    expect(tabPlaceholderTarget(tabs, 0)).toBeNull();
    expect(tabPlaceholderTarget(undefined, 1)).toBeNull();
  });
});
