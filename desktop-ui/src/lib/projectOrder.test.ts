import { describe, expect, test } from "bun:test";
import { orderedByIds } from "./projectOrder";

const items = [{ id: "a" }, { id: "b" }, { id: "c" }];
const ids = (xs: { id: string }[]) => xs.map((x) => x.id);

describe("orderedByIds", () => {
  test("follows the given order", () => {
    expect(ids(orderedByIds(items, ["c", "a", "b"]))).toEqual(["c", "a", "b"]);
  });

  test("unknown ids are skipped and unmentioned items trail in their original order", () => {
    expect(ids(orderedByIds(items, ["zz", "b"]))).toEqual(["b", "a", "c"]);
  });

  test("a repeated id is placed once", () => {
    expect(ids(orderedByIds(items, ["b", "b"]))).toEqual(["b", "a", "c"]);
  });
});
