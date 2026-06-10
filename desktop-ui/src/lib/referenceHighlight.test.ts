import { describe, expect, it } from "bun:test";
import {
  findIdentifierRanges,
  identifierAt,
  splitSegmentsByIdentifier,
  type RefSegment,
} from "./referenceHighlight";
import type { RenderSegment } from "./mergeWordDiffWithSyntax";

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
});
