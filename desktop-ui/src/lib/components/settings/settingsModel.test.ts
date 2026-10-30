import { describe, expect, it } from "bun:test";
import type { ConfigHubField } from "$lib/types";
import {
  buildSearchIndex,
  fieldMap,
  isNumericCycle,
  matchesQuery,
  optionLabel,
  pickFields,
  sectionsOf,
  stepOption,
  tabMatchCounts,
  unclaimedSections,
} from "./settingsModel";

const general: ConfigHubField[] = [
  { kind: "section", title: "Appearance" },
  { kind: "cycle", key: "display.theme", label: "Theme", description: "", options: ["graphite", "paper"], value: "graphite" },
  { kind: "section", title: "Agent" },
  { kind: "text", key: "agent.command", label: "Command", description: "Agent executable", placeholder: "", value: "claude", strict: true },
  {
    kind: "cycle",
    key: "ai_hub.max_concurrent_reviews",
    label: "Max parallel reviews",
    description: "",
    options: ["1", "2", "3"],
    value: "2",
  },
  { kind: "bool", key: "features.brand_new", label: "Brand new flag", description: "Something added later", value: true },
  { kind: "section", title: "Later section" },
  { kind: "bool", key: "later.flag", label: "Later flag", description: "", value: false },
];

const terminal: ConfigHubField[] = [
  { kind: "section", title: "Display" },
  { kind: "bool", key: "display.wrap_lines", label: "Wrap lines", description: "Wrap long lines", value: false },
];

describe("settings fields", () => {
  it("splits the flat list at section markers", () => {
    expect(sectionsOf(general).map((s) => [s.title, s.fields.length])).toEqual([
      ["Appearance", 1],
      ["Agent", 3],
      ["Later section", 1],
    ]);
  });

  it("picks fields by key in the order asked, skipping missing ones", () => {
    const picked = pickFields(fieldMap(general), ["ai_hub.max_concurrent_reviews", "missing", "agent.command"]);
    expect(picked.map((f) => f.key)).toEqual(["ai_hub.max_concurrent_reviews", "agent.command"]);
  });

  it("leaves on General every field no hand-built layout shows", () => {
    // A new key inside a claimed section, and a whole new section, both survive.
    expect(unclaimedSections(general).map((s) => [s.title, s.fields.map((f) => f.key)])).toEqual([
      ["Agent", ["features.brand_new"]],
      ["Later section", ["later.flag"]],
    ]);
  });
});

describe("steppers", () => {
  it("only numeric option lists become steppers", () => {
    expect(isNumericCycle(["1", "2", "16"])).toBe(true);
    expect(isNumericCycle(["content", "snapshot"])).toBe(false);
    expect(isNumericCycle(["4"])).toBe(false);
  });

  it("steps along the engine's options and stops at either end", () => {
    const opts = ["1", "2", "3"];
    expect(stepOption(opts, "2", 1)).toBe("3");
    expect(stepOption(opts, "3", 1)).toBeNull();
    expect(stepOption(opts, "1", -1)).toBeNull();
    // A value outside the list snaps to the first option rather than sticking.
    expect(stepOption(opts, "9", 1)).toBe("1");
  });
});

describe("settings search", () => {
  it("matches case-insensitively and treats an empty query as a match", () => {
    expect(matchesQuery("", "anything")).toBe(true);
    expect(matchesQuery("WRAP", "Wrap lines")).toBe(true);
    expect(matchesQuery("wrap", null, "Theme")).toBe(false);
  });

  it("counts matches per tab so the nav can point at another tab", () => {
    const counts = tabMatchCounts(buildSearchIndex(general, terminal), "wrap");
    expect(counts.terminal).toBe(2);
    expect(counts.general).toBe(0);
    expect(tabMatchCounts(buildSearchIndex(general, terminal), "parallel").ai).toBe(1);
    expect(tabMatchCounts(buildSearchIndex(general, terminal), "brand new").general).toBe(1);
    expect(tabMatchCounts(buildSearchIndex(general, terminal), "").ai).toBe(0);
  });
});

describe("option labels", () => {
  it("title-cases engine option ids", () => {
    expect(optionLabel("new-and-push")).toBe("New And Push");
    expect(optionLabel("contrast-dark")).toBe("Contrast Dark");
    expect(optionLabel("xhigh")).toBe("XHigh");
  });
});
