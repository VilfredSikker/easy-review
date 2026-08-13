import { describe, expect, it } from "bun:test";
import { destIndexAfterRemove, dropSlot } from "./listReorder";

describe("dropSlot", () => {
  it("returns the item index when the pointer is in the first half", () => {
    // Item 1 occupies [100, 140); midpoint 120.
    expect(dropSlot(110, 100, 40, 1)).toBe(1);
  });

  it("returns the next gap when the pointer is in the second half", () => {
    expect(dropSlot(130, 100, 40, 1)).toBe(2);
  });
});

describe("destIndexAfterRemove", () => {
  it("moves the first item to the end of a three-item list", () => {
    // Drop after the last item: slot 3, from 0 → dest 2.
    expect(destIndexAfterRemove(0, 3)).toBe(2);
  });

  it("moves the last item to the front", () => {
    expect(destIndexAfterRemove(2, 0)).toBe(0);
  });

  it("is a no-op when dropping on the source (before or after its midpoint)", () => {
    expect(destIndexAfterRemove(1, 1)).toBe(1);
    expect(destIndexAfterRemove(1, 2)).toBe(1);
  });
});
