import { describe, expect, test } from "bun:test";
import { moveHighlight, suggestionFooter, suggestionLabel, visibleSuggestions } from "./filterSuggestions";
import type { FilterSuggestionSnapshot } from "./types";

const all: FilterSuggestionSnapshot[] = [
  { kind: "kind", name: "code", expr: "kind:code", files: 12 },
  { kind: "kind", name: "test", expr: "kind:test", files: 8 },
  { kind: "kind", name: "docs", expr: "kind:docs", files: 2 },
  { kind: "preset", name: "frontend", expr: "*.ts,*.tsx" },
  { kind: "history", name: "src/api", expr: "src/api" },
];

describe("visibleSuggestions", () => {
  test("empty draft lists everything", () => {
    expect(visibleSuggestions(all, "  ")).toEqual(all);
  });

  test("kind: lists every kind and nothing else", () => {
    expect(visibleSuggestions(all, "kind:").map((s) => s.expr)).toEqual([
      "kind:code",
      "kind:test",
      "kind:docs",
    ]);
  });

  test("a partial value narrows the kinds", () => {
    expect(visibleSuggestions(all, "kind:t").map((s) => s.expr)).toEqual(["kind:test"]);
    expect(visibleSuggestions(all, "KIND:D").map((s) => s.expr)).toEqual(["kind:docs"]);
  });

  test("keeps the sign so -kind: offers excludes", () => {
    expect(visibleSuggestions(all, "-kind:te").map((s) => s.expr)).toEqual(["-kind:test"]);
    expect(visibleSuggestions(all, "+kind:c").map((s) => s.expr)).toEqual(["+kind:code"]);
  });

  test("a complete kind is not suggested back", () => {
    expect(visibleSuggestions(all, "kind:test")).toEqual([]);
  });

  test("any other draft hides the list", () => {
    expect(visibleSuggestions(all, "src")).toEqual([]);
    expect(visibleSuggestions(all, "kind:code,")).toEqual([]);
  });
});

describe("moveHighlight", () => {
  test("enters the list from the input", () => {
    expect(moveHighlight(-1, 3, 1)).toBe(0);
    expect(moveHighlight(-1, 3, -1)).toBe(2);
  });

  test("wraps at both ends", () => {
    expect(moveHighlight(2, 3, 1)).toBe(0);
    expect(moveHighlight(0, 3, -1)).toBe(2);
  });

  test("an empty list has nothing to highlight", () => {
    expect(moveHighlight(0, 0, 1)).toBe(-1);
  });
});

describe("suggestionLabel", () => {
  test("tags each kind, calling stored queries RECENT", () => {
    expect(suggestionLabel({ kind: "kind" })).toBe("KIND");
    expect(suggestionLabel({ kind: "preset" })).toBe("PRESET");
    expect(suggestionLabel({ kind: "history" })).toBe("RECENT");
  });
});

describe("suggestionFooter", () => {
  test("shows files shown and reviewed against the whole diff", () => {
    expect(suggestionFooter(3, 1, 9)).toBe("3 files · 1/9 reviewed");
    expect(suggestionFooter(1, 0, 1)).toBe("1 file · 0/1 reviewed");
  });
});
