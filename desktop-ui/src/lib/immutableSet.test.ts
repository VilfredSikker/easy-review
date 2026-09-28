import { describe, expect, it } from "bun:test";
import { toggled, withAll, without } from "./immutableSet";

describe("immutableSet", () => {
  it("toggles a value in a copy", () => {
    const start = new Set(["a", "b"]);
    expect([...toggled(start, "c")]).toEqual(["a", "b", "c"]);
    expect([...toggled(start, "a")]).toEqual(["b"]);
    expect([...start]).toEqual(["a", "b"]);
  });

  it("adds and removes many values in a copy", () => {
    const start = new Set([1, 2]);
    expect([...withAll(start, [2, 3, 4])]).toEqual([1, 2, 3, 4]);
    expect([...without(start, [2, 9])]).toEqual([1]);
    expect([...start]).toEqual([1, 2]);
  });

  it("always returns a new Set, even when nothing changes", () => {
    const start = new Set([1]);
    expect(withAll(start, [])).not.toBe(start);
    expect(without(start, [])).not.toBe(start);
  });
});
