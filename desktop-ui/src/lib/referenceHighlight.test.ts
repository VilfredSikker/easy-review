import { describe, expect, it } from "bun:test";
import {
  findIdentifierRanges,
  findMatchRanges,
  identifierAt,
  searchPrefillFromSelection,
  SEARCH_PREFILL_MAX_LEN,
  smartCaseSensitive,
  splitSegmentsByIdentifier,
  type RefSegment,
} from "./referenceHighlight";
import type { RenderSegment } from "./mergeWordDiffWithSyntax";

describe("searchPrefillFromSelection", () => {
  it("returns the trimmed selection", () => {
    expect(searchPrefillFromSelection("  fooBar(baz) ")).toBe("fooBar(baz)");
  });

  it("uses only the first line of a multi-line selection", () => {
    expect(searchPrefillFromSelection("const a = 1;\nconst b = 2;\n")).toBe("const a = 1;");
  });

  it("rejects empty and whitespace-only selections", () => {
    expect(searchPrefillFromSelection("")).toBeNull();
    expect(searchPrefillFromSelection("   \t ")).toBeNull();
    // Multi-line selection whose first line is blank is rejected too —
    // the selection visibly starts on the blank line.
    expect(searchPrefillFromSelection("\nconst b = 2;")).toBeNull();
  });

  it("rejects selections longer than the cap instead of truncating", () => {
    const long = "x".repeat(SEARCH_PREFILL_MAX_LEN + 1);
    expect(searchPrefillFromSelection(long)).toBeNull();
    const exact = "x".repeat(SEARCH_PREFILL_MAX_LEN);
    expect(searchPrefillFromSelection(exact)).toBe(exact);
  });

  it("honors a custom cap", () => {
    expect(searchPrefillFromSelection("abcdef", 5)).toBeNull();
    expect(searchPrefillFromSelection("abcde", 5)).toBe("abcde");
  });
});

describe("identifierAt", () => {
  it("extracts the identifier under the offset", () => {
    expect(identifierAt("let foo = bar;", 5)).toBe("foo");
    expect(identifierAt("let foo = bar;", 4)).toBe("foo");
    expect(identifierAt("let foo = bar;", 10)).toBe("bar");
  });

  it("falls back to the character before a trailing caret", () => {
    // caret right after "foo" (offset 7 = on the space)
    expect(identifierAt("let foo = bar;", 7)).toBe("foo");
    // caret at end of text
    expect(identifierAt("foo", 3)).toBe("foo");
  });

  it("returns null on non-word characters", () => {
    expect(identifierAt("a + b", 2)).toBeNull();
    expect(identifierAt("   ", 1)).toBeNull();
    expect(identifierAt("", 0)).toBeNull();
  });

  it("returns null for out-of-range offsets", () => {
    expect(identifierAt("foo", -1)).toBeNull();
    expect(identifierAt("foo", 4)).toBeNull();
  });

  it("includes _, $ and digits in identifiers", () => {
    expect(identifierAt("my_var$2 = 1", 3)).toBe("my_var$2");
    expect(identifierAt("$state(0)", 1)).toBe("$state");
  });

  it("rejects pure-numeric tokens", () => {
    expect(identifierAt("x = 42;", 5)).toBeNull();
    expect(identifierAt("0xFF", 2)).toBe("0xFF");
  });
});

describe("findIdentifierRanges", () => {
  it("finds whole-word occurrences only", () => {
    // clicking `foo` must not highlight `foobar`
    expect(findIdentifierRanges("foo foobar foo_ foo", "foo")).toEqual([
      [0, 3],
      [16, 19],
    ]);
  });

  it("respects boundaries at string edges and punctuation", () => {
    expect(findIdentifierRanges("foo(foo).foo", "foo")).toEqual([
      [0, 3],
      [4, 7],
      [9, 12],
    ]);
  });

  it("returns empty for no match or empty identifier", () => {
    expect(findIdentifierRanges("abc", "xyz")).toEqual([]);
    expect(findIdentifierRanges("abc", "")).toEqual([]);
  });

  it("does not match inside larger words on either side", () => {
    expect(findIdentifierRanges("afoo foob", "foo")).toEqual([]);
  });
});

describe("smartCaseSensitive", () => {
  it("is case-sensitive when the query contains an uppercase letter", () => {
    expect(smartCaseSensitive("QcWells")).toBe(true);
    expect(smartCaseSensitive("A")).toBe(true);
  });

  it("is case-insensitive for all-lowercase queries", () => {
    expect(smartCaseSensitive("qcwells")).toBe(false);
    expect(smartCaseSensitive("foo_bar.ts")).toBe(false);
    expect(smartCaseSensitive("")).toBe(false);
  });
});

describe("findMatchRanges (substring mode)", () => {
  const substring = { wordBoundary: false, caseSensitive: true };
  const substringCi = { wordBoundary: false, caseSensitive: false };

  it("matches partial words (no boundary requirement)", () => {
    expect(findMatchRanges("foobar foo", "foo", substring)).toEqual([
      [0, 3],
      [7, 10],
    ]);
  });

  it("matches a full path query inside a line", () => {
    const line =
      '  await fetch(`/experiments/${experimentId}/quality-control/wells`);';
    const query = "/quality-control/wells";
    const start = line.indexOf(query);
    expect(findMatchRanges(line, query, substring)).toEqual([
      [start, start + query.length],
    ]);
    // A route-template query with braces is plain text too.
    const tmpl = 'path: "/experiments/{experiment_id}/quality-control/wells"';
    const q2 = "/experiments/{experiment_id}/quality-control/wells";
    const s2 = tmpl.indexOf(q2);
    expect(findMatchRanges(tmpl, q2, substring)).toEqual([[s2, s2 + q2.length]]);
  });

  it("matches case-insensitively with a lowercase query", () => {
    expect(findMatchRanges("ExperimentQcWells", "qcwells", substringCi)).toEqual([
      [10, 17],
    ]);
  });

  it("is case-sensitive when requested (smart-case: uppercase query)", () => {
    expect(findMatchRanges("foo Foo FOO", "Foo", substring)).toEqual([[4, 7]]);
    expect(findMatchRanges("foo bar", "Foo", substring)).toEqual([]);
  });

  it("advances past each match (non-overlapping)", () => {
    expect(findMatchRanges("aaaa", "aa", substring)).toEqual([
      [0, 2],
      [2, 4],
    ]);
  });

  it("returns empty for an empty query", () => {
    expect(findMatchRanges("abc", "", substring)).toEqual([]);
  });

  it("word-boundary + case-insensitive composes", () => {
    expect(
      findMatchRanges("Foo foobar FOO", "foo", { wordBoundary: true, caseSensitive: false }),
    ).toEqual([
      [0, 3],
      [11, 14],
    ]);
  });
});

describe("splitSegmentsByIdentifier", () => {
  const seg = (text: string, color?: string, changed = false): RenderSegment => ({
    text,
    color,
    changed,
  });

  it("returns the same array when nothing matches", () => {
    const segs = [seg("let x = 1;")];
    expect(splitSegmentsByIdentifier(segs, "foo")).toBe(segs);
  });

  it("marks matched slices and preserves attributes", () => {
    const segs = [seg("let ", "#aaa"), seg("foo = foo;", "#bbb", true)];
    const out = splitSegmentsByIdentifier(segs, "foo") as RefSegment[];
    expect(out.map((s) => s.text).join("")).toBe("let foo = foo;");
    expect(out).toEqual([
      { text: "let ", color: "#aaa", changed: false },
      { text: "foo", color: "#bbb", changed: true, ref: true },
      { text: " = ", color: "#bbb", changed: true },
      { text: "foo", color: "#bbb", changed: true, ref: true },
      { text: ";", color: "#bbb", changed: true },
    ]);
  });

  it("highlights a match that spans segment boundaries", () => {
    // syntax/word-diff segmentation can split mid-identifier
    const segs = [seg("fo", "#aaa"), seg("o + 1", "#bbb")];
    const out = splitSegmentsByIdentifier(segs, "foo") as RefSegment[];
    expect(out).toEqual([
      { text: "fo", color: "#aaa", changed: false, ref: true },
      { text: "o", color: "#bbb", changed: false, ref: true },
      { text: " + 1", color: "#bbb", changed: false },
    ]);
  });

  it("does not mark partial-word occurrences", () => {
    const segs = [seg("foobar foo")];
    const out = splitSegmentsByIdentifier(segs, "foo") as RefSegment[];
    expect(out).toEqual([
      { text: "foobar ", changed: false, color: undefined },
      { text: "foo", changed: false, color: undefined, ref: true },
    ]);
  });

  it("marks partial-word occurrences with substring options (Cmd+F mode)", () => {
    const segs = [seg("foobar foo")];
    const out = splitSegmentsByIdentifier(segs, "foo", {
      wordBoundary: false,
      caseSensitive: true,
    }) as RefSegment[];
    expect(out).toEqual([
      { text: "foo", changed: false, color: undefined, ref: true },
      { text: "bar ", changed: false, color: undefined },
      { text: "foo", changed: false, color: undefined, ref: true },
    ]);
  });

  it("marks case-insensitive substring matches across segment boundaries", () => {
    const segs = [seg("Experiment", "#aaa"), seg("QcWells.ts", "#bbb")];
    const out = splitSegmentsByIdentifier(segs, "experimentqc", {
      wordBoundary: false,
      caseSensitive: false,
    }) as RefSegment[];
    expect(out).toEqual([
      { text: "Experiment", color: "#aaa", changed: false, ref: true },
      { text: "Qc", color: "#bbb", changed: false, ref: true },
      { text: "Wells.ts", color: "#bbb", changed: false },
    ]);
  });
});
