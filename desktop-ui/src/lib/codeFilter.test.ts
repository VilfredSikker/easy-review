import { describe, expect, test } from "bun:test";
import { CODE_FILTER, nextCodeFilter } from "./codeFilter";

describe("nextCodeFilter", () => {
  test("turns the code filter on when no filter is active", () => {
    expect(nextCodeFilter(null)).toBe(CODE_FILTER);
    expect(nextCodeFilter("")).toBe(CODE_FILTER);
  });

  test("turns it off when it is the active filter", () => {
    expect(nextCodeFilter(CODE_FILTER)).toBeNull();
    expect(nextCodeFilter(` ${CODE_FILTER} `)).toBeNull();
  });

  test("replaces another active filter", () => {
    expect(nextCodeFilter("*.rs")).toBe(CODE_FILTER);
  });
});
