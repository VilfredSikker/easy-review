import { describe, expect, it } from "bun:test";
import { mergeWordDiffWithSyntax } from "./mergeWordDiffWithSyntax";

describe("mergeWordDiffWithSyntax", () => {
  it("returns word spans unchanged when syntax spans are empty", () => {
    const word = [{ text: "foo", changed: true }];
    expect(mergeWordDiffWithSyntax(word, [])).toEqual([{ text: "foo", changed: true }]);
  });

  it("returns word spans unchanged when syntax spans are absent", () => {
    const word = [
      { text: "const ", changed: false },
      { text: "foo", changed: true },
    ];
    expect(mergeWordDiffWithSyntax(word, undefined)).toEqual([
      { text: "const ", changed: false },
      { text: "foo", changed: true },
    ]);
  });

  it("splits syntax colors within a changed word-diff region", () => {
    const word = [
      { text: "const ", changed: false },
      { text: "foo", changed: true },
    ];
    const syntax = [
      { text: "const ", color: "#c678dd" },
      { text: "foo", color: "#e06c75" },
    ];
    const merged = mergeWordDiffWithSyntax(word, syntax);
    expect(merged).toEqual([
      { text: "const ", color: "#c678dd", changed: false },
      { text: "foo", color: "#e06c75", changed: true },
    ]);
  });

  it("splits a word-diff span across multiple syntax tokens", () => {
    const word = [{ text: "fooBar", changed: true }];
    const syntax = [
      { text: "foo", color: "#aaa" },
      { text: "Bar", color: "#bbb" },
    ];
    const merged = mergeWordDiffWithSyntax(word, syntax);
    expect(merged).toEqual([
      { text: "foo", color: "#aaa", changed: true },
      { text: "Bar", color: "#bbb", changed: true },
    ]);
  });

  it("round-trips full line text", () => {
    const line = 'return "hello";';
    const word = [
      { text: "return ", changed: false },
      { text: '"hello";', changed: true },
    ];
    const syntax = [
      { text: "return", color: "#c678dd" },
      { text: ' "hello";', color: "#98c379" },
    ];
    const merged = mergeWordDiffWithSyntax(word, syntax);
    expect(merged.map((s) => s.text).join("")).toBe(line);
  });
});
