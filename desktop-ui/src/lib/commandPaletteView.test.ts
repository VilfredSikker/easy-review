import { describe, expect, it } from "bun:test";
import {
  fileDir,
  groupByMenu,
  isRowKey,
  isSingleKey,
  paletteFooterContext,
  paletteGlyph,
  palettePlaceholder,
  recentPrs,
} from "./commandPaletteView";
import type { PrInfo, ProjectSnapshot } from "./types";

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("palettePlaceholder", () => {
  it("invites a letter at the root and names the menu inside one", () => {
    expect(palettePlaceholder(null)).toBe("Press a letter, or / to search");
    expect(palettePlaceholder("AI")).toBe("Search AI… or press a letter");
  });
});

describe("paletteGlyph", () => {
  it("gives AI and its advanced menu the same glyph", () => {
    expect(paletteGlyph("AI")).toBe("✦");
    expect(paletteGlyph("AI · Advanced")).toBe("✦");
    expect(paletteGlyph("PR")).toBe("#");
  });
});

describe("isSingleKey", () => {
  it("styles a single character as a runnable key and a chord as documentation", () => {
    expect(isSingleKey("t")).toBe(true);
    expect(isSingleKey("`")).toBe(true);
    expect(isSingleKey("⌘B")).toBe(false);
    expect(isSingleKey("⌘⇧E")).toBe(false);
  });
});

describe("isRowKey", () => {
  const bound = new Set(["1", ",", "]"]);
  const has = (k: string) => bound.has(k);

  it("runs digits and punctuation bound in the current view", () => {
    expect(isRowKey(key("1"), false, has)).toBe(true);
    expect(isRowKey(key(","), false, has)).toBe(true);
    expect(isRowKey(key("]"), false, has)).toBe(true);
  });

  it("leaves unbound keys, chords and the search field alone", () => {
    expect(isRowKey(key("2"), false, has)).toBe(false);
    expect(isRowKey(key("1", { metaKey: true }), false, has)).toBe(false);
    expect(isRowKey(key("1"), true, has)).toBe(false);
  });

  it("never claims / or space, which focus search and toggle reviewers", () => {
    const all = () => true;
    expect(isRowKey(key("/"), false, all)).toBe(false);
    expect(isRowKey(key(" "), false, all)).toBe(false);
    expect(isRowKey(key("Enter"), false, all)).toBe(false);
  });
});

describe("groupByMenu", () => {
  it("keeps each menu's hits together in first-seen order with one header each", () => {
    const rows = groupByMenu([
      { id: "a1", group: "AI" },
      { id: "x1", group: "AI · Advanced" },
      { id: "a2", group: "AI" },
      { id: "p1", group: "PR" },
    ]);
    expect(rows.map((r) => r.item.id)).toEqual(["a1", "a2", "x1", "p1"]);
    expect(rows.map((r) => r.header)).toEqual(["AI", null, "AI · Advanced", "PR"]);
  });

  it("returns nothing for no hits", () => {
    expect(groupByMenu([])).toEqual([]);
  });
});

describe("paletteFooterContext", () => {
  it("names the repo and PR the palette acts on", () => {
    expect(paletteFooterContext({ repo: "discovery", prNumber: 1756 })).toBe("er · discovery #1756");
    expect(paletteFooterContext({ repo: "discovery", prNumber: null })).toBe("er · discovery");
    expect(paletteFooterContext({ repo: "", prNumber: 4 })).toBe("er");
  });
});

describe("fileDir", () => {
  it("splits the directory off a path", () => {
    expect(fileDir("src/lib/a.ts")).toBe("src/lib");
    expect(fileDir("README.md")).toBe("");
  });
});

describe("recentPrs", () => {
  const pr = (number: number, updated_at: string) => ({ number, title: `PR ${number}`, updated_at }) as PrInfo;
  const project = (id: string, prs: PrInfo[]) => ({ id, name: id, recent_prs: prs }) as ProjectSnapshot;

  it("merges projects newest first and skips the PR on screen", () => {
    const out = recentPrs(
      [
        project("er", [pr(412, "2026-10-01T00:00:00Z"), pr(410, "2026-10-03T00:00:00Z")]),
        project("discovery", [pr(1752, "2026-10-02T00:00:00Z")]),
      ],
      { projectId: "er", prNumber: 410 },
    );
    expect(out.map((r) => `${r.project.id}#${r.pr.number}`)).toEqual(["discovery#1752", "er#412"]);
  });

  it("lists a number once per project and keeps the same number in another project", () => {
    const out = recentPrs(
      [
        project("a", [pr(1, "2026-10-01T00:00:00Z"), pr(1, "2026-10-01T00:00:00Z")]),
        project("b", [pr(1, "2026-10-02T00:00:00Z")]),
      ],
      { projectId: null, prNumber: null },
    );
    expect(out.map((r) => r.project.id)).toEqual(["b", "a"]);
  });

  it("caps the list", () => {
    const many = Array.from({ length: 9 }, (_, i) => pr(i, `2026-10-0${i + 1}T00:00:00Z`));
    expect(recentPrs([project("a", many)], { projectId: null, prNumber: null }, 3)).toHaveLength(3);
  });
});
